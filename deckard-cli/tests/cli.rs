use assert_cmd::Command;
use predicates::prelude::*;
use std::env;
use std::path::{Path, PathBuf};

/// Helper to get path of test_files directory
fn test_files_path() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("test_files")
        .canonicalize()
        .expect("test_files path does not exist")
}

#[test]
fn fail_on_bad_path() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.args(["unknow_path"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No valid paths provided"));
}

#[test]
fn dont_fail_one_good_path() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.args([test_files_path().as_os_str(), "unknow_path".as_ref()])
        .assert()
        .success();
}

#[test]
fn test_same_files() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    let base_path = test_files_path().join("same_files");
    let file_a = base_path.join("file_a.txt");
    let file_b = base_path.join("file_b.txt");
    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .and(predicate::str::contains(file_b.to_string_lossy()));

    cmd.arg("--json")
        .args([file_a.as_os_str(), file_b.as_os_str()])
        .assert()
        .success()
        .stdout(predicate);
}

#[test]
fn test_similar_files() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    let base_path = test_files_path().join("similar_files");
    let file_a = base_path.join("file_a.txt");
    let file_b = base_path.join("file_b.txt");
    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .or(predicate::str::contains(file_b.to_string_lossy()))
        .not();

    cmd.arg("--json")
        .args([file_a.as_os_str(), file_b.as_os_str()])
        .assert()
        .success()
        .stdout(predicate);
}

#[test]
fn test_skip_hidden_files() {
    let base_path = test_files_path().join(".hidden_dir");
    let file_a = base_path.join(".hidden_a");
    let file_b = base_path.join(".hidden_b");

    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .or(predicate::str::contains(file_b.to_string_lossy()));

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("--json")
        .args([base_path.as_os_str()])
        .assert()
        .success()
        .stdout(predicate);

    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .or(predicate::str::contains(file_b.to_string_lossy()));

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("--json")
        .arg("--skip_hidden")
        .args([base_path.as_os_str()])
        .assert()
        .success()
        .stdout(predicate.not());
}

#[test]
fn test_skip_empty_files() {
    let base_path = test_files_path().join("empty");
    let file_a = base_path.join("emptyfile_a");
    let file_b = base_path.join("emptyfile_b");

    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .or(predicate::str::contains(file_b.to_string_lossy()));

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("--json")
        .args([base_path.as_os_str()])
        .assert()
        .success()
        .stdout(predicate);

    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .or(predicate::str::contains(file_b.to_string_lossy()));

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("--json")
        .arg("--skip_empty")
        .args([base_path.as_os_str()])
        .assert()
        .success()
        .stdout(predicate.not());
}
