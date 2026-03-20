use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "nyx",
    about = "Index and search Claude Code conversation history",
    version
)]
pub struct Cli {
    /// Output results as JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show index summary stats
    Status,

    /// Build or update the search index
    Index,

    /// Full-text search across conversations
    Search {
        /// Search query
        query: String,

        /// Scope search to a specific project
        #[arg(long)]
        project: Option<String>,

        /// Time scope (e.g., 7d, 24h, 30d)
        #[arg(long)]
        last: Option<String>,
    },

    /// List all indexed conversations
    List,

    /// Show a conversation transcript
    Show {
        /// Conversation slug (e.g., luminous-toasting-ember)
        slug: String,
    },
}
