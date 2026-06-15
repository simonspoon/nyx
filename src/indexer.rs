use std::io::BufRead;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::db::Database;
use crate::error::Result;
use crate::models::Record;

/// Default location of Claude Code conversation data.
pub fn default_projects_dir() -> PathBuf {
    crate::db::home_dir().join(".claude").join("projects")
}

/// Index all JSONL files under the projects directory into the database.
/// Returns (files_indexed, files_skipped) count.
pub fn index_all(db: &mut Database, projects_dir: &Path) -> Result<(usize, usize)> {
    let mut indexed = 0;
    let mut skipped = 0;

    // Collect file entries first so we don't borrow across the transaction
    let entries: Vec<_> = WalkDir::new(projects_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .filter(|e| !e.path().to_string_lossy().ends_with(".meta.json"))
        .map(|e| e.path().to_path_buf())
        .collect();

    // Use rusqlite::Transaction for automatic rollback on error/drop
    let tx = db.conn.transaction()?;

    for path in &entries {
        // Check mtime for incremental indexing
        let metadata = std::fs::metadata(path)?;
        let mtime = metadata.modified()?;

        if !needs_reindex_tx(&tx, path, mtime)? {
            skipped += 1;
            continue;
        }

        // Determine session_id and project from path
        let (session_id, project) = extract_session_info(path, projects_dir);

        // Delete old data for this session if re-indexing
        delete_session_tx(&tx, &session_id)?;

        // Parse and index the file
        match index_file_tx(&tx, path, &session_id, &project) {
            Ok(()) => {
                record_source_file_tx(&tx, path, mtime, &session_id)?;
                indexed += 1;
            }
            Err(e) => {
                eprintln!("Warning: failed to index {}: {}", path.display(), e);
                // Continue with other files
            }
        }
    }

    tx.commit()?;

    Ok((indexed, skipped))
}

/// Check if a source file needs re-indexing (transaction-local version).
fn needs_reindex_tx(
    tx: &rusqlite::Transaction,
    path: &Path,
    current_mtime: std::time::SystemTime,
) -> Result<bool> {
    let path_str = path.to_string_lossy().to_string();
    let current_secs = current_mtime
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let result: std::result::Result<i64, _> = tx.query_row(
        "SELECT mtime_secs FROM source_files WHERE path = ?1",
        rusqlite::params![path_str],
        |row| row.get(0),
    );

    match result {
        Ok(stored_secs) => Ok(current_secs > stored_secs),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
        Err(e) => Err(crate::error::Error::Db(e)),
    }
}

/// Delete all data for a session (transaction-local version).
fn delete_session_tx(tx: &rusqlite::Transaction, session_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM messages WHERE session_id = ?1",
        rusqlite::params![session_id],
    )?;
    tx.execute(
        "DELETE FROM tool_calls WHERE session_id = ?1",
        rusqlite::params![session_id],
    )?;
    // NOTE: message_usage is deliberately NOT deleted here. It is deduplicated on
    // request_id (INSERT ... ON CONFLICT keeping the MAX per column), so it is safe
    // to re-run, and skipping the delete keeps usage from sibling files that share
    // this session_id (e.g. subagent transcripts) from collapsing to the last file.
    tx.execute(
        "DELETE FROM conversations WHERE session_id = ?1",
        rusqlite::params![session_id],
    )?;
    // NOTE: We deliberately do NOT delete from source_files here. Multiple files
    // (a main session file plus its subagent files) can share a session_id, so a
    // session-scoped delete would wipe siblings' rows and force them to be
    // re-indexed every run. record_source_file_tx uses INSERT OR REPLACE keyed on
    // the path PK, so the current file's mtime row is refreshed correctly.
    Ok(())
}

/// Record that a source file has been indexed (transaction-local version).
fn record_source_file_tx(
    tx: &rusqlite::Transaction,
    path: &Path,
    mtime: std::time::SystemTime,
    session_id: &str,
) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let mtime_secs = mtime
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    tx.execute(
        "INSERT OR REPLACE INTO source_files (path, mtime_secs, session_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![path_str, mtime_secs, session_id],
    )?;
    Ok(())
}

