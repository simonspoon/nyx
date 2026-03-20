mod cli;
mod db;
mod error;
mod indexer;
mod models;
mod output;
mod search;

use clap::Parser;

use cli::{Cli, Command};
use db::Database;
use error::Result;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Status => cmd_status(cli.json),
        Command::Index => cmd_index(cli.json),
        Command::Search {
            query,
            project,
            last,
        } => cmd_search(query, project.as_deref(), last.as_deref(), cli.json),
        Command::List => cmd_list(cli.json),
        Command::Show { slug } => cmd_show(slug, cli.json),
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

fn cmd_index(json: bool) -> Result<()> {
    let db_path = db::default_db_path();
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
