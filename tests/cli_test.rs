use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_start_command_runs_successfully() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();
    cmd.arg("start")
        .write_stdin("nyx exit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Your ID: "))
        .stdout(predicate::str::contains(
            "Keep your private key secure and do not share it.",
        ));
}

#[test]
fn test_invalid_command_fails() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();
    cmd.arg("invalid command");
    cmd.assert().failure();
}

#[test]
fn test_connect_command_requires_id() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();
    cmd.arg("connect");
    cmd.assert().failure();
}

#[test]
fn test_connect_command_runs_successfully_with_id() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();
    cmd.arg("connect").arg("8F3A-92KD-XX12");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Connecting to 8F3A-92KD-XX12"));
}

#[test]
fn test_start_session_loop_exits_cleanly() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();

    cmd.arg("start")
        .write_stdin("nyx exit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Session ended."));
}

#[test]
fn test_serve_command_runs_successfully() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();

    cmd.arg("host");

    cmd.assert().success().stdout(predicate::str::contains(
        "This node is now a Nyx Local Server",
    ));
}