/// Extract session ID and project name from a JSONL file path.
fn extract_session_info(path: &Path, projects_dir: &Path) -> (String, String) {
    let relative = path.strip_prefix(projects_dir).unwrap_or(path);

    let components: Vec<&str> = relative
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Pattern: {encoded-cwd}/{session-uuid}.jsonl
    // Or: {encoded-cwd}/{session-uuid}/subagents/agent-{hash}.jsonl
    let project = if !components.is_empty() {
        decode_project_name(components[0])
    } else {
        "unknown".to_string()
    };

    // Session ID: the UUID part of the filename, or the parent directory name for subagents
    let session_id = if components.len() >= 3 && components.contains(&"subagents") {
        // Subagent: session_id is the directory above "subagents"
        components[1].to_string()
    } else if components.len() >= 2 {
        // Main session file: strip .jsonl extension
        let filename = components[1];
        filename
            .strip_suffix(".jsonl")
            .unwrap_or(filename)
            .to_string()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    (session_id, project)
}

/// Decode a project directory name back to a readable name.
/// e.g., "-Users-simonspoon-claudehub" -> "claudehub"
/// We take the last meaningful path component.
///
/// Note: hyphenated directory names (e.g., "my-project") are indistinguishable
/// from path separators in Claude Code's encoding, so project names containing
/// hyphens may be truncated to only the last segment.
fn decode_project_name(encoded: &str) -> String {
    // The encoded directory name uses dashes for path separators
    // e.g., "-Users-simonspoon-Documents-Development-opensource-dante"
    // We want the last component: "dante"
    let parts: Vec<&str> = encoded.split('-').filter(|s| !s.is_empty()).collect();
    parts.last().unwrap_or(&"unknown").to_string()
}

/// Index a single JSONL file (transaction-local version).
fn index_file_tx(
    tx: &rusqlite::Transaction,
    path: &Path,
    session_id: &str,
    project: &str,
) -> Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);

    // Insert placeholder conversation row first so FK constraints are satisfied
    upsert_conversation_tx(
        tx,
        session_id,
        None,
        project,
        None,
        None,
        &path.to_string_lossy(),
        None,
    )?;

    let mut slug: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut first_ts: Option<String> = None;
    let mut last_ts: Option<String> = None;

    // Map a tool_use_id to the row id of its tool_calls record, so a later
    // tool_result (usually on the next user message) can set is_error.
    let mut tool_row_by_use_id: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    // Map a tool_use_id to the observed is_error flag from a tool_result.
    let mut error_by_use_id: std::collections::HashMap<String, bool> = std::collections::HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let record: Record = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue, // Skip unparseable lines
        };

        // Track timestamps
        if let Some(ts) = record.timestamp() {
            let ts_owned = ts.to_string();
            if first_ts.is_none() || first_ts.as_ref().is_some_and(|f| ts_owned < *f) {
                first_ts = Some(ts_owned.clone());
            }
            if last_ts.is_none() || last_ts.as_ref().is_some_and(|l| ts_owned > *l) {
                last_ts = Some(ts_owned);
            }
        }

        // Extract slug (from assistant/system records)
        if slug.is_none() && record.slug().is_some() {
            slug = record.slug().map(|s| s.to_string());
        }

        // Extract custom title
        if let Record::CustomTitle {
            custom_title: Some(ref t),
            ..
        } = record
        {
            custom_title = Some(t.clone());
        }

        // Capture tool_use blocks as tool_calls rows.
        for (id, name, input) in record.tool_uses() {
            let Some(name) = name else { continue };
            let (input_str, truncated) = match input {
                Some(v) => {
                    let raw = v.to_string();
                    let (slice, trunc) = truncate_on_char_boundary(&raw, TOOL_INPUT_CAP);
                    (Some(slice.to_string()), trunc)
                }
                None => (None, false),
            };
            let row_id = insert_tool_call_tx(
                tx,
                session_id,
                record.timestamp(),
                id,
                name,
                input_str.as_deref(),
                truncated,
            )?;
            if let Some(id) = id {
                tool_row_by_use_id.insert(id.to_string(), row_id);
            }
        }

        // Capture tool_result is_error flags (resolved against tool_calls below).
        for (tool_use_id, is_error) in record.tool_results() {
            if let (Some(id), Some(err)) = (tool_use_id, is_error) {
                error_by_use_id.insert(id.to_string(), err);
            }
        }

        // Capture token usage independently of text. Text-less tool-only
        // assistant turns carry usage but produce no `messages` row.
        if let Some(usage) = record.usage() {
            insert_message_usage_tx(
                tx,
                session_id,
                record.timestamp(),
                record.request_id(),
                record.model(),
                usage,
            )?;
        }

        // Extract and store text content
        if let (Some(role), Some(text)) = (record.role(), record.extract_text())
            && !text.is_empty()
        {
            let record_type = match &record {
                Record::User { .. } => "user",
                Record::Assistant { .. } => "assistant",
                Record::System { .. } => "system",
                Record::CustomTitle { .. } => "custom-title",
                _ => "other",
            };
            insert_message_tx(tx, session_id, record.timestamp(), role, &text, record_type)?;
        }
    }

    // Resolve is_error flags now that the whole file has been read.
    for (use_id, err) in &error_by_use_id {
        if let Some(row_id) = tool_row_by_use_id.get(use_id) {
            tx.execute(
                "UPDATE tool_calls SET is_error = ?1 WHERE id = ?2",
                rusqlite::params![*err as i64, row_id],
            )?;
        }
    }

    // Upsert conversation metadata
    upsert_conversation_tx(
        tx,
        session_id,
        slug.as_deref(),
        project,
        first_ts.as_deref(),
        last_ts.as_deref(),
        &path.to_string_lossy(),
        custom_title.as_deref(),
    )?;

    Ok(())
}

