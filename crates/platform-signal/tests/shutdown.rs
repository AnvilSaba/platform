#[cfg(windows)]
#[tokio::test]
async fn registration_waits_for_signal() {
    let mut receiver = platform_signal::shutdown_receiver().unwrap();
    tokio::task::yield_now().await;
    assert_eq!(
        receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_notifies_shutdown_receiver() {
    let receiver = platform_signal::shutdown_receiver().unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
        .await
        .unwrap()
        .unwrap();
}
