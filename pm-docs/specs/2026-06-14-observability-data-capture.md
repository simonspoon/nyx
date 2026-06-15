# nyx — Observability Data Capture (tokens, cost, tool calls)

## Goal

Extend nyx so that, in addition to indexing conversation text for full-text
search, it captures the observability data that already lives in the Claude
Code session JSONL but is currently discarded: per-assistant-message token
usage, model name, request/prompt IDs, and per-tool-call records (tool name +
truncated input + error flag). A new `nyx usage` command aggregates this by
session / project / model / day and reports estimated USD cost computed from an
editable per-model pricing file. When this is done, a user can run
`nyx usage --last 30d` and see token totals and an estimated dollar cost broken
down by model and project, sourced entirely from data nyx already reads.

## Context

nyx is a Rust 2024 CLI that walks `~/.claude/projects/**/*.jsonl`, parses each
line into a `Record` enum, and writes extracted text + conversation metadata to
a SQLite DB at `~/.nyx/index.db` (FTS5 for search). Findings from recon:

- **Schema + version gate.** `src/db.rs:8` `const SCHEMA_VERSION: i32 = 1`.
  `init_schema` (`src/db.rs:43`) creates tables with `CREATE TABLE IF NOT
  EXISTS` and seeds `schema_version` only when empty. `validate_schema_version`
  (`src/db.rs:131`) **errors** (`Error::SchemaMismatch`) when the stored version
  ≠ `SCHEMA_VERSION` — there is no migration logic. The mismatch error message
  (`src/error.rs:27`) already tells users to run `nyx index --rebuild`, **but
  that flag does not exist** (`src/cli.rs:23` `Index` is a bare variant;
  `src/main.rs:63` `cmd_index` has no rebuild path). So a v2 bump requires
  implementing `--rebuild`.
- **The usage data is already parsed-adjacent.** Assistant JSONL lines carry
  `message.usage` (`input_tokens`, `output_tokens`,
  `cache_creation_input_tokens`, `cache_read_input_tokens`, `service_tier`,
  `speed`, and a `cache_creation` sub-object with `ephemeral_5m_input_tokens` /
  `ephemeral_1h_input_tokens`), `message.model`, and top-level `requestId`.
  `promptId` appears only on `user` records. Verified by inspecting
  `~/.claude/projects/-Users-simonspoon-inaros/*.jsonl`.
- **Models in the data** include `claude-opus-4-8`, `claude-fable-5`,
  `claude-sonnet-4-6`, `claude-haiku-4-5` (and `[1m]`/dated variants).
- **`models.rs` currently drops it.** `AssistantMessage` (`src/models.rs:93`)
  parses `role`, `content`, `model` but no `usage`. `AssistantContentBlock`
  (`src/models.rs:109`) parses `ToolUse {}` and `ToolResult {}` as **empty**
  variants — name/input/id are discarded. `tool_result` blocks on user messages
  (`src/models.rs:70` `UserContentBlock::ToolResult`) parse `content` only;
  `tool_use_id` and `is_error` are discarded. Real `tool_use` blocks carry
  `id`, `name`, `input`, `caller`; real `tool_result` blocks carry
  `tool_use_id`, `content`, `is_error`.
- **Two ingestion copies.** Production path: `index_all` →
  `index_file_tx` (`src/indexer.rs:184`) → `insert_message_tx`
  (`src/indexer.rs:302`). Test-only path: `index_file` (`src/indexer.rs:320`,
  `#[cfg(test)]`) → `db.insert_message`. Both build a `record_type` string and
  must be updated in lockstep or the unit tests in `src/indexer.rs` fail.
- **Message insert.** `insert_message` (`src/db.rs:216`) and
  `insert_message_tx` (`src/indexer.rs:302`) both write `(session_id,
  timestamp, role, content, record_type)`. FTS5 sync is via triggers on the
  `messages` table (`src/db.rs:101`) and indexes the `content` column only.
- **Query/CLI patterns.** Commands are `clap` subcommands (`src/cli.rs:18`),
  dispatched in `src/main.rs:32`; each opens the DB, calls a `search.rs`
  function, and prints via `src/output.rs`. `parse_duration` +
  `cutoff_timestamp` (`src/search.rs:37`,`:62`) already turn `7d`/`24h`/`30d`
  into an ISO cutoff string for `--last`-style filters. Timestamps are stored
  as the raw ISO strings from the JSONL.
