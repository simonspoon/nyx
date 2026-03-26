use rusqlite::params;

use crate::db::Database;
use crate::error::Result;
use crate::search::parse_duration;

/// A detected friction pattern.
#[derive(Debug)]
pub struct FrictionMatch {
    pub session_id: String,
    pub slug: Option<String>,
    pub project: String,
    pub timestamp: Option<String>,
    pub user_message: String,
    pub context: Option<String>,
    pub friction_type: FrictionType,
    pub severity: Severity,
    pub matched_phrase: String,
}

/// Category of friction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrictionType {
    Correction,
    Redirection,
    Frustration,
}

impl FrictionType {
    pub fn label(&self) -> &'static str {
        match self {
            FrictionType::Correction => "correction",
            FrictionType::Redirection => "redirection",
            FrictionType::Frustration => "frustration",
        }
    }
}

impl std::fmt::Display for FrictionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Severity of the friction signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A friction pattern definition.
struct Pattern {
    /// Lowercase phrase to match (checked against lowercased message).
    phrase: &'static str,
    friction_type: FrictionType,
    severity: Severity,
    /// If true, the phrase must appear at the start of the message (after trimming).
    starts_with: bool,
}

/// All friction patterns to detect.
fn patterns() -> Vec<Pattern> {
    vec![
        // -- Corrections (high confidence) --
        Pattern {
            phrase: "no,",
            friction_type: FrictionType::Correction,
            severity: Severity::Medium,
            starts_with: true,
        },
        Pattern {
            phrase: "no.",
            friction_type: FrictionType::Correction,
            severity: Severity::Medium,
            starts_with: true,
        },
        Pattern {
            phrase: "no!",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: true,
        },
        Pattern {
            phrase: "not that",
            friction_type: FrictionType::Correction,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "wrong",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "that's not what i asked",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "that's not what i meant",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "i said",
            friction_type: FrictionType::Correction,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "don't do that",
            friction_type: FrictionType::Correction,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "stop",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: true,
        },
        Pattern {
            phrase: "don't",
            friction_type: FrictionType::Correction,
            severity: Severity::Low,
            starts_with: true,
        },
        Pattern {
            phrase: "do not",
            friction_type: FrictionType::Correction,
            severity: Severity::Low,
            starts_with: true,
        },
        Pattern {
            phrase: "that is wrong",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "that's wrong",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "not correct",
            friction_type: FrictionType::Correction,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "that's incorrect",
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            starts_with: false,
        },
        // -- Redirection --
        Pattern {
            phrase: "instead,",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "instead.",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "simpler",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "just do",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "try again",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "let me clarify",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "what i meant was",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "to be clear",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "i meant",
            friction_type: FrictionType::Redirection,
            severity: Severity::Low,
            starts_with: false,
        },
        Pattern {
            phrase: "that's not the approach",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "over-engineer",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "overengineer",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "too complex",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "keep it simple",
            friction_type: FrictionType::Redirection,
            severity: Severity::Medium,
            starts_with: false,
        },
        // -- Frustration --
        Pattern {
            phrase: "again?",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "already told you",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "i already said",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "why did you",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "why are you",
            friction_type: FrictionType::Frustration,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "i just said",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "you keep",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "you already",
            friction_type: FrictionType::Frustration,
            severity: Severity::Medium,
            starts_with: false,
        },
        Pattern {
            phrase: "how many times",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "still not",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
        Pattern {
            phrase: "that's still wrong",
            friction_type: FrictionType::Frustration,
            severity: Severity::High,
            starts_with: false,
        },
    ]
}

/// Scan the database for friction patterns in user messages.
pub fn detect_friction(
    db: &Database,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<FrictionMatch>> {
    let cutoff = match since {
        Some(d) => {
            let secs = parse_duration(d)?;
            Some(cutoff_timestamp(secs))
        }
        None => None,
    };

    // Query user messages, optionally filtered by time
    let sql = if cutoff.is_some() {
        "SELECT m.id, m.session_id, c.slug, c.project, m.timestamp, m.content \
         FROM messages m \
         JOIN conversations c ON c.session_id = m.session_id \
         WHERE m.role = 'user' AND m.record_type = 'user' AND m.timestamp >= ?1 \
         ORDER BY m.timestamp DESC"
    } else {
        "SELECT m.id, m.session_id, c.slug, c.project, m.timestamp, m.content \
         FROM messages m \
         JOIN conversations c ON c.session_id = m.session_id \
         WHERE m.role = 'user' AND m.record_type = 'user' \
         ORDER BY m.timestamp DESC"
    };

    let mut stmt = db.conn.prepare(sql)?;

    type MessageRow = (i64, String, Option<String>, String, Option<String>, String);
    let rows: Vec<MessageRow> = if let Some(
        ref c,
    ) = cutoff
    {
        stmt.query_map(params![c], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let pats = patterns();
    let max_results = limit.unwrap_or(usize::MAX);
    let mut results = Vec::new();

    for (_msg_id, session_id, slug, project, timestamp, content) in &rows {
        if results.len() >= max_results {
            break;
        }

        // Skip very short messages and tool output (likely not real user text)
        if content.len() < 3 {
            continue;
        }

        let lower = content.to_lowercase();
        let trimmed_lower = lower.trim_start();

        // Find the highest-severity matching pattern
        let mut best_match: Option<(&Pattern, usize)> = None;

        for pat in &pats {
            let matched = if pat.starts_with {
                trimmed_lower.starts_with(pat.phrase)
            } else {
                lower.contains(pat.phrase)
            };

            if matched {
                let severity_rank = match pat.severity {
                    Severity::High => 2,
                    Severity::Medium => 1,
                    Severity::Low => 0,
                };
                if best_match
                    .as_ref()
                    .is_none_or(|(_, rank)| severity_rank > *rank)
                {
                    best_match = Some((pat, severity_rank));
                }
            }
        }

        if let Some((pat, _)) = best_match {
            // Get preceding assistant message as context
            let context = get_preceding_assistant_message(db, session_id, timestamp.as_deref());

            // Truncate user message for display
            let display_msg = truncate_message(content, 300);

            results.push(FrictionMatch {
                session_id: session_id.clone(),
                slug: slug.clone(),
                project: project.clone(),
                timestamp: timestamp.clone(),
                user_message: display_msg,
                context,
                friction_type: pat.friction_type,
                severity: pat.severity,
                matched_phrase: pat.phrase.to_string(),
            });
        }
    }

    Ok(results)
}

/// Get the assistant message immediately before a user message in the same session.
fn get_preceding_assistant_message(
    db: &Database,
    session_id: &str,
    timestamp: Option<&str>,
) -> Option<String> {
    let ts = timestamp?;
    let sql = "SELECT content FROM messages \
               WHERE session_id = ?1 AND role = 'assistant' AND record_type = 'assistant' \
               AND timestamp < ?2 \
               ORDER BY timestamp DESC LIMIT 1";
    db.conn
        .query_row(sql, params![session_id, ts], |row| row.get::<_, String>(0))
        .ok()
        .map(|s| truncate_message(&s, 200))
}

/// Truncate a message to max_chars, respecting char boundaries.
fn truncate_message(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let truncate_at = s
        .char_indices()
        .take_while(|(i, _)| *i < max_chars)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    format!("{}...", &s[..truncate_at])
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

/// Friction summary: counts grouped by inferred category.
#[derive(Debug)]
pub struct FrictionSummary {
    pub total: usize,
    pub by_type: Vec<(FrictionType, usize)>,
    pub by_severity: Vec<(Severity, usize)>,
    pub top_phrases: Vec<(String, usize)>,
}

/// Build a summary from friction results.
pub fn summarize(results: &[FrictionMatch]) -> FrictionSummary {
    use std::collections::HashMap;

    let mut type_counts: HashMap<FrictionType, usize> = HashMap::new();
    let mut severity_counts: HashMap<Severity, usize> = HashMap::new();
    let mut phrase_counts: HashMap<String, usize> = HashMap::new();

    for r in results {
        *type_counts.entry(r.friction_type).or_default() += 1;
        *severity_counts.entry(r.severity).or_default() += 1;
        *phrase_counts.entry(r.matched_phrase.clone()).or_default() += 1;
    }

    let mut by_type: Vec<_> = type_counts.into_iter().collect();
    by_type.sort_by(|a, b| b.1.cmp(&a.1));

    let mut by_severity: Vec<_> = severity_counts.into_iter().collect();
    by_severity.sort_by(|a, b| b.1.cmp(&a.1));

    let mut top_phrases: Vec<_> = phrase_counts.into_iter().collect();
    top_phrases.sort_by(|a, b| b.1.cmp(&a.1));
    top_phrases.truncate(10);

    FrictionSummary {
        total: results.len(),
        by_type,
        by_severity,
        top_phrases,
    }
}

/// Format a friction match as a suda store command.
pub fn format_suda_command(m: &FrictionMatch) -> String {
    let name = format!(
        "friction-{}-{}",
        m.friction_type.label(),
        m.timestamp
            .as_deref()
            .unwrap_or("unknown")
            .get(..10)
            .unwrap_or("unknown")
    );

    let desc = format!(
        "Friction detected: {} ({} severity) - matched '{}'",
        m.friction_type, m.severity, m.matched_phrase
    );

    // Build content with context
    let mut content = format!("User said: {}", m.user_message);
    if let Some(ref ctx) = m.context {
        content = format!("Claude said: {}\n\n{}", ctx, content);
    }
    if let Some(ref ts) = m.timestamp {
        content = format!("{}\n\nTimestamp: {}", content, ts);
    }
    content = format!("{}\nProject: {}", content, m.project);

    // Escape single quotes in content for shell safety
    let content_escaped = content.replace('\'', "'\\''");
    let desc_escaped = desc.replace('\'', "'\\''");
    let name_escaped = name.replace('\'', "'\\''");

    format!(
        "suda store --type feedback --name '{}' --description '{}' '{}'",
        name_escaped, desc_escaped, content_escaped
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_test_db() -> Database {
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

        // Assistant message
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:00:00Z"),
            "assistant",
            "Here is how to implement it with a complex framework.",
            "assistant",
        )
        .unwrap();

        // User correction
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:01:00Z"),
            "user",
            "No, that's not what I asked. I want something simpler.",
            "user",
        )
        .unwrap();

        // Another assistant message
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:02:00Z"),
            "assistant",
            "Let me try a different approach.",
            "assistant",
        )
        .unwrap();

        // User frustration
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:03:00Z"),
            "user",
            "Why did you do it that way again? I already told you to keep it simple.",
            "user",
        )
        .unwrap();

        // Normal message (no friction)
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:04:00Z"),
            "user",
            "Thanks, that looks good now.",
            "user",
        )
        .unwrap();

        db
    }

    #[test]
    fn test_detect_friction_finds_patterns() {
        let db = setup_test_db();
        let results = detect_friction(&db, None, None).unwrap();
        assert!(results.len() >= 2, "Expected at least 2 friction matches, got {}", results.len());
    }

    #[test]
    fn test_detect_friction_with_limit() {
        let db = setup_test_db();
        let results = detect_friction(&db, None, Some(1)).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_detect_friction_empty_db() {
        let db = Database::open_memory().unwrap();
        let results = detect_friction(&db, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_detect_friction_no_matches() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "sess-1", None, "proj", None, None, "/p.jsonl", None,
        ).unwrap();
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "Please help me build a REST API.",
            "user",
        )
        .unwrap();
        let results = detect_friction(&db, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_friction_types_detected() {
        let db = setup_test_db();
        let results = detect_friction(&db, None, None).unwrap();

        let has_correction = results.iter().any(|r| r.friction_type == FrictionType::Correction);
        let has_frustration = results.iter().any(|r| r.friction_type == FrictionType::Frustration);
        assert!(has_correction, "Should detect correction");
        assert!(has_frustration, "Should detect frustration");
    }

    #[test]
    fn test_summarize() {
        let db = setup_test_db();
        let results = detect_friction(&db, None, None).unwrap();
        let summary = summarize(&results);
        assert_eq!(summary.total, results.len());
        assert!(!summary.by_type.is_empty());
        assert!(!summary.by_severity.is_empty());
        assert!(!summary.top_phrases.is_empty());
    }

    #[test]
    fn test_format_suda_command() {
        let m = FrictionMatch {
            session_id: "sess-1".to_string(),
            slug: Some("test-slug".to_string()),
            project: "myproject".to_string(),
            timestamp: Some("2026-03-20T01:01:00Z".to_string()),
            user_message: "No, that's wrong.".to_string(),
            context: Some("Here is my approach.".to_string()),
            friction_type: FrictionType::Correction,
            severity: Severity::High,
            matched_phrase: "wrong".to_string(),
        };
        let cmd = format_suda_command(&m);
        assert!(cmd.starts_with("suda store --type feedback"));
        assert!(cmd.contains("friction-correction-2026-03-20"));
        assert!(cmd.contains("No, that"), "Should contain user message text");
        assert!(cmd.contains("wrong."), "Should contain user message text");
    }

    #[test]
    fn test_truncate_message() {
        assert_eq!(truncate_message("short", 100), "short");
        let long = "a".repeat(500);
        let truncated = truncate_message(&long, 100);
        assert!(truncated.len() < 110);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_friction_type_display() {
        assert_eq!(FrictionType::Correction.to_string(), "correction");
        assert_eq!(FrictionType::Redirection.to_string(), "redirection");
        assert_eq!(FrictionType::Frustration.to_string(), "frustration");
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Low.to_string(), "low");
        assert_eq!(Severity::Medium.to_string(), "medium");
        assert_eq!(Severity::High.to_string(), "high");
    }

    #[test]
    fn test_starts_with_patterns() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation("s1", None, "p", None, None, "/p.jsonl", None).unwrap();

        // "No," at start should match
        db.insert_message("s1", Some("2026-03-20T01:00:00Z"), "user", "No, do it differently", "user").unwrap();
        let results = detect_friction(&db, None, None).unwrap();
        assert!(!results.is_empty(), "Should match 'No,' at start");

        // "no" buried in a sentence should NOT match starts_with patterns
        let db2 = Database::open_memory().unwrap();
        db2.upsert_conversation("s2", None, "p", None, None, "/p.jsonl", None).unwrap();
        db2.insert_message("s2", Some("2026-03-20T01:00:00Z"), "user", "I have no idea how to do this", "user").unwrap();
        let results2 = detect_friction(&db2, None, None).unwrap();
        // "no" appears mid-sentence, but starts_with patterns won't fire.
        // However, "no idea" doesn't match any non-starts_with pattern either.
        assert!(results2.is_empty(), "Should not match 'no' in middle of sentence");
    }

    #[test]
    fn test_since_filter() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "s1", None, "p",
            Some("2020-01-01T00:00:00Z"),
            Some("2020-01-01T01:00:00Z"),
            "/p.jsonl", None,
        ).unwrap();
        db.insert_message(
            "s1", Some("2020-01-01T00:00:00Z"),
            "user", "No, that's wrong", "user",
        ).unwrap();

        // With a 1-day window, the 2020 message should be excluded
        let results = detect_friction(&db, Some("1d"), None).unwrap();
        assert!(results.is_empty(), "Old message should be filtered out by --since");
    }
}
