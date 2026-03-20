use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

#[allow(dead_code)]
const SCHEMA_VERSION: i32 = 1;

pub struct Database {
    pub conn: Connection,
    pub path: PathBuf,
}

impl Database {
    /// Open or create the database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Database {
            conn,
            path: path.to_path_buf(),
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Database {
            conn,
            path: PathBuf::from(":memory:"),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conversations (
                session_id TEXT PRIMARY KEY,
                slug TEXT,
                project TEXT,
                first_timestamp TEXT,
                last_timestamp TEXT,
                source_path TEXT NOT NULL,
                custom_title TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                timestamp TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                record_type TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES conversations(session_id)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session
                ON messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_messages_role
                ON messages(role);
            CREATE INDEX IF NOT EXISTS idx_conversations_project
                ON conversations(project);
            CREATE INDEX IF NOT EXISTS idx_conversations_slug
                ON conversations(slug);

            CREATE TABLE IF NOT EXISTS source_files (
                path TEXT PRIMARY KEY,
                mtime_secs INTEGER NOT NULL,
                session_id TEXT NOT NULL
            );
            ",
        )?;

        // Create FTS5 virtual table if it doesn't exist
        self.conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content,
                content='messages',
                content_rowid='id',
                tokenize='unicode61'
            );

            -- Triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content)
                    VALUES('delete', old.id, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content)
                    VALUES('delete', old.id, old.content);
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;
            ",
        )?;

        // Set schema version if not set
        let count: i32 = self
            .conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))?;
        if count == 0 {
            self.conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }

        Ok(())
    }

    /// Check if a source file needs re-indexing based on mtime.
    pub fn needs_reindex(&self, path: &Path, current_mtime: SystemTime) -> Result<bool> {
        let path_str = path.to_string_lossy().to_string();
        let current_secs = current_mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let result: std::result::Result<i64, _> = self.conn.query_row(
            "SELECT mtime_secs FROM source_files WHERE path = ?1",
            params![path_str],
            |row| row.get(0),
        );

        match result {
            Ok(stored_secs) => Ok(current_secs > stored_secs),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
            Err(e) => Err(Error::Db(e)),
        }
    }

    /// Record that a source file has been indexed with its current mtime.
    pub fn record_source_file(
        &self,
        path: &Path,
        mtime: SystemTime,
        session_id: &str,
    ) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();
        let mtime_secs = mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO source_files (path, mtime_secs, session_id) VALUES (?1, ?2, ?3)",
            params![path_str, mtime_secs, session_id],
        )?;
        Ok(())
    }

    /// Insert or update a conversation record.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_conversation(
        &self,
        session_id: &str,
        slug: Option<&str>,
        project: &str,
        first_ts: Option<&str>,
        last_ts: Option<&str>,
        source_path: &str,
        custom_title: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO conversations (session_id, slug, project, first_timestamp, last_timestamp, source_path, custom_title)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(session_id) DO UPDATE SET
                slug = COALESCE(?2, conversations.slug),
                first_timestamp = MIN(COALESCE(?4, conversations.first_timestamp), COALESCE(conversations.first_timestamp, ?4)),
                last_timestamp = MAX(COALESCE(?5, conversations.last_timestamp), COALESCE(conversations.last_timestamp, ?5)),
                custom_title = COALESCE(?7, conversations.custom_title)",
            params![session_id, slug, project, first_ts, last_ts, source_path, custom_title],
        )?;
        Ok(())
    }

    /// Insert a message record.
    pub fn insert_message(
        &self,
        session_id: &str,
        timestamp: Option<&str>,
        role: &str,
        content: &str,
        record_type: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (session_id, timestamp, role, content, record_type)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, timestamp, role, content, record_type],
        )?;
        Ok(())
    }

    /// Delete all data for a session (for re-indexing).
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        self.conn.execute(
            "DELETE FROM conversations WHERE session_id = ?1",
            params![session_id],
        )?;
        self.conn.execute(
            "DELETE FROM source_files WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Get the last update time of the database file.
    pub fn last_updated(&self) -> Option<String> {
        if self.path.as_os_str() == ":memory:" {
            return None;
        }
        std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(format_system_time)
    }

    /// Get summary statistics.
    pub fn stats(&self) -> Result<DbStats> {
        let message_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
        let conversation_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))?;
        let project_count: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT project) FROM conversations",
            [],
            |row| row.get(0),
        )?;
        let date_range = self.conn.query_row(
            "SELECT MIN(first_timestamp), MAX(last_timestamp) FROM conversations",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )?;

        Ok(DbStats {
            message_count,
            conversation_count,
            project_count,
            first_date: date_range.0,
            last_date: date_range.1,
        })
    }

    /// Get per-project conversation counts, ordered by count descending.
    pub fn project_counts(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT project, COUNT(*) as cnt FROM conversations GROUP BY project ORDER BY cnt DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

pub struct DbStats {
    pub message_count: i64,
    pub conversation_count: i64,
    pub project_count: i64,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
}

pub fn default_db_path() -> PathBuf {
    dirs_home().join(".nyx").join("index.db")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn format_system_time(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple UTC datetime formatting without chrono
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;

    // Days since epoch to Y-M-D (simplified Gregorian)
    let (year, month, day) = epoch_days_to_date(days as i64);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hours, minutes
    )
}

fn epoch_days_to_date(mut days: i64) -> (i64, i64, i64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_database() {
        let db = Database::open_memory().unwrap();
        let stats = db.stats().unwrap();
        assert_eq!(stats.message_count, 0);
        assert_eq!(stats.conversation_count, 0);
    }

    #[test]
    fn test_insert_and_query() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "sess-1",
            Some("test-slug"),
            "my-project",
            Some("2026-03-20T01:00:00Z"),
            Some("2026-03-20T02:00:00Z"),
            "/path/to/file.jsonl",
            None,
        )
        .unwrap();
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "hello world",
            "user",
        )
        .unwrap();

        let stats = db.stats().unwrap();
        assert_eq!(stats.conversation_count, 1);
        assert_eq!(stats.message_count, 1);
    }

    #[test]
    fn test_fts5_search() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation(
            "sess-1",
            Some("test-slug"),
            "my-project",
            Some("2026-03-20T01:00:00Z"),
            Some("2026-03-20T02:00:00Z"),
            "/path/to/file.jsonl",
            None,
        )
        .unwrap();
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:00:00Z"),
            "user",
            "hello world rust programming",
            "user",
        )
        .unwrap();
        db.insert_message(
            "sess-1",
            Some("2026-03-20T01:05:00Z"),
            "assistant",
            "I can help with rust",
            "assistant",
        )
        .unwrap();

        // FTS5 search
        let mut stmt = db.conn.prepare(
            "SELECT m.content FROM messages m JOIN messages_fts f ON m.id = f.rowid WHERE messages_fts MATCH ?1"
        ).unwrap();
        let results: Vec<String> = stmt
            .query_map(params!["rust"], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_incremental_indexing() {
        let db = Database::open_memory().unwrap();
        let path = Path::new("/test/file.jsonl");
        let mtime = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000);

        // First time: needs indexing
        assert!(db.needs_reindex(path, mtime).unwrap());

        // Record it
        db.record_source_file(path, mtime, "sess-1").unwrap();

        // Same mtime: no reindex needed
        assert!(!db.needs_reindex(path, mtime).unwrap());

        // Newer mtime: needs reindex
        let newer = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2000);
        assert!(db.needs_reindex(path, newer).unwrap());
    }

    #[test]
    fn test_epoch_days_to_date() {
        // 2026-03-20 is day 20532 from epoch (1970-01-01)
        let (y, m, d) = epoch_days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));

        // Known date: 2024-01-01 = day 19723
        let (y, m, d) = epoch_days_to_date(19723);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn test_project_counts() {
        let db = Database::open_memory().unwrap();
        db.upsert_conversation("s1", None, "project-a", None, None, "/a.jsonl", None)
            .unwrap();
        db.upsert_conversation("s2", None, "project-a", None, None, "/b.jsonl", None)
            .unwrap();
        db.upsert_conversation("s3", None, "project-b", None, None, "/c.jsonl", None)
            .unwrap();

        let counts = db.project_counts().unwrap();
        assert_eq!(counts[0], ("project-a".to_string(), 2));
        assert_eq!(counts[1], ("project-b".to_string(), 1));
    }
}