- **Pricing (base, per MTok).** From the `claude-api` skill (cached
  2026-06-04): Opus 4.x `$5`/`$25`, Sonnet 4.6 `$3`/`$15`, Haiku 4.5 `$1`/`$5`,
  Fable 5 `$10`/`$50`. Cache multipliers from the skill's prompt-caching doc:
  cache-write 5m = 1.25× input, cache-write 1h = 2× input, cache-read = 0.1×
  input. Opus 4.8 and Fable 5 are documented as 1M context at **standard
  pricing — no long-context premium** [web, unverified beyond skill cache], so
  the seed table has no separate 1M tier.
- **No TOML dependency yet.** `Cargo.toml` has clap, rusqlite, serde,
  serde_json, walkdir, thiserror, libc. A config file in TOML needs the `toml`
  crate added (serde is already present).

## Requirements

1. Re-indexing captures, for each assistant message that has a `usage` object:
   `model`, `input_tokens`, `output_tokens`, `cache_creation_input_tokens`,
   `cache_read_input_tokens`, `cache_creation_5m`, `cache_creation_1h`,
   `service_tier`, `speed`, and the message's `request_id`. All are nullable.
2. Re-indexing captures one row per `tool_use` block: `tool_name`, `tool_use_id`,
   truncated `input` (raw JSON, truncated to a fixed byte cap), `session_id`,
   `timestamp`. It records whether the matching `tool_result` had `is_error:
   true` when that result is available in the same file.
3. Tool-call `input` is stored truncated to a fixed cap (default 1024 bytes,
   truncated on a UTF-8 char boundary); a truncation marker distinguishes
   truncated from whole values.
4. Existing behavior is unchanged: `nyx search`, `list`, `show`, `friction`,
   `status` produce the same output as before for the same data; the FTS index
   contains message text only (no tool-call rows).
5. `schema_version` is bumped to 2. Opening a v1 DB produces the existing
   `SchemaMismatch` error directing the user to `nyx index --rebuild`.
6. `nyx index --rebuild` drops and recreates all tables (schema v2) and performs
   a full re-index. `nyx index` with no flag continues to do incremental
   indexing (and errors with the rebuild hint against a v1 DB).
7. A per-model pricing config exists at `~/.nyx/pricing.toml`, seeded with
   current defaults on first run if absent. It maps a model id (or prefix) to
   input / output / cache-write-5m / cache-write-1h / cache-read rates per
   million tokens.
8. `nyx usage` aggregates captured token data and prints, per grouping, total
   input/output/cache tokens and estimated USD cost. It supports `--last <dur>`
   (reusing `parse_duration`), `--project <name>`, a `--by
   session|project|model|day` grouping selector, and `--json`.
9. Cost for a model not present in `pricing.toml` is reported as null/unknown
   (not zero), and `nyx usage` prints a visible note listing unpriced models it
   encountered.

## Non-goals

- Adding tool-call inputs/outputs to the FTS index or making them searchable via
  `nyx search` (explicitly analytics-only).
- Capturing tool *result* content/output bodies (only the `is_error` flag and
  the linkage to a `tool_use` are kept).
- Capturing per-user-message `promptId`, `gitBranch`, `version`, `entrypoint`,
  `cwd`, or `permission-mode` events — out of scope for this pass.
- A migration that preserves an existing v1 DB in place; re-index is required and
  acceptable (JSONL is the source of truth).
- Editing cost/usage into the existing `status`, `search`, or `show` output.
- Auto-refreshing or fetching live prices; the TOML is user-maintained after
  seeding.

## Assumptions

- **Assuming the pricing seed values above are correct as of 2026-06-14** and
  that hardcoding them as the seed (then letting the user edit `pricing.toml`)
  is acceptable — correct me if wrong. Prices are volatile; the editable file is
  the long-term source of truth.
- **Assuming tool-input truncation cap = 1024 bytes.** Big enough to keep most
  Bash commands / file paths, small enough to bound DB growth.
- **Assuming `nyx usage` default grouping is `--by model`** (the most useful
  single view) when no `--by` is given.
- **Assuming model matching in pricing is longest-prefix** (so `claude-opus-4-8`
  and `claude-opus-4-8[1m]` can share one `claude-opus-4-8` entry), falling back
  to exact-match then unpriced.
