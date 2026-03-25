# nyx

Index and search Claude Code conversation history from the command line.

## Overview

Nyx reads JSONL conversation files from `~/.claude/projects/`, parses them into structured records, and indexes the text content into a SQLite database with FTS5 full-text search. The index enables fast, flexible searching across all conversations with project and time filtering.

## Install

### Homebrew

```bash
brew install simonspoon/tap/nyx
```

### From GitHub Releases

Download a pre-built binary from [Releases](https://github.com/simonspoon/nyx/releases) and place it on your PATH.

### From source

```bash
cargo install --git https://github.com/simonspoon/nyx.git
```

## Usage

### Build the index

```bash
nyx index
```

Indexes all JSONL conversation files from `~/.claude/projects/` into a SQLite database at `~/.nyx/index.db`. Supports incremental indexing -- only re-indexes files that have changed.

### Check status

```bash
nyx status
```

```
Index: ~/.nyx/index.db (last updated: 2026-03-20 14:32)
Records: 54,823 across 297 conversations in 26 projects
Date range: 2026-02-01 to 2026-03-20
Size: 194.8 MB source, 42.1 MB index

Top projects:
  claudehub          89 conversations
  limbo              45 conversations
  ...
```

### Search conversations

```bash
nyx search "error handling"
nyx search "sqlite" --project claudehub
nyx search "deployment" --last 7d
```

Full-text search with FTS5. Results are grouped by conversation with highlighted matches. Limited to 100 results, ordered newest first.

Options:
- `--project <PROJECT>` — Filter results to a specific project
- `--last <DURATION>` — Filter to recent conversations: `7d` (days), `24h` (hours), `30m` (minutes)

### List conversations

```bash
nyx list
```

Shows all indexed conversations with slug, date, and project.

### Show a conversation

```bash
nyx show luminous-toasting-ember
```

Prints the conversation as a readable transcript (user and assistant turns only). Accepts slugs or session ID prefixes.

### JSON output

All commands support `--json` for machine-readable output:

```bash
nyx --json search "query"
nyx --json status
nyx --json list
```

## Architecture

```
main.rs        Entry point, dispatches commands
cli.rs         Clap-based CLI definition
db.rs          Database connection, schema init, CRUD
models.rs      Serde deserialization of JSONL record types
indexer.rs     File discovery, parsing, incremental indexing
search.rs      FTS5 queries, listing, transcript retrieval
output.rs      Human and JSON formatting
error.rs       Custom error types
```

### Database

Stored at `~/.nyx/index.db` (SQLite with WAL mode).

Tables:
- **conversations** — Session ID, slug, project, timestamps, custom title
- **messages** — Session ID, timestamp, role, content, record type
- **messages_fts** — FTS5 virtual table for full-text search (unicode61 tokenizer)
- **source_files** — Mtime tracking for incremental indexing

### Record Types

Nyx parses these JSONL record types from Claude Code conversation files:
- **User** — User messages (text or block-based content)
- **Assistant** — Claude responses (text, thinking blocks, tool usage)
- **System** — System events
- **CustomTitle** — User-defined conversation titles
- Progress and FileHistorySnapshot records are skipped during indexing.

## License

MIT