/// Insert or update a conversation record (transaction-local version).
#[allow(clippy::too_many_arguments)]
fn upsert_conversation_tx(
    tx: &rusqlite::Transaction,
    session_id: &str,
    slug: Option<&str>,
    project: &str,
    first_ts: Option<&str>,
    last_ts: Option<&str>,
    source_path: &str,
    custom_title: Option<&str>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO conversations (session_id, slug, project, first_timestamp, last_timestamp, source_path, custom_title)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(session_id) DO UPDATE SET
            slug = COALESCE(?2, conversations.slug),
            first_timestamp = MIN(COALESCE(?4, conversations.first_timestamp), COALESCE(conversations.first_timestamp, ?4)),
            last_timestamp = MAX(COALESCE(?5, conversations.last_timestamp), COALESCE(conversations.last_timestamp, ?5)),
            custom_title = COALESCE(?7, conversations.custom_title)",
        rusqlite::params![session_id, slug, project, first_ts, last_ts, source_path, custom_title],
    )?;
    Ok(())
}

/// Insert a message record (transaction-local version).
fn insert_message_tx(
    tx: &rusqlite::Transaction,
    session_id: &str,
    timestamp: Option<&str>,
    role: &str,
    content: &str,
    record_type: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO messages (session_id, timestamp, role, content, record_type)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![session_id, timestamp, role, content, record_type],
    )?;
    Ok(())
}

/// Insert a per-assistant-message usage record (transaction-local version).
fn insert_message_usage_tx(
    tx: &rusqlite::Transaction,
    session_id: &str,
    timestamp: Option<&str>,
    request_id: Option<&str>,
    model: Option<&str>,
    usage: &crate::models::Usage,
) -> Result<()> {
    let cc = usage.cache_creation.as_ref();
    tx.execute(
        crate::db::MESSAGE_USAGE_UPSERT,
        rusqlite::params![
            session_id,
            timestamp,
            request_id,
            model,
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_creation_input_tokens,
            usage.cache_read_input_tokens,
            cc.and_then(|c| c.ephemeral_5m_input_tokens),
            cc.and_then(|c| c.ephemeral_1h_input_tokens),
            usage.service_tier.as_deref(),
            usage.speed.as_deref(),
        ],
    )?;
    Ok(())
}

