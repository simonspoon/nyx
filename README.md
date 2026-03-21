# nyx

Index and search Claude Code conversation history.

## Install

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

Full-text search with FTS5. Results are grouped by conversation with highlighted matches.

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

## How it works

nyx reads Claude Code's JSONL conversation files stored in `~/.claude/projects/`, parses them into structured records, and indexes the text content into a SQLite database with FTS5 full-text search. The index lives at `~/.nyx/index.db`.

## License

MIT
