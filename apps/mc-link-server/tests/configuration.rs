use std::process::Command;

#[test]
fn invalid_input_timeout_is_rejected_before_database_connection() {
    for value in ["0", "-1", "NaN", "inf", "not-a-number"] {
        let output = Command::new(env!("CARGO_BIN_EXE_mc-link-server"))
            .env("MC_LINK_SERVER_INPUT_TIMEOUT_SECONDS", value)
            .env("MC_LINK_SERVER_LISTEN", "127.0.0.1:0")
            .env_remove("DATABASE_URL")
            .output()
            .unwrap();
        assert!(!output.status.success(), "timeout={value}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!error.contains("Environment(NotPresent)"), "{error}");
    }
}

#[test]
fn positive_and_default_timeouts_reach_database_configuration() {
    for value in [None, Some("0.5"), Some("300"), Some("3600")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mc-link-server"));
        command
            .env("MC_LINK_SERVER_LISTEN", "127.0.0.1:0")
            .env_remove("DATABASE_URL")
            .env_remove("MC_LINK_SERVER_INPUT_TIMEOUT_SECONDS");
        if let Some(value) = value {
            command.env("MC_LINK_SERVER_INPUT_TIMEOUT_SECONDS", value);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("Environment(NotPresent)"), "{error}");
    }
}