/// Insert a tool-call record (transaction-local version).
fn insert_tool_call_tx(
    tx: &rusqlite::Transaction,
    session_id: &str,
    timestamp: Option<&str>,
    tool_use_id: Option<&str>,
    tool_name: &str,
    input: Option<&str>,
    input_truncated: bool,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO tool_calls (session_id, timestamp, tool_use_id, tool_name,
            input, input_truncated, is_error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
        rusqlite::params![
            session_id,
            timestamp,
            tool_use_id,
            tool_name,
            input,
            input_truncated as i64,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Truncate a string to at most `cap` bytes on a UTF-8 char boundary.
/// Returns the (possibly borrowed) slice and whether truncation occurred.
fn truncate_on_char_boundary(s: &str, cap: usize) -> (&str, bool) {
    if s.len() <= cap {
        return (s, false);
    }
    let mut end = cap;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

/// Maximum stored byte length of a tool call's serialized input.
const TOOL_INPUT_CAP: usize = 1024;

/// Index a single JSONL file (uses Database methods directly, for testing).
#[cfg(test)]
fn index_file(db: &Database, path: &Path, session_id: &str, project: &str) -> Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);

    // Insert placeholder conversation row first so FK constraints are satisfied
    db.upsert_conversation(
        session_id,
        None,
        project,
        None,
        None,
        &path.to_string_lossy(),
        None,
    )?;

    let mut slug: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut first_ts: Option<String> = None;
    let mut last_ts: Option<String> = None;

    // Map a tool_use_id to the row id of its tool_calls record, so a later
    // tool_result (usually on the next user message) can set is_error.
    let mut tool_row_by_use_id: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    // Map a tool_use_id to the observed is_error flag from a tool_result.
    let mut error_by_use_id: std::collections::HashMap<String, bool> = std::collections::HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let record: Record = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue, // Skip unparseable lines
        };

        // Track timestamps
        if let Some(ts) = record.timestamp() {
            let ts_owned = ts.to_string();
            if first_ts.is_none() || first_ts.as_ref().is_some_and(|f| ts_owned < *f) {
                first_ts = Some(ts_owned.clone());
            }
            if last_ts.is_none() || last_ts.as_ref().is_some_and(|l| ts_owned > *l) {
                last_ts = Some(ts_owned);
            }
        }

        // Extract slug (from assistant/system records)
        if slug.is_none() && record.slug().is_some() {
            slug = record.slug().map(|s| s.to_string());
        }

        // Extract custom title
        if let Record::CustomTitle {
            custom_title: Some(ref t),
            ..
        } = record
        {
            custom_title = Some(t.clone());
        }

        // Capture tool_use blocks as tool_calls rows.
        for (id, name, input) in record.tool_uses() {
            let Some(name) = name else { continue };
            let (input_str, truncated) = match input {
                Some(v) => {
                    let raw = v.to_string();
                    let (slice, trunc) = truncate_on_char_boundary(&raw, TOOL_INPUT_CAP);
                    (Some(slice.to_string()), trunc)
                }
                None => (None, false),
            };
            let row_id = db.insert_tool_call(
                session_id,
                record.timestamp(),
                id,
                name,
                input_str.as_deref(),
                truncated,
                None,
            )?;
            if let Some(id) = id {
                tool_row_by_use_id.insert(id.to_string(), row_id);
            }
        }

        // Capture tool_result is_error flags (resolved against tool_calls below).
        for (tool_use_id, is_error) in record.tool_results() {
            if let (Some(id), Some(err)) = (tool_use_id, is_error) {
                error_by_use_id.insert(id.to_string(), err);
            }
        }

        // Capture token usage independently of text. Text-less tool-only
        // assistant turns carry usage but produce no `messages` row.
        if let Some(usage) = record.usage() {
            db.insert_message_usage(
                session_id,
                record.timestamp(),
                record.request_id(),
                record.model(),
                usage,
            )?;
        }

        // Extract and store text content
        if let (Some(role), Some(text)) = (record.role(), record.extract_text())
            && !text.is_empty()
        {
            let record_type = match &record {
                Record::User { .. } => "user",
                Record::Assistant { .. } => "assistant",
                Record::System { .. } => "system",
                Record::CustomTitle { .. } => "custom-title",
                _ => "other",
            };
            db.insert_message(session_id, record.timestamp(), role, &text, record_type)?;
        }
    }

    // Resolve is_error flags now that the whole file has been read.
    for (use_id, err) in &error_by_use_id {
        if let Some(row_id) = tool_row_by_use_id.get(use_id) {
            db.conn.execute(
                "UPDATE tool_calls SET is_error = ?1 WHERE id = ?2",
                rusqlite::params![*err as i64, row_id],
            )?;
        }
    }

    // Upsert conversation metadata
    db.upsert_conversation(
        session_id,
        slug.as_deref(),
        project,
        first_ts.as_deref(),
        last_ts.as_deref(),
        &path.to_string_lossy(),
        custom_title.as_deref(),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_project_name() {
        assert_eq!(
            decode_project_name("-Users-simonspoon-claudehub"),
            "claudehub"
        );
        assert_eq!(
            decode_project_name("-Users-simonspoon-Documents-Development-opensource-dante"),
            "dante"
        );
        assert_eq!(decode_project_name("-private-tmp"), "tmp");
    }

    #[test]
    fn test_extract_session_info() {
        let projects_dir = Path::new("/home/user/.claude/projects");

        let (sid, proj) = extract_session_info(
            Path::new("/home/user/.claude/projects/-Users-user-myproject/abc-123.jsonl"),
            projects_dir,
        );
        assert_eq!(sid, "abc-123");
        assert_eq!(proj, "myproject");

        // Subagent path
        let (sid, proj) = extract_session_info(
            Path::new(
                "/home/user/.claude/projects/-Users-user-myproject/session-uuid/subagents/agent-hash.jsonl",
            ),
            projects_dir,
        );
        assert_eq!(sid, "session-uuid");
        assert_eq!(proj, "myproject");
    }

    #[test]
    fn test_index_file_with_test_data() {
        let db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        let data = r#"{"type":"user","message":{"role":"user","content":"hello world"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"sess-1"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hi there!"}],"model":"claude-opus-4-6"},"timestamp":"2026-03-20T01:01:00Z","sessionId":"sess-1","slug":"test-slug"}
{"type":"progress","data":{"type":"hook_progress"},"toolUseID":"abc"}
{"type":"system","subtype":"stop_hook","timestamp":"2026-03-20T01:02:00Z","sessionId":"sess-1","slug":"test-slug"}
"#;
        std::fs::write(&jsonl_path, data).unwrap();

        index_file(&db, &jsonl_path, "sess-1", "test-project").unwrap();

        let stats = db.stats().unwrap();
        assert_eq!(stats.conversation_count, 1);
        // user + assistant + system = 3 messages (progress is skipped)
        assert_eq!(stats.message_count, 3);

        // Verify slug was captured
        let slug: String = db
            .conn
            .query_row(
                "SELECT slug FROM conversations WHERE session_id = 'sess-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(slug, "test-slug");
    }

    #[test]
    fn test_index_all_incremental() {
        let mut db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // Create a fake project structure
        let project_dir = dir.path().join("-Users-test-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let jsonl_path = project_dir.join("session-1.jsonl");
        let data = r#"{"type":"user","message":{"role":"user","content":"test"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"session-1"}
"#;
        std::fs::write(&jsonl_path, data).unwrap();

        // First index
        let (indexed, skipped) = index_all(&mut db, dir.path()).unwrap();
        assert_eq!(indexed, 1);
        assert_eq!(skipped, 0);

        // Second index (no changes) should skip
        let (indexed, skipped) = index_all(&mut db, dir.path()).unwrap();
        assert_eq!(indexed, 0);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn test_index_all_incremental_shared_session_id() {
        let mut db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // Create a fake project structure with a main session file AND a
        // subagent file that maps to the same session_id ("session-1").
        let project_dir = dir.path().join("-Users-test-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let main_path = project_dir.join("session-1.jsonl");
        let main_data = r#"{"type":"user","message":{"role":"user","content":"main"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"session-1"}
"#;
        std::fs::write(&main_path, main_data).unwrap();

        let subagent_dir = project_dir.join("session-1").join("subagents");
        std::fs::create_dir_all(&subagent_dir).unwrap();
        let subagent_path = subagent_dir.join("agent-abc.jsonl");
        let subagent_data = r#"{"type":"user","message":{"role":"user","content":"sub"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"session-1"}
"#;
        std::fs::write(&subagent_path, subagent_data).unwrap();

        // First index: both files are new -> both indexed.
        let (indexed, skipped) = index_all(&mut db, dir.path()).unwrap();
        assert_eq!(indexed, 2);
        assert_eq!(skipped, 0);

        // Second index with no changes: both files should be skipped.
        // Before the fix, delete_session_tx wiped the source_files rows of
        // siblings sharing session_id "session-1", so both were re-indexed.
        let (indexed, skipped) = index_all(&mut db, dir.path()).unwrap();
        assert_eq!(indexed, 0);
        assert_eq!(skipped, 2);
    }

    #[test]
    fn test_extract_session_info_bare_file() {
        // Edge case: file directly in the projects dir (no project subdirectory)
        let projects_dir = Path::new("/home/user/.claude/projects");
        let (sid, proj) = extract_session_info(
            Path::new("/home/user/.claude/projects/orphan.jsonl"),
            projects_dir,
        );
        // Single component: project decode is "orphan.jsonl", session_id fallback is file_stem
        assert_eq!(sid, "orphan");
        assert!(!proj.is_empty());
    }

    #[test]
    fn test_index_file_with_custom_title() {
        let db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        let data = r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"sess-ct"}
{"type":"custom-title","customTitle":"My Custom Title","sessionId":"sess-ct"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"response"}],"model":"m"},"timestamp":"2026-03-20T01:01:00Z","sessionId":"sess-ct","slug":"test-slug"}
"#;
        std::fs::write(&jsonl_path, data).unwrap();

        index_file(&db, &jsonl_path, "sess-ct", "test-project").unwrap();

        // Verify custom title was captured
        let title: Option<String> = db
            .conn
            .query_row(
                "SELECT custom_title FROM conversations WHERE session_id = 'sess-ct'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, Some("My Custom Title".to_string()));
    }

    #[test]
    fn test_index_file_with_unparseable_lines() {
        let db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        let data = r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"sess-up"}
not valid json at all
{"type":"user","message":{"role":"user","content":"second msg"},"timestamp":"2026-03-20T01:02:00Z","sessionId":"sess-up"}

"#;
        std::fs::write(&jsonl_path, data).unwrap();

        index_file(&db, &jsonl_path, "sess-up", "test-project").unwrap();

        // Should have 2 messages (skipping the unparseable line and the blank line)
        let stats = db.stats().unwrap();
        assert_eq!(stats.message_count, 2);
    }

    #[test]
    fn test_index_file_timestamp_ordering() {
        let db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        // Put later timestamp first to test min/max tracking
        let data = r#"{"type":"user","message":{"role":"user","content":"later"},"timestamp":"2026-03-20T05:00:00Z","sessionId":"sess-ts"}
{"type":"user","message":{"role":"user","content":"earlier"},"timestamp":"2026-03-20T01:00:00Z","sessionId":"sess-ts"}
"#;
        std::fs::write(&jsonl_path, data).unwrap();

        index_file(&db, &jsonl_path, "sess-ts", "test-project").unwrap();

        let (first_ts, last_ts): (Option<String>, Option<String>) = db
            .conn
            .query_row(
                "SELECT first_timestamp, last_timestamp FROM conversations WHERE session_id = 'sess-ts'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(first_ts, Some("2026-03-20T01:00:00Z".to_string()));
        assert_eq!(last_ts, Some("2026-03-20T05:00:00Z".to_string()));
    }

    #[test]
    fn test_decode_project_name_empty() {
        // Edge case: empty string
        assert_eq!(decode_project_name(""), "unknown");
    }

    #[test]
    fn test_truncate_on_char_boundary() {
        let (s, t) = truncate_on_char_boundary("hello", 10);
        assert_eq!(s, "hello");
        assert!(!t);

        // Multi-byte char straddling the cap is not split.
        let input = "aaa\u{e9}"; // 'é' is 2 bytes; total 5 bytes
        let (s, t) = truncate_on_char_boundary(input, 4);
        assert_eq!(s, "aaa");
        assert!(t);
    }

    #[test]
    fn test_index_file_captures_usage_and_tool_calls() {
        let db = Database::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jsonl_path = dir.path().join("test.jsonl");

        // Line 1: text + tool_use assistant turn (req_1). Line 2: tool_result.
        // Lines 3-4: ONE billed response (req_2) split across two blocks — a
        // text-LESS tool-only turn that produces no `messages` row, with
        // output_tokens growing to its final value on the last block.
        let data = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reading"},{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/tmp/x"}}],"model":"claude-opus-4-8","usage":{"input_tokens":2832,"output_tokens":1160,"cache_creation_input_tokens":23317,"cache_read_input_tokens":0,"service_tier":"standard","speed":"standard","cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":23317}}},"timestamp":"2026-06-14T01:00:00Z","sessionId":"sess-u","slug":"s","requestId":"req_1"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"failed","is_error":true}]},"timestamp":"2026-06-14T01:00:01Z","sessionId":"sess-u"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"ls"}}],"model":"claude-opus-4-8","usage":{"input_tokens":500,"output_tokens":40}},"timestamp":"2026-06-14T01:00:02Z","sessionId":"sess-u","requestId":"req_2"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_3","name":"Bash","input":{"command":"ls"}}],"model":"claude-opus-4-8","usage":{"input_tokens":500,"output_tokens":200}},"timestamp":"2026-06-14T01:00:03Z","sessionId":"sess-u","requestId":"req_2"}
