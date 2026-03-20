use std::path::Path;

use crate::db::{Database, DbStats};
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
    let projects_json: Vec<String> = projects
        .iter()
        .map(|(p, c)| format!(r#"{{"project":"{}","conversations":{}}}"#, p, c))
        .collect();

    println!(
        r#"{{"index_path":"{}","message_count":{},"conversation_count":{},"project_count":{},"source_size_bytes":{},"index_size_bytes":{},"projects":[{}]}}"#,
        db.path.display(),
        stats.message_count,
        stats.conversation_count,
        stats.project_count,
        source_size,
        db_size,
        projects_json.join(",")
    );
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
    let items: Vec<String> = results
        .iter()
        .map(|r| {
            format!(
                r#"{{"session_id":"{}","slug":{},"project":"{}","timestamp":{},"role":"{}","snippet":"{}"}}"#,
                escape_json(&r.session_id),
                r.slug.as_ref().map_or("null".to_string(), |s| format!(r#""{}""#, escape_json(s))),
                escape_json(&r.project),
                r.timestamp.as_ref().map_or("null".to_string(), |t| format!(r#""{}""#, escape_json(t))),
                escape_json(&r.role),
                escape_json(&r.snippet),
            )
        })
        .collect();
    println!("[{}]", items.join(","));
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
    let items: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                r#"{{"session_id":"{}","slug":{},"project":"{}","first_timestamp":{},"last_timestamp":{}}}"#,
                escape_json(&e.session_id),
                e.slug.as_ref().map_or("null".to_string(), |s| format!(r#""{}""#, escape_json(s))),
                escape_json(&e.project),
                e.first_timestamp.as_ref().map_or("null".to_string(), |t| format!(r#""{}""#, escape_json(t))),
                e.last_timestamp.as_ref().map_or("null".to_string(), |t| format!(r#""{}""#, escape_json(t))),
            )
        })
        .collect();
    println!("[{}]", items.join(","));
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
    let msgs: Vec<String> = messages
        .iter()
        .map(|m| {
            format!(
                r#"{{"timestamp":{},"role":"{}","content":"{}"}}"#,
                m.timestamp
                    .as_ref()
                    .map_or("null".to_string(), |t| format!(r#""{}""#, escape_json(t))),
                escape_json(&m.role),
                escape_json(&m.content),
            )
        })
        .collect();
    println!(
        r#"{{"session_id":"{}","slug":{},"project":"{}","messages":[{}]}}"#,
        escape_json(&conv.session_id),
        conv.slug
            .as_ref()
            .map_or("null".to_string(), |s| format!(r#""{}""#, escape_json(s))),
        escape_json(&conv.project),
        msgs.join(","),
    );
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

/// Format an ISO 8601 timestamp to a short date string.
fn format_date_short(ts: &str) -> String {
    // Input: "2026-03-20T01:26:11.953Z" or "2026-03-20 14:32"
    // Output: "2026-03-20"
    ts.get(..10).unwrap_or(ts).to_string()
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
