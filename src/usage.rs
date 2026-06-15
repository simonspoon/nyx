use crate::db::Database;
use crate::error::{Error, Result};
use crate::search::parse_duration;

/// How to group usage aggregation rows.
#[derive(Clone, Copy)]
pub enum GroupBy {
    Session,
    Project,
    Model,
    Day,
}

impl GroupBy {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "session" => Ok(GroupBy::Session),
            "project" => Ok(GroupBy::Project),
            "model" => Ok(GroupBy::Model),
            "day" => Ok(GroupBy::Day),
            other => Err(Error::Other(format!(
                "invalid --by value '{other}' (expected session|project|model|day)"
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GroupBy::Session => "session",
            GroupBy::Project => "project",
            GroupBy::Model => "model",
            GroupBy::Day => "day",
        }
    }
}

/// Token usage for one (group, model) pair, summed straight from the DB. Cost
/// is not computed here — the command layer prices each sub-row by its model
/// and folds sub-rows back into display groups.
#[derive(Debug)]
pub struct UsageRow {
    /// The group key as displayed (slug/project/model/day).
    pub group: String,
    /// The model id for this sub-row, used for pricing. `None` when the
    /// assistant message carried no model.
    pub model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_5m: i64,
    pub cache_creation_1h: i64,
}

/// Aggregate token usage from the `message_usage` table, grouped per `by`,
/// filtered by optional `project` and `last` duration. Every `message_usage`
/// row carries usage (one per assistant turn with a usage object).
pub fn aggregate_usage(
    db: &Database,
    by: GroupBy,
    project: Option<&str>,
    last: Option<&str>,
) -> Result<Vec<UsageRow>> {
    let cutoff = match last {
        Some(d) => Some(crate::search::cutoff_timestamp(parse_duration(d)?)),
        None => None,
    };

    let group_expr = match by {
        GroupBy::Session => "COALESCE(c.slug, m.session_id)",
        GroupBy::Project => "c.project",
        GroupBy::Model => "COALESCE(m.model, '(unknown)')",
        GroupBy::Day => "substr(m.timestamp, 1, 10)",
    };

    let mut filter_clauses = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(p) = project {
        param_values.push(Box::new(p.to_string()));
        filter_clauses.push(format!("AND c.project = ?{}", param_values.len()));
    }
    if let Some(ref c) = cutoff {
        param_values.push(Box::new(c.clone()));
        filter_clauses.push(format!("AND m.timestamp >= ?{}", param_values.len()));
    }

    // Group by (display group, model) so every sub-row has a single model and
    // can be priced; the command layer sums sub-rows back into display groups.
    let sql = format!(
        "SELECT {group_expr} AS grp, \
         m.model, \
         SUM(COALESCE(m.input_tokens, 0)), \
         SUM(COALESCE(m.output_tokens, 0)), \
         SUM(COALESCE(m.cache_creation_input_tokens, 0)), \
         SUM(COALESCE(m.cache_read_input_tokens, 0)), \
         SUM(COALESCE(m.cache_creation_5m, 0)), \
         SUM(COALESCE(m.cache_creation_1h, 0)) \
         FROM message_usage m \
         JOIN conversations c ON c.session_id = m.session_id \
         WHERE 1=1 \
         {filters} \
         GROUP BY grp, m.model \
         ORDER BY SUM(COALESCE(m.input_tokens, 0)) + SUM(COALESCE(m.output_tokens, 0)) DESC",
        filters = filter_clauses.join(" ")
    );

    let mut stmt = db.conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(&*param_refs, |row| {
        Ok(UsageRow {
            group: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            model: row.get(1)?,
            input_tokens: row.get(2)?,
            output_tokens: row.get(3)?,
            cache_creation_tokens: row.get(4)?,
            cache_read_tokens: row.get(5)?,
            cache_creation_5m: row.get(6)?,
            cache_creation_1h: row.get(7)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}
