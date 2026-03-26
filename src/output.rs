use std::path::Path;

use serde_json::json;

use crate::db::{Database, DbStats};
use crate::friction::{FrictionMatch, FrictionSummary};
use crate::search::{ConversationEntry, SearchResult, TranscriptMessage};

/// Format and print status output.
pub fn print_status(db: &Database, stats: &DbStats, source_size: u64, json: bool) {
    if json {
        print_status_json(db, stats, source_size);
    } else {
        print_status_human(db, stats, source_size);
    }
}

fn print_status_human(db: &Database, stats: &DbStats, source_size: u64) {
    let db_size = std::fs::metadata(&db.path).map(|m| m.len()).unwrap_or(0);

    let last_updated = db.last_updated().unwrap_or_else(|| "unknown".to_string());

    println!(
        "Index: {} (last updated: {})",
        db.path.display(),
        last_updated
    );
    println!(
        "Records: {} across {} conversations in {} projects",
        stats.message_count, stats.conversation_count, stats.project_count
    );

    let first = stats
        .first_date
        .as_deref()
        .map(format_date_short)
        .unwrap_or_else(|| "?".to_string());
    let last = stats
        .last_date
        .as_deref()
        .map(format_date_short)
        .unwrap_or_else(|| "?".to_string());
    println!("Date range: {} to {}", first, last);
    println!(
        "Size: {} source, {} index",
        format_bytes(source_size),
        format_bytes(db_size)
    );

    if let Ok(counts) = db.project_counts() {
        println!();
        println!("Top projects:");
        for (project, count) in counts.iter().take(10) {
            println!("  {:<20} {} conversations", project, count);
        }
        if counts.len() > 10 {
            println!("  ... and {} more", counts.len() - 10);
        }
    }
}

fn print_status_json(db: &Database, stats: &DbStats, source_size: u64) {
    let db_size = std::fs::metadata(&db.path).map(|m| m.len()).unwrap_or(0);

    let projects = db.project_counts().unwrap_or_default();
    let projects_arr: Vec<serde_json::Value> = projects
        .iter()
        .map(|(p, c)| json!({"project": p, "conversations": c}))
        .collect();

    let output = json!({
        "index_path": db.path.display().to_string(),
        "message_count": stats.message_count,
        "conversation_count": stats.conversation_count,
        "project_count": stats.project_count,
        "source_size_bytes": source_size,
        "index_size_bytes": db_size,
        "projects": projects_arr,
    });
    println!("{}", serde_json::to_string(&output).unwrap());
}

/// Format and print search results.
pub fn print_search_results(results: &[SearchResult], json: bool) {
    if json {
        print_search_json(results);
    } else {
        print_search_human(results);
    }
}

fn print_search_human(results: &[SearchResult]) {
    if results.is_empty() {
        println!("No results found.");
        return;
    }

    // Group by conversation
    let mut current_session = String::new();
    for result in results {
        if result.session_id != current_session {
            if !current_session.is_empty() {
                println!();
            }
            current_session = result.session_id.clone();
            let slug_display = result
                .slug
                .as_deref()
                .unwrap_or(&result.session_id[..8.min(result.session_id.len())]);
            println!("--- {} [{}] ---", slug_display, result.project);
        }

        let ts = result
            .timestamp
            .as_deref()
            .map(format_date_short)
            .unwrap_or_default();
        let role_label = match result.role.as_str() {
            "user" => "You",
            "assistant" => "Claude",
            other => other,
        };

        // Highlight search matches (markers from FTS5 snippet)
        let snippet = result
            .snippet
            .replace(">>>", "\x1b[1;33m")
            .replace("<<<", "\x1b[0m");

        println!("  {} [{}]: {}", role_label, ts, snippet);
    }
}

fn print_search_json(results: &[SearchResult]) {
    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            json!({
                "session_id": r.session_id,
                "slug": r.slug,
                "project": r.project,
                "timestamp": r.timestamp,
                "role": r.role,
                "snippet": r.snippet,
            })
        })
        .collect();
    println!("{}", serde_json::to_string(&items).unwrap());
}