- **Assuming usage rows live as nullable columns on `messages`** (1:1 with
  assistant messages) and **tool calls live in a new `tool_calls` table**
  (many-per-session). This keeps the FTS trigger surface untouched.
- **Assuming cost is computed at query time** from stored token counts ×
  pricing, never stored in the DB (so editing `pricing.toml` re-prices history
  with no re-index).

## Design

**Storage.** Add nullable columns to `messages`: `model`, `input_tokens`,
`output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`,
`cache_creation_5m`, `cache_creation_1h`, `service_tier`, `speed`,
`request_id`. Add a new table:

```sql
CREATE TABLE tool_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    timestamp TEXT,
    tool_use_id TEXT,
    tool_name TEXT NOT NULL,
    input TEXT,            -- truncated JSON, NULL if none
    input_truncated INTEGER NOT NULL DEFAULT 0,
    is_error INTEGER,      -- NULL until/unless a matching tool_result is seen
    FOREIGN KEY (session_id) REFERENCES conversations(session_id)
);
CREATE INDEX idx_tool_calls_session ON tool_calls(session_id);
CREATE INDEX idx_tool_calls_name ON tool_calls(tool_name);
```

Columns on `messages` are chosen over a side `message_usage` table because usage
is strictly 1:1 with an assistant message and avoids a join in the common
aggregation. The FTS5 virtual table and its triggers are unaffected (they
reference `content` only).

**Schema versioning.** Bump `SCHEMA_VERSION` to 2. Because `init_schema` uses
`CREATE TABLE IF NOT EXISTS` and `validate_schema_version` hard-errors on
mismatch, the only supported upgrade is rebuild. Implement `nyx index
--rebuild`: open DB, `DROP` the `messages`, `conversations`, `source_files`,
`tool_calls`, `messages_fts` (+ triggers) objects, re-run `init_schema`, then
`index_all`. This matches the pre-existing `SchemaMismatch` message and is the
smallest correct path (no migration framework).

**Parsing.** Extend `models.rs`:
- Add a `Usage` struct (`#[derive(Deserialize)]`) with the token fields +
  `service_tier`/`speed` and a nested `cache_creation` struct for the 5m/1h
  split; add `usage: Option<Usage>` to `AssistantMessage`.
- Add `request_id` (rename `requestId`) to the `Assistant` `Record` variant.
- Fill in `ToolUse { id, name, input }` (capture `input` as
  `serde_json::Value` / raw). Leave `caller` unparsed.
- Add `tool_use_id` + `is_error` to `UserContentBlock::ToolResult`.
- Add accessor methods (e.g. `Record::usage()`, `Record::model()`,
  `Record::request_id()`, `Record::tool_uses()`, `Record::tool_results()`)
  mirroring the existing accessor style, so the indexer stays declarative.

**Ingestion.** In both `index_file_tx` (prod) and the `#[cfg(test)] index_file`
copy: when inserting an assistant message, also pass usage/model/request_id to
an extended `insert_message(_tx)`. Within the per-file loop, collect `tool_use`
blocks into `tool_calls` rows and build a `tool_use_id -> is_error` map from
`tool_result` blocks seen in the same file, applied to the rows after the loop
(results usually appear on the *next* user message, so resolve at end-of-file).
Truncate tool input to the cap on a char boundary, setting `input_truncated`.

**Pricing.** New module `pricing.rs`: a `Pricing` map loaded from
`~/.nyx/pricing.toml`; if the file is absent, write the seed table first, then
load. Add the `toml` crate. A `cost_for(model, tokens) -> Option<f64>` helper
applies input/output/cache-read and the 5m/1h cache-write rates; returns `None`
for an unpriced model. Matching: exact id, then longest registered prefix.

**Query + CLI.** New `Command::Usage { last, project, by, }` in `cli.rs`,
dispatched in `main.rs` to a `usage.rs` (or `search.rs`) function that runs a
`GROUP BY` over `messages` filtered by optional project (join `conversations`)
and `--last` cutoff (reuse `cutoff_timestamp`), sums token columns, and the
command layer applies pricing to produce per-group cost and a totals line.
Unpriced models are collected and surfaced as a note. Output via a new
`output.rs` printer (human + `--json`).

## Implementation

