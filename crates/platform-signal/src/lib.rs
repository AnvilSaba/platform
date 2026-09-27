use std::io;

/// 終了シグナルを登録し、受信時に `on_signal` を一度だけ実行する。
///
/// この関数が戻る前にシグナルの登録を終えるため、登録失敗は呼び出し元で
/// 起動時のエラーとして処理できる。
pub fn install_signal_handler<F>(on_signal: F) -> io::Result<()>
where
    F: FnOnce() + Send + 'static,
{
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;

        tokio::spawn(async move {
            tokio::select! {
                _ = sigint.recv() => {},
                _ = sigterm.recv() => {},
            }
            on_signal();
        });
    }

    #[cfg(windows)]
    {
        let mut ctrl_c = tokio::signal::windows::ctrl_c()?;

        tokio::spawn(async move {
            ctrl_c.recv().await;
            on_signal();
        });
    }

    Ok(())
}
