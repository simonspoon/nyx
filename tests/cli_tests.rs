use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::TempDir;

/// Helper to create a test JSONL file with realistic data.
fn create_test_data(dir: &TempDir) -> std::path::PathBuf {
    let project_dir = dir.path().join("-Users-testuser-myproject");
    std::fs::create_dir_all(&project_dir).unwrap();

    let session_file = project_dir.join("abc-def-123.jsonl");
    let mut f = std::fs::File::create(&session_file).unwrap();

    // User message with string content
    writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"How do I implement a linked list in Rust?"}},"timestamp":"2026-03-20T10:00:00Z","sessionId":"abc-def-123"}}"#).unwrap();

    // Assistant message with text block
    writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"Here is how to implement a linked list in Rust using Box and Option."}}],"model":"claude-opus-4-6"}},"timestamp":"2026-03-20T10:01:00Z","sessionId":"abc-def-123","slug":"clever-coding-fox"}}"#).unwrap();

    // User message with array content (tool_result)
    writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_abc","content":"file contents here"}}]}},"timestamp":"2026-03-20T10:02:00Z","sessionId":"abc-def-123"}}"#).unwrap();

    // Progress record (should be skipped)
    writeln!(
        f,
        r#"{{"type":"progress","data":{{"type":"hook_progress"}},"toolUseID":"xyz"}}"#
    )
    .unwrap();

    // System record
    writeln!(f, r#"{{"type":"system","subtype":"stop_hook","timestamp":"2026-03-20T10:03:00Z","sessionId":"abc-def-123","slug":"clever-coding-fox"}}"#).unwrap();

    dir.path().to_path_buf()
}

/// Create a second session for multi-conversation tests.
fn add_second_session(dir: &TempDir) {
    let project_dir = dir.path().join("-Users-testuser-otherproject");
    std::fs::create_dir_all(&project_dir).unwrap();

    let session_file = project_dir.join("xyz-789.jsonl");
    let mut f = std::fs::File::create(&session_file).unwrap();

    writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"Tell me about SQLite full text search"}},"timestamp":"2026-03-19T08:00:00Z","sessionId":"xyz-789"}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"SQLite FTS5 is a powerful full-text search extension."}}],"model":"claude-opus-4-6"}},"timestamp":"2026-03-19T08:01:00Z","sessionId":"xyz-789","slug":"bright-searching-owl"}}"#).unwrap();
}

fn nyx_cmd() -> Command {
    Command::cargo_bin("nyx").unwrap()
}

#[test]
fn test_help_output() {
    nyx_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Index and search Claude Code conversation history",
        ))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("index"))
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("show"));
}

#[test]
fn test_version_output() {
    nyx_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("nyx"));
}

#[test]
fn test_status_without_index() {
    // Use a nonexistent DB path
    let dir = TempDir::new().unwrap();
    nyx_cmd()
        .env("HOME", dir.path())
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No index found"));
}

#[test]
fn test_index_and_status() {
    let dir = TempDir::new().unwrap();
    let data_dir = create_test_data(&dir);

    // Set up HOME so nyx creates its DB in the temp dir
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Create the claude projects dir structure
    let claude_projects = home.join(".claude").join("projects");
    std::fs::create_dir_all(&claude_projects).unwrap();

    // Copy test data into the claude projects dir
    let src_project = data_dir.join("-Users-testuser-myproject");
    let dst_project = claude_projects.join("-Users-testuser-myproject");
    copy_dir(&src_project, &dst_project);

    // Index
    nyx_cmd()
        .env("HOME", &home)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Indexed 1 files"));

    // Status
    nyx_cmd()
        .env("HOME", &home)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("conversations"))
        .stdout(predicate::str::contains("myproject"));
}

#[test]
fn test_search() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home(&dir);

    nyx_cmd()
        .env("HOME", &home)
        .args(["search", "linked list"])
        .assert()
        .success()
        // FTS5 highlights matches, so "linked list" gets ANSI codes inserted
        .stdout(predicate::str::contains("linked"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("clever-coding-fox"));
}

#[test]
fn test_search_with_project_filter() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home_multi(&dir);

    // Search in myproject only
    nyx_cmd()
        .env("HOME", &home)
        .args(["search", "linked list", "--project", "myproject"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked"))
        .stdout(predicate::str::contains("myproject"));

    // Search in otherproject — should not find "linked list"
    nyx_cmd()
        .env("HOME", &home)
        .args(["search", "linked list", "--project", "otherproject"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No results"));
}

#[test]
fn test_list() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home(&dir);

    nyx_cmd()
        .env("HOME", &home)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("clever-coding-fox"))
        .stdout(predicate::str::contains("myproject"));
}

#[test]
fn test_show() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home(&dir);

    nyx_cmd()
        .env("HOME", &home)
        .args(["show", "clever-coding-fox"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked list"))
        .stdout(predicate::str::contains("clever-coding-fox"));
}

#[test]
fn test_show_not_found() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home(&dir);

    nyx_cmd()
        .env("HOME", &home)
        .args(["show", "nonexistent-slug"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_json_output() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home(&dir);

    // Status JSON
    nyx_cmd()
        .env("HOME", &home)
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("message_count"))
        .stdout(predicate::str::contains("conversation_count"));

    // List JSON
    nyx_cmd()
        .env("HOME", &home)
        .args(["--json", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session_id"));

    // Search JSON
    nyx_cmd()
        .env("HOME", &home)
        .args(["--json", "search", "linked list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("snippet"));
}

#[test]
fn test_incremental_index() {
    let dir = TempDir::new().unwrap();
    let home = setup_indexed_home(&dir);

    // Re-index: should skip the already-indexed file
    nyx_cmd()
        .env("HOME", &home)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("skipped"));
}

// --- Helpers ---

fn setup_indexed_home(dir: &TempDir) -> std::path::PathBuf {
    let data_dir = create_test_data(dir);
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let claude_projects = home.join(".claude").join("projects");
    std::fs::create_dir_all(&claude_projects).unwrap();

    let src_project = data_dir.join("-Users-testuser-myproject");
    let dst_project = claude_projects.join("-Users-testuser-myproject");
    copy_dir(&src_project, &dst_project);

    // Run index
    nyx_cmd().env("HOME", &home).arg("index").assert().success();

    home
}

fn setup_indexed_home_multi(dir: &TempDir) -> std::path::PathBuf {
    let data_dir = create_test_data(dir);
    add_second_session(dir);

    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let claude_projects = home.join(".claude").join("projects");
    std::fs::create_dir_all(&claude_projects).unwrap();

    // Copy both project dirs
    let src1 = data_dir.join("-Users-testuser-myproject");
    let dst1 = claude_projects.join("-Users-testuser-myproject");
    copy_dir(&src1, &dst1);

    let src2 = dir.path().join("-Users-testuser-otherproject");
    let dst2 = claude_projects.join("-Users-testuser-otherproject");
    copy_dir(&src2, &dst2);

    nyx_cmd().env("HOME", &home).arg("index").assert().success();

    home
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}
