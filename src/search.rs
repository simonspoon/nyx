use rusqlite::params;

use crate::db::Database;
use crate::error::{Error, Result};

/// A search result with context.
#[derive(Debug)]
pub struct SearchResult {
    pub session_id: String,
    pub slug: Option<String>,
    pub project: String,
    pub timestamp: Option<String>,
    pub role: String,
    pub snippet: String,
}

/// A conversation listing entry.
#[derive(Debug)]
pub struct ConversationEntry {
    pub session_id: String,
    pub slug: Option<String>,
    pub project: String,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
    pub custom_title: Option<String>,
}

/// A message in a conversation transcript.
#[derive(Debug)]
pub struct TranscriptMessage {
    pub timestamp: Option<String>,
    pub role: String,
    pub content: String,
}

/// Parse a duration string like "7d", "24h", "30d" into seconds.
pub fn parse_duration(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::InvalidDuration(s.to_string()));
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str
        .parse()
        .map_err(|_| Error::InvalidDuration(s.to_string()))?;

    match unit {
        "d" => Ok(num * 86400),
        "h" => Ok(num * 3600),
        "m" => Ok(num * 60),
        _ => Err(Error::InvalidDuration(s.to_string())),
    }
}

/// Compute an ISO 8601 timestamp for "now minus duration_secs".
fn cutoff_timestamp(duration_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now.saturating_sub(duration_secs);
    crate::db::format_system_time(
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(cutoff),
    )
}

/// Full-text search across all indexed conversations.
pub fn search(
    db: &Database,
    query: &str,
    project: Option<&str>,
    last_duration: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let cutoff = match last_duration {
        Some(d) => {
            let secs = parse_duration(d)?;
            Some(cutoff_timestamp(secs))
        }
        None => None,
    };

    // Build query based on filters
    let sql = if project.is_some() || cutoff.is_some() {
        format!(
            "SELECT m.session_id, c.slug, c.project, m.timestamp, m.role, \
             snippet(messages_fts, 0, '>>>', '<<<', '...', 64) as snippet \
             FROM messages_fts f \
             JOIN messages m ON m.id = f.rowid \
             JOIN conversations c ON c.session_id = m.session_id \
             WHERE messages_fts MATCH ?1 \
             {} {} \
             ORDER BY m.timestamp DESC \
             LIMIT 100",
            if project.is_some() {
                "AND c.project = ?2"
            } else {
                ""
            },
            if cutoff.is_some() {
                if project.is_some() {
                    "AND m.timestamp >= ?3"
                } else {
                    "AND m.timestamp >= ?2"
                }
            } else {
                ""
            }
        )
    } else {
        "SELECT m.session_id, c.slug, c.project, m.timestamp, m.role, \
         snippet(messages_fts, 0, '>>>', '<<<', '...', 64) as snippet \
         FROM messages_fts f \
         JOIN messages m ON m.id = f.rowid \
         JOIN conversations c ON c.session_id = m.session_id \
         WHERE messages_fts MATCH ?1 \
         ORDER BY m.timestamp DESC \
         LIMIT 100"
            .to_string()
    };

    let mut stmt = db.conn.prepare(&sql)?;

    let rows = match (project, &cutoff) {
        (Some(p), Some(c)) => stmt.query_map(params![query, p, c], map_search_result)?,
        (Some(p), None) => stmt.query_map(params![query, p], map_search_result)?,
        (None, Some(c)) => stmt.query_map(params![query, c], map_search_result)?,
        (None, None) => stmt.query_map(params![query], map_search_result)?,
    };

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

fn map_search_result(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        session_id: row.get(0)?,
        slug: row.get(1)?,
        project: row.get(2)?,
        timestamp: row.get(3)?,
        role: row.get(4)?,
        snippet: row.get(5)?,
    })
}