/// Format and print conversation list.
pub fn print_conversation_list(entries: &[ConversationEntry], json: bool) {
    if json {
        print_list_json(entries);
    } else {
        print_list_human(entries);
    }
}

fn print_list_human(entries: &[ConversationEntry]) {
    if entries.is_empty() {
        println!("No conversations indexed. Run `nyx index` first.");
        return;
    }

    println!("{:<30} {:<12} {:<20}", "SLUG", "DATE", "PROJECT");
    println!("{}", "-".repeat(65));

    for entry in entries {
        let slug = entry
            .slug
            .as_deref()
            .or(entry.custom_title.as_deref())
            .unwrap_or(&entry.session_id[..8.min(entry.session_id.len())]);
        let date = entry
            .last_timestamp
            .as_deref()
            .map(format_date_short)
            .unwrap_or_else(|| "?".to_string());
        println!("{:<30} {:<12} {:<20}", slug, date, entry.project);
    }

    println!("\n{} conversations total", entries.len());
}

fn print_list_json(entries: &[ConversationEntry]) {
    let items: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            json!({
                "session_id": e.session_id,
                "slug": e.slug,
                "project": e.project,
                "first_timestamp": e.first_timestamp,
                "last_timestamp": e.last_timestamp,
            })
        })
        .collect();
    println!("{}", serde_json::to_string(&items).unwrap());
}

/// Format and print conversation transcript.
pub fn print_transcript(conv: &ConversationEntry, messages: &[TranscriptMessage], json: bool) {
    if json {
        print_transcript_json(conv, messages);
    } else {
        print_transcript_human(conv, messages);
    }
}

fn print_transcript_human(conv: &ConversationEntry, messages: &[TranscriptMessage]) {
    let slug = conv.slug.as_deref().unwrap_or(&conv.session_id);
    println!("=== {} [{}] ===", slug, conv.project);
    if let Some(ref title) = conv.custom_title {
        println!("Title: {}", title);
    }
    println!();

    for msg in messages {
        let ts = msg
            .timestamp
            .as_deref()
            .map(format_date_short)
            .unwrap_or_default();
        let role_label = match msg.role.as_str() {
            "user" => "\x1b[1;34mYou\x1b[0m",
            "assistant" => "\x1b[1;32mClaude\x1b[0m",
            other => other,
        };

        println!("[{}] {}", ts, role_label);

        // Truncate very long messages for readability (char-boundary safe)
        let content = if msg.content.len() > 2000 {
            let truncate_at = msg
                .content
                .char_indices()
                .take_while(|(i, _)| *i < 2000)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            format!(
                "{}... [truncated, {} chars total]",
                &msg.content[..truncate_at],
                msg.content.chars().count()
            )
        } else {
            msg.content.clone()
        };
        println!("{}", content);
        println!();
    }
}