"#;
        std::fs::write(&jsonl_path, data).unwrap();

        index_file(&db, &jsonl_path, "sess-u", "test-project").unwrap();

        // Usage captured for the text-bearing assistant turn.
        let input_tokens: Option<i64> = db
            .conn
            .query_row(
                "SELECT input_tokens FROM message_usage WHERE request_id = 'req_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(input_tokens, Some(2832));

        // The two req_2 lines collapse to ONE row deduped on request_id; the
        // text-less turn's usage is captured even with no `messages` row.
        let usage_rows: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM message_usage", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_rows, 2);
        // Only req_1 carried text, so only one assistant `messages` row exists.
        let assistant_msgs: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE role = 'assistant'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assistant_msgs, 1);
        // req_2 keeps the MAX output (200, not the partial 40, and not summed).
        let req2_out: i64 = db
            .conn
            .query_row(
                "SELECT output_tokens FROM message_usage WHERE request_id = 'req_2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(req2_out, 200);
        // Totals reflect deduped usage: 2832 + 500 input, 1160 + 200 output.
        let (total_input, total_output): (i64, i64) = db
            .conn
            .query_row(
                "SELECT SUM(input_tokens), SUM(output_tokens) FROM message_usage",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(total_input, 3332);
        assert_eq!(total_output, 1360);

        // tool_calls row with name, resolved is_error, and untruncated input.
        let (name, is_error, truncated): (String, Option<i64>, i64) = db
            .conn
            .query_row(
                "SELECT tool_name, is_error, input_truncated FROM tool_calls WHERE tool_use_id = 'toolu_1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "Read");
        assert_eq!(is_error, Some(1));
        assert_eq!(truncated, 0);
    }
}