/// List all indexed conversations.
pub fn list_conversations(db: &Database) -> Result<Vec<ConversationEntry>> {
    let mut stmt = db.conn.prepare(
        "SELECT session_id, slug, project, first_timestamp, last_timestamp, custom_title \
         FROM conversations \
         ORDER BY last_timestamp DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(ConversationEntry {
            session_id: row.get(0)?,
            slug: row.get(1)?,
            project: row.get(2)?,
            first_timestamp: row.get(3)?,
            last_timestamp: row.get(4)?,
            custom_title: row.get(5)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Get the transcript of a conversation by slug or session_id prefix.
pub fn show_conversation(
    db: &Database,
    slug: &str,
) -> Result<(ConversationEntry, Vec<TranscriptMessage>)> {
    // Try slug first, then session_id prefix
    let conv = db
        .conn
        .query_row(
            "SELECT session_id, slug, project, first_timestamp, last_timestamp, custom_title \
         FROM conversations WHERE slug = ?1 OR session_id LIKE ?2 \
         ORDER BY CASE WHEN slug = ?1 THEN 0 ELSE 1 END \
         LIMIT 1",
            params![slug, format!("{}%", slug)],
            |row| {
                Ok(ConversationEntry {
                    session_id: row.get(0)?,
                    slug: row.get(1)?,
                    project: row.get(2)?,
                    first_timestamp: row.get(3)?,
                    last_timestamp: row.get(4)?,
                    custom_title: row.get(5)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Error::ConversationNotFound(slug.to_string()),
            other => Error::Db(other),
        })?;

    // Get messages (only user and assistant text, skip tool_use/thinking/progress)
    let mut stmt = db.conn.prepare(
        "SELECT timestamp, role, content FROM messages \
         WHERE session_id = ?1 AND role IN ('user', 'assistant') AND record_type IN ('user', 'assistant') \
         ORDER BY timestamp ASC",
    )?;

    let rows = stmt.query_map(params![conv.session_id], |row| {
        Ok(TranscriptMessage {
            timestamp: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
        })
    })?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row?);
    }
    Ok((conv, messages))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("7d").unwrap(), 7 * 86400);
        assert_eq!(parse_duration("24h").unwrap(), 24 * 3600);
        assert_eq!(parse_duration("30m").unwrap(), 30 * 60);
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("7x").is_err());
    }

    #[test]
    fn test_search_with_indexed_data() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "sess-1",
            Some("test-slug"),
            "myproject",
            Some("2026-03-20T01:00:00Z"),
            Some("2026-03-20T02:00:00Z"),
            "/path.jsonl",
            None,
        )
        .unwrap();
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "How do I implement a binary search tree in Rust?",
            "user",
        )
        .unwrap();
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:01:00Z"),
            "assistant",
            "Here is how to implement a binary search tree...",
            "assistant",
        )
        .unwrap();

        let results = search(&db, "binary search", None, None).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].project, "myproject");
    }

    #[test]
    fn test_search_with_project_filter() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "s1",
            Some("slug-1"),
            "project-a",
            Some("2026-03-20T01:00:00Z"),
            None,
            "/a.jsonl",
            None,
        )
        .unwrap();
        db.upsert_conversation(
            "s2",
            Some("slug-2"),
            "project-b",
            Some("2026-03-20T01:00:00Z"),
            None,
            "/b.jsonl",
            None,
        )
        .unwrap();
        db.insert_message(
            "s1",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "rust programming",
            "user",
        )
        .unwrap();
        db.insert_message(
            "s2",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "rust programming",
            "user",
        )
        .unwrap();

        let results = search(&db, "rust", Some("project-a"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "project-a");
    }

    #[test]
    fn test_list_conversations() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "s1",
            Some("slug-1"),
            "proj",
            Some("2026-03-20T01:00:00Z"),
            Some("2026-03-20T02:00:00Z"),
            "/a.jsonl",
            None,
        )
        .unwrap();

        let convos = list_conversations(&db).unwrap();
        assert_eq!(convos.len(), 1);
        assert_eq!(convos[0].slug.as_deref(), Some("slug-1"));
    }

    #[test]
    fn test_show_conversation() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "s1",
            Some("my-slug"),
            "proj",
            Some("2026-03-20T01:00:00Z"),
            Some("2026-03-20T02:00:00Z"),
            "/a.jsonl",
            None,
        )
        .unwrap();
        db.insert_message("s1", Some("2026-03-20T01:00:00Z"), "user", "hello", "user")
            .unwrap();
        db.insert_message(
            "s1",
            Some("2026-03-20T01:01:00Z"),
            "assistant",
            "hi there",
            "assistant",
        )
        .unwrap();

        let (conv, msgs) = show_conversation(&db, "my-slug").unwrap();
        assert_eq!(conv.session_id, "s1");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn test_show_conversation_not_found() {
        let db = Database::open_memory().unwrap();
        let result = show_conversation(&db, "nonexistent");
        assert!(result.is_err());
    }
}
