mod cli;
mod db;
mod error;
mod friction;
mod indexer;
mod models;
mod output;
mod pricing;
mod search;
mod usage;

use clap::Parser;

use cli::{Cli, Command};
use db::Database;
use error::Result;

fn main() {
    // Restore default SIGPIPE behavior so the process terminates quietly
    // when a downstream reader (e.g. `head`) closes the pipe early, instead
    // of panicking on a failed write to stdout.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Status => cmd_status(cli.json),
        Command::Index { rebuild } => cmd_index(*rebuild, cli.json),
        Command::Search {
            query,
            project,
            last,
        } => cmd_search(query, project.as_deref(), last.as_deref(), cli.json),
        Command::List => cmd_list(cli.json),
        Command::Show { slug } => cmd_show(slug, cli.json),
        Command::Friction {
            since,
            limit,
            summary,
        } => cmd_friction(since.as_deref(), *limit, *summary, cli.json),
        Command::Usage { last, project, by } => {
            cmd_usage(last.as_deref(), project.as_deref(), by, cli.json)
        }
    }
}

fn cmd_status(json: bool) -> Result<()> {
    let db_path = db::default_db_path();
    if !db_path.exists() {
        return Err(error::Error::NoIndex(db_path));
    }
    let db = Database::open(&db_path)?;
    let stats = db.stats()?;
    let source_size = output::calculate_source_size(&indexer::default_projects_dir());
    output::print_status(&db, &stats, source_size, json);
    Ok(())
}

fn cmd_index(rebuild: bool, json: bool) -> Result<()> {
    let db_path = db::default_db_path();
    if rebuild {
        db::drop_all(&db_path)?;
    }
    let mut db = Database::open(&db_path)?;
    let projects_dir = indexer::default_projects_dir();

    if !projects_dir.exists() {
        return Err(error::Error::Other(format!(
            "Claude Code projects directory not found: {}",
            projects_dir.display()
        )));
    }

    let (indexed, skipped) = indexer::index_all(&mut db, &projects_dir)?;
    output::print_index_result(indexed, skipped, json);
    Ok(())
}

fn cmd_search(query: &str, project: Option<&str>, last: Option<&str>, json: bool) -> Result<()> {
    let db_path = db::default_db_path();
    if !db_path.exists() {
        return Err(error::Error::NoIndex(db_path));
    }
    let db = Database::open(&db_path)?;
    let results = search::search(&db, query, project, last)?;
    output::print_search_results(&results, json);
    Ok(())
}

fn cmd_list(json: bool) -> Result<()> {
    let db_path = db::default_db_path();
    if !db_path.exists() {
        return Err(error::Error::NoIndex(db_path));
    }
    let db = Database::open(&db_path)?;
    let entries = search::list_conversations(&db)?;
    output::print_conversation_list(&entries, json);
    Ok(())
}

fn cmd_show(slug: &str, json: bool) -> Result<()> {
    let db_path = db::default_db_path();
    if !db_path.exists() {
        return Err(error::Error::NoIndex(db_path));
    }
    let db = Database::open(&db_path)?;
    let (conv, messages) = search::show_conversation(&db, slug)?;
    output::print_transcript(&conv, &messages, json);
    Ok(())
}

fn cmd_usage(last: Option<&str>, project: Option<&str>, by: &str, json: bool) -> Result<()> {
    let group_by = usage::GroupBy::parse(by)?;

    let db_path = db::default_db_path();
    if !db_path.exists() {
        return Err(error::Error::NoIndex(db_path));
    }
    let db = Database::open(&db_path)?;
    let rows = usage::aggregate_usage(&db, group_by, project, last)?;
    let pricing = pricing::Pricing::load()?;

    // Each sub-row is one (group, model) pair. Price it by its model, then fold
    // sub-rows back into display groups (preserving first-seen order, which is
    // the DB's token-descending order). A group's cost is the sum of its priced
    // sub-rows; a sub-row whose model is unknown/unpriced is collected as a note
    // and contributes no cost.
    let mut display: Vec<output::DisplayUsageRow> = Vec::new();
    let mut index_of: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut unpriced: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for r in &rows {
        let tokens = pricing::TokenCounts {
            input: r.input_tokens,
            output: r.output_tokens,
            cache_read: r.cache_read_tokens,
            cache_write_5m: r.cache_creation_5m,
            cache_write_1h: r.cache_creation_1h,
        };
        let cost = match r.model.as_deref() {
            Some(m) => {
                let c = pricing.cost_for(m, tokens);
                if c.is_none() {
                    unpriced.insert(m.to_string());
                }
                c
            }
            None => {
                unpriced.insert("(unknown)".to_string());
                None
            }
        };

        let idx = *index_of.entry(r.group.clone()).or_insert_with(|| {
            display.push(output::DisplayUsageRow {
                group: r.group.clone(),
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_5m: 0,
                cache_creation_1h: 0,
                cost: None,
            });
            display.len() - 1
        });
        let d = &mut display[idx];
        d.input_tokens += r.input_tokens;
        d.output_tokens += r.output_tokens;
        d.cache_creation_tokens += r.cache_creation_tokens;
        d.cache_read_tokens += r.cache_read_tokens;
        d.cache_creation_5m += r.cache_creation_5m;
        d.cache_creation_1h += r.cache_creation_1h;
        if let Some(c) = cost {
            d.cost = Some(d.cost.unwrap_or(0.0) + c);
        }
    }

    output::print_usage(
        group_by,
        &display,
        &unpriced.into_iter().collect::<Vec<_>>(),
        json,
    );
    Ok(())
}

fn cmd_friction(
    since: Option<&str>,
    limit: Option<usize>,
    summary: bool,
    json: bool,
) -> Result<()> {
    let db_path = db::default_db_path();
    if !db_path.exists() {
        return Err(error::Error::NoIndex(db_path));
    }
    let db = Database::open(&db_path)?;
    let results = friction::detect_friction(&db, since, limit)?;

    if summary {
        let summary = friction::summarize(&results);
        output::print_friction_summary(&summary, json);
    } else {
        output::print_friction_results(&results, json);
    }
    Ok(())
}
