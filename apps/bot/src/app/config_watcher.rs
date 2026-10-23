use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use futures::future::BoxFuture;
use tokio::task::JoinHandle;
use tracing::{error, info};

use super::AppError;

type ConfigHandler = dyn for<'a> Fn(&'a str) -> BoxFuture<'a, Result<(), AppError>> + Send;

#[derive(Default)]
pub struct ConfigWatcher {
    files: Vec<WatchedFile>,
}

struct WatchedFile {
    path: PathBuf,
    previous: Option<String>,
    handler: Box<ConfigHandler>,
}

impl ConfigWatcher {
    pub fn register<F>(&mut self, path: &Path, handler: F)
    where
        F: for<'a> Fn(&'a str) -> BoxFuture<'a, Result<(), AppError>> + Send + 'static,
    {
        self.files.push(WatchedFile {
            path: path.to_owned(),
            previous: None,
            handler: Box::new(handler),
        });
    }

    pub fn start(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            self.poll().await;
        }
    }

    async fn poll(&mut self) {
        for file in &mut self.files {
            let contents = match tokio::fs::read_to_string(&file.path).await {
                Ok(contents) => contents,
                Err(error) => {
                    error!(path = %file.path.display(), "Failed to read configuration; keeping current value: {error}");
                    continue;
                }
            };

            if file.previous.as_ref() == Some(&contents) {
                continue;
            }

            match (file.handler)(&contents).await {
                Ok(()) => {
                    file.previous = Some(contents);
                    info!(path = %file.path.display(), "Configuration reloaded");
                }
                Err(error) => {
                    error!(path = %file.path.display(), "Failed to reload configuration; keeping current value: {error:#}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatches_only_changed_files_and_continues_after_read_failure() {
        let path = std::env::temp_dir().join(format!("bot-config-poll-{}", rand::random::<u64>()));
        let other_path = path.with_extension("other");
        tokio::fs::write(&path, "initial").await.unwrap();
        tokio::fs::write(&other_path, "other").await.unwrap();
        let (loaded, mut loads) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = ConfigWatcher::default();
        let other_loaded = loaded.clone();
        watcher.register(&path, move |contents| {
            loaded.send(contents.to_owned()).unwrap();
            Box::pin(std::future::ready(Ok(())))
        });
        watcher.register(&other_path, move |contents| {
            let loaded = other_loaded.clone();
            Box::pin(async move {
                loaded.send(contents.to_owned()).unwrap();
                Ok(())
            })
        });
        watcher.poll().await;
        assert_eq!(loads.try_recv().unwrap(), "initial");
        assert_eq!(loads.try_recv().unwrap(), "other");

        watcher.poll().await;
        assert!(loads.try_recv().is_err());
        tokio::fs::write(&path, "updated").await.unwrap();
        watcher.poll().await;
        assert_eq!(loads.try_recv().unwrap(), "updated");
        assert!(loads.try_recv().is_err());

        tokio::fs::remove_file(&path).await.unwrap();
        tokio::fs::write(&other_path, "other-updated").await.unwrap();
        watcher.poll().await;
        assert_eq!(loads.try_recv().unwrap(), "other-updated");
        assert!(loads.try_recv().is_err());
        tokio::fs::write(&path, "recovered").await.unwrap();
        watcher.poll().await;
        assert_eq!(loads.try_recv().unwrap(), "recovered");
        assert!(loads.try_recv().is_err());
        tokio::fs::remove_file(path).await.unwrap();
        tokio::fs::remove_file(other_path).await.unwrap();
    }

    #[tokio::test]
    async fn retries_contents_until_handler_succeeds() {
        let path = std::env::temp_dir().join(format!("bot-config-retry-{}", rand::random::<u64>()));
        tokio::fs::write(&path, "updated").await.unwrap();
        let (loaded, mut loads) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = ConfigWatcher::default();
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        watcher.register(&path, move |contents| {
            loaded.send(contents.to_owned()).unwrap();
            let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(std::future::ready(if attempt == 0 {
                Err(anyhow::anyhow!("rejected"))
            } else {
                Ok(())
            }))
        });

        watcher.poll().await;
        assert_eq!(loads.try_recv().unwrap(), "updated");
        watcher.poll().await;
        assert_eq!(loads.try_recv().unwrap(), "updated");
        watcher.poll().await;
        assert!(loads.try_recv().is_err());
        tokio::fs::remove_file(path).await.unwrap();
    }
}