fn print_transcript_json(conv: &ConversationEntry, messages: &[TranscriptMessage]) {
    let msgs: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            json!({
                "timestamp": m.timestamp,
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();
    let output = json!({
        "session_id": conv.session_id,
        "slug": conv.slug,
        "project": conv.project,
        "messages": msgs,
    });
    println!("{}", serde_json::to_string(&output).unwrap());
}

/// Print indexing progress.
pub fn print_index_result(indexed: usize, skipped: usize, json: bool) {
    if json {
        println!(r#"{{"indexed":{},"skipped":{}}}"#, indexed, skipped);
    } else {
        println!("Indexed {} files ({} unchanged, skipped)", indexed, skipped);
    }
}

/// Calculate total size of JSONL files under a directory.
pub fn calculate_source_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Format and print friction results.
pub fn print_friction_results(results: &[FrictionMatch], json: bool) {
    if json {
        print_friction_json(results);
    } else {
        print_friction_human(results);
    }
}

fn print_friction_human(results: &[FrictionMatch]) {
    if results.is_empty() {
        println!("No friction patterns detected.");
        return;
    }

    println!("{} friction pattern(s) detected:\n", results.len());

    for (i, r) in results.iter().enumerate() {
        let ts = r
            .timestamp
            .as_deref()
            .map(format_date_short)
            .unwrap_or_else(|| "?".to_string());
        let slug = r
            .slug
            .as_deref()
            .unwrap_or(&r.session_id[..8.min(r.session_id.len())]);

        let severity_color = match r.severity {
            crate::friction::Severity::High => "\x1b[1;31m",
            crate::friction::Severity::Medium => "\x1b[1;33m",
            crate::friction::Severity::Low => "\x1b[0;33m",
        };

        println!(
            "{}. [{}] {} [{}] {}{}\x1b[0m",
            i + 1,
            ts,
            slug,
            r.project,
            severity_color,
            r.friction_type
        );

        if let Some(ref ctx) = r.context {
            println!("   Claude: {}", ctx);
        }
        println!("   You: {}", r.user_message);
        println!("   Matched: \"{}\" ({})\n", r.matched_phrase, r.severity);
    }
}

fn print_friction_json(results: &[FrictionMatch]) {
    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            json!({
                "session_id": r.session_id,
                "slug": r.slug,
                "project": r.project,
                "timestamp": r.timestamp,
                "user_message": r.user_message,
                "context": r.context,
                "friction_type": r.friction_type.label(),
                "severity": r.severity.label(),
                "matched_phrase": r.matched_phrase,
            })
        })
        .collect();
    println!("{}", serde_json::to_string(&items).unwrap());
}

/// Format and print friction summary.
pub fn print_friction_summary(summary: &FrictionSummary, json: bool) {
    if json {
        print_friction_summary_json(summary);
    } else {
        print_friction_summary_human(summary);
    }
}

fn print_friction_summary_human(summary: &FrictionSummary) {
    if summary.total == 0 {
        println!("No friction patterns detected.");
        return;
    }

    println!("Friction Summary ({} total)\n", summary.total);

    println!("By type:");
    for (ft, count) in &summary.by_type {
        let pct = (*count as f64 / summary.total as f64) * 100.0;
        println!("  {:<14} {:>4} ({:.0}%)", ft.label(), count, pct);
    }

    println!("\nBy severity:");
    for (sev, count) in &summary.by_severity {
        let pct = (*count as f64 / summary.total as f64) * 100.0;
        println!("  {:<14} {:>4} ({:.0}%)", sev.label(), count, pct);
    }

    if !summary.top_phrases.is_empty() {
        println!("\nTop matched phrases:");
        for (phrase, count) in &summary.top_phrases {
            println!("  \"{:<30}\" {:>4}", phrase, count);
        }
    }
}

fn print_friction_summary_json(summary: &FrictionSummary) {
    let by_type: Vec<serde_json::Value> = summary
        .by_type
        .iter()
        .map(|(ft, c)| json!({"type": ft.label(), "count": c}))
        .collect();
    let by_severity: Vec<serde_json::Value> = summary
        .by_severity
        .iter()
        .map(|(s, c)| json!({"severity": s.label(), "count": c}))
        .collect();
    let top_phrases: Vec<serde_json::Value> = summary
        .top_phrases
        .iter()
        .map(|(p, c)| json!({"phrase": p, "count": c}))
        .collect();
    let output = json!({
        "total": summary.total,
        "by_type": by_type,
        "by_severity": by_severity,
        "top_phrases": top_phrases,
    });
    println!("{}", serde_json::to_string(&output).unwrap());
}

/// Print suda export commands for friction results.
pub fn print_friction_suda_export(results: &[FrictionMatch]) {
    if results.is_empty() {
        println!("No friction patterns to export.");
        return;
    }

    println!(
        "# Suda store commands for {} friction pattern(s):",
        results.len()
    );
    println!("# Review and run the ones you want to save:\n");

    for r in results {
        println!("{}\n", crate::friction::format_suda_command(r));
    }
}

/// Format an ISO 8601 timestamp to a short date string.
fn format_date_short(ts: &str) -> String {
    // Input: "2026-03-20T01:26:11.953Z" or "2026-03-20 14:32"
    // Output: "2026-03-20"
    ts.get(..10).unwrap_or(ts).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db_with_data() -> Database {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "s1",
            Some("test-slug"),
            "myproject",
            Some("2026-03-20T01:00:00Z"),
            Some("2026-03-20T02:00:00Z"),
            "/a.jsonl",
            None,
        )
        .unwrap();
        db.insert_message(
            "s1",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "hello world",
            "user",
        )
        .unwrap();
        db.insert_message(
            "s1",
            Some("2026-03-20T01:01:00Z"),
            "assistant",
            "hi there",
            "assistant",
        )
        .unwrap();
        db
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(2_147_483_648), "2.0 GB");
    }

    #[test]
    fn test_format_date_short() {
        assert_eq!(format_date_short("2026-03-20T01:26:11.953Z"), "2026-03-20");
        assert_eq!(format_date_short("2026-03-20 14:32"), "2026-03-20");
        // Short string (less than 10 chars) returns the whole thing
        assert_eq!(format_date_short("short"), "short");
    }

    #[test]
    fn test_print_status_human() {
        let db = make_db_with_data();
        let stats = db.stats().unwrap();
        // Just verify it doesn't panic
        print_status_human(&db, &stats, 12345);
    }

    #[test]
    fn test_print_status_json() {
        let db = make_db_with_data();
        let stats = db.stats().unwrap();
        // Just verify it doesn't panic
        print_status_json(&db, &stats, 12345);
    }

    #[test]
    fn test_print_search_human_empty() {
        print_search_human(&[]);
    }

    #[test]
    fn test_print_search_human_with_results() {
        let results = vec![
            SearchResult {
                session_id: "s1".to_string(),
                slug: Some("test-slug".to_string()),
                project: "proj".to_string(),
                timestamp: Some("2026-03-20T01:00:00Z".to_string()),
                role: "user".to_string(),
                snippet: ">>>hello<<< world".to_string(),
            },
            SearchResult {
                session_id: "s1".to_string(),
                slug: Some("test-slug".to_string()),
                project: "proj".to_string(),
                timestamp: Some("2026-03-20T01:01:00Z".to_string()),
                role: "assistant".to_string(),
                snippet: ">>>hello<<< back".to_string(),
            },
            SearchResult {
                session_id: "s2".to_string(),
                slug: None,
                project: "proj2".to_string(),
                timestamp: None,
                role: "system".to_string(),
                snippet: "system msg".to_string(),
            },
        ];
        print_search_human(&results);
    }

    #[test]
    fn test_print_search_json() {
        let results = vec![SearchResult {
            session_id: "s1".to_string(),
            slug: Some("slug".to_string()),
            project: "proj".to_string(),
            timestamp: Some("2026-03-20T01:00:00Z".to_string()),
            role: "user".to_string(),
            snippet: "snippet text".to_string(),
        }];
        print_search_json(&results);
    }

    #[test]
    fn test_print_list_human_empty() {
        print_list_human(&[]);
    }

    #[test]
    fn test_print_list_human_with_entries() {
        let entries = vec![
            ConversationEntry {
                session_id: "s1".to_string(),
                slug: Some("my-slug".to_string()),
                project: "proj".to_string(),
                first_timestamp: Some("2026-03-20T01:00:00Z".to_string()),
                last_timestamp: Some("2026-03-20T02:00:00Z".to_string()),
                custom_title: None,
            },
            ConversationEntry {
                session_id: "abcdefgh-1234".to_string(),
                slug: None,
                project: "proj2".to_string(),
                first_timestamp: None,
                last_timestamp: None,
                custom_title: Some("Custom Title".to_string()),
            },
        ];
        print_list_human(&entries);
    }

    #[test]
    fn test_print_list_json() {
        let entries = vec![ConversationEntry {
            session_id: "s1".to_string(),
            slug: Some("slug".to_string()),
            project: "proj".to_string(),
            first_timestamp: Some("2026-03-20T01:00:00Z".to_string()),
            last_timestamp: Some("2026-03-20T02:00:00Z".to_string()),
            custom_title: None,
        }];
        print_list_json(&entries);
    }

    #[test]
    fn test_print_transcript_human() {
        let conv = ConversationEntry {
            session_id: "s1".to_string(),
            slug: Some("my-slug".to_string()),
            project: "proj".to_string(),
            first_timestamp: Some("2026-03-20T01:00:00Z".to_string()),
            last_timestamp: Some("2026-03-20T02:00:00Z".to_string()),
            custom_title: Some("My Title".to_string()),
        };
        let messages = vec![
            TranscriptMessage {
                timestamp: Some("2026-03-20T01:00:00Z".to_string()),
                role: "user".to_string(),
                content: "hello".to_string(),
            },
            TranscriptMessage {
                timestamp: Some("2026-03-20T01:01:00Z".to_string()),
                role: "assistant".to_string(),
                content: "hi there".to_string(),
            },
            TranscriptMessage {
                timestamp: None,
                role: "system".to_string(),
                content: "system msg".to_string(),
            },
        ];
        print_transcript_human(&conv, &messages);
    }

    #[test]
    fn test_print_transcript_human_long_message() {
        let conv = ConversationEntry {
            session_id: "s1".to_string(),
            slug: None,
            project: "proj".to_string(),
            first_timestamp: None,
            last_timestamp: None,
            custom_title: None,
        };
        // Create a message longer than 2000 chars to test truncation
        let long_content = "a".repeat(3000);
        let messages = vec![TranscriptMessage {
            timestamp: Some("2026-03-20T01:00:00Z".to_string()),
            role: "user".to_string(),
            content: long_content,
        }];
        print_transcript_human(&conv, &messages);
    }

    #[test]
    fn test_print_transcript_json() {
        let conv = ConversationEntry {
            session_id: "s1".to_string(),
            slug: Some("slug".to_string()),
            project: "proj".to_string(),
            first_timestamp: Some("2026-03-20T01:00:00Z".to_string()),
            last_timestamp: Some("2026-03-20T02:00:00Z".to_string()),
            custom_title: None,
        };
        let messages = vec![TranscriptMessage {
            timestamp: Some("2026-03-20T01:00:00Z".to_string()),
            role: "user".to_string(),
            content: "hello".to_string(),
        }];
        print_transcript_json(&conv, &messages);
    }

    #[test]
    fn test_print_index_result_human() {
        print_index_result(5, 3, false);
    }

    #[test]
    fn test_print_index_result_json() {
        print_index_result(5, 3, true);
    }

    #[test]
    fn test_calculate_source_size_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(calculate_source_size(dir.path()), 0);
    }

    #[test]
    fn test_calculate_source_size_with_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.jsonl");
        std::fs::write(&file, "some data here").unwrap();
        assert!(calculate_source_size(dir.path()) > 0);
    }

    #[test]
    fn test_print_status_dispatches() {
        let db = make_db_with_data();
        let stats = db.stats().unwrap();
        // Test both paths of the dispatcher
        print_status(&db, &stats, 100, false);
        print_status(&db, &stats, 100, true);
    }

    #[test]
    fn test_print_search_results_dispatches() {
        let results = vec![];
        print_search_results(&results, false);
        print_search_results(&results, true);
    }

    #[test]
    fn test_print_conversation_list_dispatches() {
        let entries = vec![];
        print_conversation_list(&entries, false);
        print_conversation_list(&entries, true);
    }

    #[test]
    fn test_print_transcript_dispatches() {
        let conv = ConversationEntry {
            session_id: "s1".to_string(),
            slug: Some("slug".to_string()),
            project: "proj".to_string(),
            first_timestamp: None,
            last_timestamp: None,
            custom_title: None,
        };
        let messages = vec![];
        print_transcript(&conv, &messages, false);
        print_transcript(&conv, &messages, true);
    }
}
