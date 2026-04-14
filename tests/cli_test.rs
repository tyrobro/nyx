use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_start_command_runs_successfully() {
    let mut cmd = Command::cargo_bin("nyx").unwrap();
    cmd.arg("start");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Starting nyx session..."));
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
