use assert_cmd::Command;
use deckard::config::SearchConfig;
use predicates::prelude::*;
use std::env;
use std::path::{Path, PathBuf};

#[test]
fn scan_depth_reads_config_without_depth() {
    let config = SearchConfig {
        skip_hidden: true,
        ..SearchConfig::default()
    };
    let serialized = serde_json::to_value(config).unwrap();
    assert!(serialized.get("depth").is_none());
    let config: SearchConfig = serde_json::from_value(serialized).unwrap();
    assert!(config.skip_hidden);
    assert_eq!(config.depth, None);
}

#[test]
fn scan_depth_limits_disk_usage() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let config_dir = env::temp_dir().join(format!("deckard-depth-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&config_dir).unwrap();
    let root = test_files_path().join("folder");
    let subfolder = root.join("subfolder");
    let output = Command::cargo_bin(env!("CARGO_PKG_NAME"))
        .unwrap()
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("APPDATA", &config_dir)
        .args(["-D", "-d", "1", "--json"])
        .arg(&root)
        .arg(&subfolder)
        .output();
    std::fs::remove_dir_all(config_dir).unwrap();
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut rows: Vec<(PathBuf, u64)> = serde_json::from_slice(&output.stdout).unwrap();
    rows.sort_unstable();
    let expected = [root.join("another.txt"), subfolder.join("midfile")].map(|path| {
        let size = path.metadata().unwrap().len();
        (path, size)
    });
    assert_eq!(rows, expected);
}

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

#[test]
fn test_audio_duplicates() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    let base_path = test_files_path().join("audio");
    let file_a = base_path.join("440_44khz_16b_mono.flac");
    let file_b = base_path.join("440_44khz_24b_mono.flac");
    let predicate = predicate::str::contains(file_a.to_string_lossy())
        .and(predicate::str::contains(file_b.to_string_lossy()));

    cmd.arg("--json")
        .arg("--check_audio")
        .args([base_path.as_os_str()])
        .assert()
        .success()
        .stdout(predicate);
}

#[test]
fn duplicate_names_config_defaults() {
    for (section, expected) in [
        (None, (true, false)),
        (
            Some(serde_json::json!({"ignore_track_numbers": false})),
            (false, false),
        ),
        (
            Some(serde_json::json!({"ignore_separators": true})),
            (true, true),
        ),
    ] {
        let mut value = serde_json::to_value(SearchConfig::default()).unwrap();
        let fields = value.as_object_mut().unwrap();
        fields.remove("name_config");
        if let Some(section) = section {
            fields.insert("name_config".into(), section);
        }
        let config: SearchConfig = serde_json::from_value(value).unwrap();
        assert_eq!(
            (
                config.name_config.ignore_track_numbers,
                config.name_config.ignore_separators
            ),
            expected
        );
    }
}

#[test]
fn duplicate_names_reports_ordered_paths() {
    let root = env::temp_dir().join(format!("deckard-names-cli-{}", std::process::id()));
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let names = [
        "album/Avenquill - 04 Thimblewake.wav",
        "pre-order/avenquill - Thimblewake.flac",
        "album/COVER.jpg",
        "pre-order/cover.png",
        "bonus/cover.gif",
        "solo",
    ];
    let paths = names.map(|name| data.join(name));
    for (i, path) in paths.iter().enumerate() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0; i + 1]).unwrap();
    }
    let run = |extra: &[&str]| {
        let out = Command::cargo_bin(env!("CARGO_PKG_NAME"))
            .unwrap()
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("APPDATA", root.join("config"))
            .args(["--duplicate_names", "--json", "--check_audio"])
            .args(extra)
            .arg(&data)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Vec<Vec<PathBuf>>>(&out.stdout).unwrap()
    };
    let all = run(&[]);
    let filtered = run(&["-f", "*Thimblewake*"]);
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(
        all,
        vec![
            paths[..2].to_vec(),
            vec![paths[2].clone(), paths[4].clone(), paths[3].clone()]
        ]
    );
    assert_eq!(filtered, vec![paths[..2].to_vec()]);
}