1. **Schema v2 + rebuild.** Bump `SCHEMA_VERSION`; add the new `messages`
   columns and `tool_calls` table/indexes to `init_schema`; add `--rebuild` to
   the `Index` clap variant and a drop-and-recreate path in `cmd_index`.
   *Verify:* on a v1 `~/.nyx/index.db`, `nyx status` errors with the rebuild
   hint; `nyx index --rebuild` exits 0 and `nyx status` then works. `cargo test`
   green (update `test_schema_version_mismatch` expectations only if needed).
2. **Parsing.** Extend `models.rs` structs/variants/accessors for usage, model,
   request_id, tool_use, tool_result. *Verify:* new unit tests in `models.rs`
   parse a real assistant line (with `usage` + `tool_use`) and a user line
   (with `tool_result` + `is_error`) and assert extracted fields; existing
   `models.rs` tests still pass (`cargo test models`).
3. **Ingestion + DB writes.** Extend `insert_message(_tx)` signature; add
   `insert_tool_call(_tx)`; update both `index_file_tx` and `index_file` to
   populate usage columns and `tool_calls`, with end-of-file `is_error`
   resolution and char-boundary truncation. *Verify:* a unit test indexes a
   crafted JSONL with one assistant(usage+tool_use) + one user(tool_result) and
   asserts `messages.input_tokens` and a `tool_calls` row with correct
   `tool_name`/`is_error`/`input_truncated`. `cargo test indexer` green.
4. **Pricing.** Add `toml` dep; `pricing.rs` with seed-on-missing, loader, and
   `cost_for`. *Verify:* unit test — loading absent file writes the seed and
   returns Opus cost = expected for a known token vector; unknown model →
   `None`; `[1m]` variant resolves via prefix.
5. **`nyx usage` command.** Add the clap variant, dispatch, query function, and
   output printer (human + `--json`); wire `--last`/`--project`/`--by`; surface
   unpriced-model note. *Verify:* against the real local index after a rebuild,
   `nyx usage --by model` prints non-zero token totals and a dollar figure for
   priced models; `nyx usage --by project --json` emits valid JSON;
   `nyx usage --last 7d` runs without error.
6. **Docs + full check.** Update `README.md` (new command, `--rebuild`, pricing
   file, the retired `--export-suda` mention can stay untouched per scope).
   *Verify:* `cargo build --release`, `cargo test`, and `cargo clippy` clean;
   `nyx usage --help` shows the documented flags.

## Open questions

- **Exact long-context (1M) pricing tiers.** The skill states Opus 4.8 / Fable 5
  are 1M at standard pricing, but doesn't give a numeric >200K premium for any
  4.x model. The spec assumes no separate 1M tier in the seed; if a premium tier
  is later confirmed, it is added as additional `pricing.toml` entries
  (mechanism already supports it) — no code change.

## Acceptance

- [ ] `nyx index --rebuild` on a pre-existing v1 DB succeeds; subsequent
      `nyx status` works (v2). (`nyx index --rebuild && nyx status`)
- [ ] After rebuild, `SELECT count(*) FROM messages WHERE input_tokens IS NOT
      NULL` > 0 and `SELECT count(*) FROM tool_calls` > 0 on the real local data.
- [ ] `nyx usage --by model` shows per-model input/output token totals and a USD
      cost for priced models, and a note naming any unpriced models.
- [ ] `nyx usage --by project --json` emits valid JSON parseable by `jq`.
- [ ] `~/.nyx/pricing.toml` is created with the seed table when absent; editing
      a rate changes the reported cost with no re-index.
- [ ] `nyx search`, `list`, `show`, `friction`, `status` outputs are unchanged
      for unchanged input (regression check on a known query).
- [ ] `cargo test` and `cargo clippy` are clean.

## Appendix: Q&A

**Q1 (data scope).** How much observability data should nyx capture?
**A:** Full — tokens + cost + tool calls.

**Q2 (action).** Build it now or plan it?
**A:** Write a spec first.

**Q3 (pricing source).** Where should the per-model price table live?
**A:** Editable config file (`~/.nyx/pricing.toml`, seeded with defaults on
first run).

**Q4 (tool input).** How much of each tool call's input to store?
**A:** Truncated input (store name + input JSON truncated to a cap).

**Q5 (FTS scope).** Should tool-call data be full-text searchable?
**A:** Analytics-only — `tool_calls` in a plain table; `nyx search` unchanged.
