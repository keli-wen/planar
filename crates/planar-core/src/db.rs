use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::burst::{
    IDLE_GAP_CAP_MS, RawSpan, format_tz_label, fractional_hour, local_midnight_ms,
    merge_activity_spans, project_bursts_into_days, sum_interval_minutes_in_range,
};
use crate::defaults::{
    FEWEST_USER_INPUTS_MIN, LINE_PRIORITY_MAX, LINE_PRIORITY_MIN, MAX_SHORT_ITEMS_RETURNED,
    ONE_OFF_LINE_COLOR_INDEX, QUERY_LIMIT_MAX, QUERY_LIMIT_MIN, SEARCH_DAYS_MAX,
    SESSIONS_SHOWN_PER_LINE_MIN, SLICER_STEPS_MIN, STATUS_WINDOW_DAYS_MAX, SYNC_BATCH_LIMIT_MAX,
    SYNC_BATCH_LIMIT_MIN, SYNC_CONCURRENCY_MAX, SYNC_CONCURRENCY_MIN, WINDOW_DAYS_MAX,
    WINDOW_DAYS_MIN, harness_order, palette_color,
};
pub use crate::defaults::{MAX_L1_BATCH_LINES, MAX_L2_BATCH_SESSIONS};
use crate::env::{HarnessRoots, cli_binary_name_for_harness, resolve_cli_binary};
use crate::error::{PlanarError, Result};
use crate::model::{
    AmbiguousSessionHint, AppSettings, BatchLinesResponse, BatchSessionsResponse,
    ContributionDayDto, DayMeta, HarnessCount, HarnessStatusDto, INTERNAL_DIGEST_SENTINEL,
    LineDetailDto, LineDto, LineHint, LineIdUpdate, LineSessionDigestDto, LitePlaneResponse,
    LiveSessionSummary, ONE_OFF_LINE_ID, PlaneResponse, SearchHitDto, SearchQuery, SearchResponse,
    SessionDepLink, SessionDigestSummaryDto, SessionDto, SessionFocus, SessionSliceDto,
    ShortItemDto, ShortSessionsDto, SlicedTurnDto, StatusDependency, StatusLineSummary,
    StatusSessionPreview, StatusSnapshot, UnsummarizedDto,
};
use crate::pricing::estimate_cost_usd;

const SETTINGS_ROW_KEY: &str = "app_settings";

struct ExistingSessionState {
    step_count: i64,
    user_input_count: i64,
    first_user_prompt: String,
    user_prompt_timestamps_json: String,
    cwd: Option<String>,
    started_at: i64,
    last_active_at: i64,
    has_open_turn: i64,
    model: Option<String>,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

/// Metadata required to resume or reveal an ingested session in its source harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLaunchInfo {
    pub id: String,
    pub harness: String,
    pub cwd: Option<String>,
    pub source_path: String,
}

/// File cursor persisted in `sessions` for O(1) skip and byte-offset incremental ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCursor {
    pub session_id: String,
    pub source_size: u64,
    pub source_mtime_ms: i64,
}

/// How an ingested session update should be applied to the SQLite store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestMode {
    /// Full parse from byte 0: replaces intervals, prompt timestamps, and counts.
    FullReplace,
    /// Incremental tail parse from previous byte cursor: merges new deltas with stored state.
    Incremental,
}

/// Parsed trajectory data produced by a harness scanner.
#[derive(Debug, Clone)]
pub struct IngestedSession {
    pub id: String,
    pub harness: String,
    pub source_path: String,
    pub source_size: u64,
    pub source_mtime_ms: i64,
    pub mode: IngestMode,
    pub cwd: Option<String>,
    pub step_count: u64,
    pub user_input_count: u64,
    pub first_user_prompt: String,
    pub prompt_timestamps_ms: Vec<i64>,
    pub raw_spans: Vec<RawSpan>,
    pub archived: bool,
    pub is_subagent: bool,
    pub parent_session_id: Option<String>,
    pub saw_turn_state: bool,
    pub has_open_turn: bool,
    pub started_at: i64,
    pub last_active_at: i64,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub usage_is_cumulative: bool,
}

/// Parameters for updating a Line of work.
#[derive(Debug, Clone, Default)]
pub struct LineUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub priority: Option<u8>,
    pub status: Option<String>,
    pub pinned: Option<bool>,
}

/// SQLite WAL-backed store for Planar state (`~/.planar/planar.db`).
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens or creates the SQLite database at `path`, configuring mandatory WAL PRAGMAs
    /// (`journal_mode = WAL`, `busy_timeout = 5000`, `synchronous = NORMAL`) and schema.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = 5000;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;

        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Opens an existing SQLite database strictly read-only without creating parent directories,
    /// creating a missing file, switching journal modes, or running schema migrations.
    ///
    /// Returns `Ok(None)` if `path` does not exist or has not yet been initialized with Planar's
    /// core tables.
    pub fn open_readonly(path: &Path) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;
        conn.execute_batch(
            "
            PRAGMA busy_timeout = 5000;
            PRAGMA query_only = ON;
            ",
        )?;
        let required_tables: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name IN ('app_settings', 'lines', 'sessions', 'session_digests')",
            [],
            |row| row.get(0),
        )?;
        if required_tables < 4 {
            return Ok(None);
        }
        Ok(Some(Self { conn }))
    }

    /// Borrows the underlying SQLite connection (used for diagnostics and tests).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS lines (
                id               TEXT PRIMARY KEY,
                name             TEXT NOT NULL,
                description      TEXT NOT NULL DEFAULT '',
                color            TEXT NOT NULL,
                pinned           INTEGER NOT NULL DEFAULT 1,
                is_system_bucket INTEGER NOT NULL DEFAULT 0,
                priority         INTEGER NOT NULL DEFAULT 0,
                status           TEXT NOT NULL DEFAULT 'active',
                sort_order       INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id                          TEXT PRIMARY KEY,
                harness                     TEXT NOT NULL,
                source_path                 TEXT NOT NULL,
                source_size                 INTEGER NOT NULL DEFAULT 0,
                source_mtime_ms             INTEGER NOT NULL DEFAULT 0,
                cwd                         TEXT,
                step_count                  INTEGER NOT NULL DEFAULT 0,
                user_input_count            INTEGER NOT NULL DEFAULT 0,
                first_user_prompt           TEXT NOT NULL DEFAULT '',
                user_prompt_timestamps_json TEXT NOT NULL DEFAULT '[]',
                archived                    INTEGER NOT NULL DEFAULT 0,
                hidden                      INTEGER NOT NULL DEFAULT 0,
                is_subagent                 INTEGER NOT NULL DEFAULT 0,
                parent_session_id           TEXT,
                has_open_turn               INTEGER NOT NULL DEFAULT 0,
                line_id                     TEXT REFERENCES lines(id) ON DELETE SET NULL,
                started_at                  INTEGER NOT NULL DEFAULT 0,
                last_active_at              INTEGER NOT NULL DEFAULT 0,
                model                       TEXT,
                input_tokens                INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens           INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens          INTEGER NOT NULL DEFAULT 0,
                output_tokens               INTEGER NOT NULL DEFAULT 0,
                total_tokens                INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_source_path
                ON sessions(source_path);
            CREATE INDEX IF NOT EXISTS idx_sessions_line_id
                ON sessions(line_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_last_active
                ON sessions(last_active_at DESC);

            CREATE TABLE IF NOT EXISTS activity_intervals (
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                start_ms   INTEGER NOT NULL,
                end_ms     INTEGER NOT NULL,
                PRIMARY KEY (session_id, start_ms)
            );

            CREATE TABLE IF NOT EXISTS session_digests (
                session_id        TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                last_step_index   INTEGER NOT NULL DEFAULT 0,
                slice_config_hash TEXT NOT NULL DEFAULT '',
                title             TEXT NOT NULL,
                stopped_at        TEXT NOT NULL,
                left_open         TEXT NOT NULL DEFAULT '',
                hidden            INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS session_dependencies (
                from_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                to_session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                kind            TEXT NOT NULL DEFAULT 'depends_on',
                source          TEXT NOT NULL DEFAULT 'manual',
                PRIMARY KEY (from_session_id, to_session_id)
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key        TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS known_subagents (
                child_session_id  TEXT PRIMARY KEY,
                parent_session_id TEXT NOT NULL
            );
            ",
        )?;

        self.conn.execute(
            "INSERT OR IGNORE INTO lines (id, name, color, pinned, is_system_bucket, priority, status, sort_order)
             VALUES (?1, ?2, ?3, 1, 1, 0, 'active', 9999)",
            params![ONE_OFF_LINE_ID, "One-off", palette_color(ONE_OFF_LINE_COLOR_INDEX)],
        )?;

        self.migrate_line_description()?;
        self.migrate_token_columns()?;
        self.migrate_prompt_envelopes()?;
        self.migrate_batch_default_4()?;
        self.migrate_sessions_fts()?;

        Ok(())
    }

    fn migrate_line_description(&mut self) -> Result<()> {
        let mut existing_cols = HashSet::new();
        {
            let mut stmt = self.conn.prepare("PRAGMA table_info(lines)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            for col in rows {
                existing_cols.insert(col?);
            }
        }
        if !existing_cols.contains("description") {
            self.conn.execute_batch(
                "ALTER TABLE lines ADD COLUMN description TEXT NOT NULL DEFAULT '';",
            )?;
        }
        Ok(())
    }

    fn migrate_token_columns(&mut self) -> Result<()> {
        let mut existing_cols = HashSet::new();
        {
            let mut stmt = self.conn.prepare("PRAGMA table_info(sessions)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            for col in rows {
                existing_cols.insert(col?);
            }
        }

        for (col_name, col_ddl) in [
            ("model", "TEXT"),
            ("input_tokens", "INTEGER NOT NULL DEFAULT 0"),
            ("cache_read_tokens", "INTEGER NOT NULL DEFAULT 0"),
            ("cache_write_tokens", "INTEGER NOT NULL DEFAULT 0"),
            ("output_tokens", "INTEGER NOT NULL DEFAULT 0"),
            ("total_tokens", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !existing_cols.contains(col_name) {
                self.conn.execute_batch(&format!(
                    "ALTER TABLE sessions ADD COLUMN {col_name} {col_ddl};"
                ))?;
            }
        }

        let backfill_done: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'schema_token_backfill_v1'",
                [],
                |r| r.get(0),
            )
            .optional()?;

        // One-time cursor reset when upgrading an older `planar.db` so existing on-disk
        // transcripts backfill token & model metrics on next scan while pruned logs stay untouched.
        if backfill_done.is_none() {
            let mut candidates = Vec::new();
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, source_path FROM sessions WHERE source_size > 0")?;
                let rows =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for item in rows {
                    let (id, sp) = item?;
                    if !sp.is_empty() && Path::new(&sp).exists() {
                        candidates.push(id);
                    }
                }
            }
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if !candidates.is_empty() {
                let mut reset_stmt = tx.prepare(
                    "UPDATE sessions SET source_size = 0, source_mtime_ms = 0 WHERE id = ?1",
                )?;
                for id in candidates {
                    reset_stmt.execute(params![id])?;
                }
            }
            tx.execute(
                "INSERT OR REPLACE INTO app_settings (key, value_json) VALUES ('schema_token_backfill_v1', '1')",
                [],
            )?;
            tx.commit()?;
        }

        Ok(())
    }

    fn migrate_prompt_envelopes(&mut self) -> Result<()> {
        let done: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'schema_prompt_envelope_v1'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if done.is_some() {
            return Ok(());
        }

        let mut candidates = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, source_path FROM sessions
                 WHERE source_size > 0
                   AND (harness = 'codex'
                        OR first_user_prompt LIKE '[cospan%'
                        OR first_user_prompt LIKE '<skill%')",
            )?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for item in rows {
                let (id, sp) = item?;
                if !sp.is_empty() && Path::new(&sp).exists() {
                    candidates.push(id);
                }
            }
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !candidates.is_empty() {
            let mut reset_stmt = tx.prepare(
                "UPDATE sessions SET source_size = 0, source_mtime_ms = 0 WHERE id = ?1",
            )?;
            for id in candidates {
                reset_stmt.execute(params![id])?;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO app_settings (key, value_json) VALUES ('schema_prompt_envelope_v1', '1')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn migrate_batch_default_4(&mut self) -> Result<()> {
        const LEGACY_SYNC_BATCH_LIMIT: u64 = 8;
        const MIGRATED_SYNC_BATCH_LIMIT: usize = 4;

        let done: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'schema_batch_default_4_v1'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if done.is_some() {
            return Ok(());
        }

        let persisted_raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![SETTINGS_ROW_KEY],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(raw_json) = persisted_raw
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw_json)
            && val
                .get("syncBatchLimit")
                .and_then(serde_json::Value::as_u64)
                == Some(LEGACY_SYNC_BATCH_LIMIT)
            && let Ok(mut settings) = serde_json::from_str::<AppSettings>(&raw_json)
        {
            settings.sync_batch_limit = MIGRATED_SYNC_BATCH_LIMIT;
            let _ = self.save_settings(&settings);
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value_json) VALUES ('schema_batch_default_4_v1', '1')",
            [],
        )?;
        Ok(())
    }

    fn migrate_sessions_fts(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
                session_id UNINDEXED,
                title,
                stopped_at,
                left_open,
                first_user_prompt,
                tokenize = 'unicode61'
            );",
        )?;

        let done: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'schema_sessions_fts_v1'",
                [],
                |r| r.get(0),
            )
            .optional()?;

        let needs_backfill = if done.is_none() {
            true
        } else {
            let fts_count: i64 =
                self.conn
                    .query_row("SELECT COUNT(*) FROM sessions_fts", [], |r| r.get(0))?;
            if fts_count > 0 {
                false
            } else {
                let eligible_count: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM sessions WHERE is_subagent = 0 AND step_count > 0",
                    [],
                    |r| r.get(0),
                )?;
                eligible_count > 0
            }
        };

        if needs_backfill {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("DELETE FROM sessions_fts", [])?;
            tx.execute(
                "INSERT INTO sessions_fts (rowid, session_id, title, stopped_at, left_open, first_user_prompt)
                 SELECT s.rowid,
                        s.id,
                        COALESCE(d.title, ''),
                        COALESCE(d.stopped_at, ''),
                        COALESCE(d.left_open, ''),
                        s.first_user_prompt
                 FROM sessions s
                 LEFT JOIN session_digests d ON d.session_id = s.id
                 WHERE s.is_subagent = 0
                   AND s.step_count > 0
                   AND INSTR(s.first_user_prompt, ?1) = 0",
                params![INTERNAL_DIGEST_SENTINEL],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO app_settings (key, value_json) VALUES ('schema_sessions_fts_v1', '1')",
                [],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Loads saved `source_path -> FileCursor` entries for incremental scanning.
    pub fn load_file_cursors(&self) -> Result<HashMap<String, FileCursor>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source_path, id, source_size, source_mtime_ms FROM sessions")?;
        let rows = stmt.query_map([], |row| {
            let source_path: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let size_i64: i64 = row.get(2)?;
            let source_mtime_ms: i64 = row.get(3)?;
            Ok((
                source_path,
                FileCursor {
                    session_id,
                    source_size: u64::try_from(size_i64.max(0)).unwrap_or(0),
                    source_mtime_ms,
                },
            ))
        })?;

        let mut map = HashMap::new();
        for item in rows {
            let (path, cursor) = item?;
            map.insert(path, cursor);
        }
        Ok(map)
    }

    /// Inserts or updates an ingested session and its clamped burst intervals inside one transaction.
    ///
    /// Existing manual `line_id` and `hidden` values are always preserved.
    /// Sessions flagged with `INTERNAL_DIGEST_SENTINEL` are ignored so background CLI runs never self-ingest.
    pub fn upsert_ingested_session(&mut self, data: &IngestedSession) -> Result<()> {
        if data.first_user_prompt.contains(INTERNAL_DIGEST_SENTINEL) {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing: Option<ExistingSessionState> = tx
            .query_row(
                "SELECT step_count, user_input_count, first_user_prompt, user_prompt_timestamps_json,
                        cwd, started_at, last_active_at, has_open_turn,
                        model, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, total_tokens
                 FROM sessions WHERE id = ?1",
                params![&data.id],
                |row| {
                    Ok(ExistingSessionState {
                        step_count: row.get(0)?,
                        user_input_count: row.get(1)?,
                        first_user_prompt: row.get(2)?,
                        user_prompt_timestamps_json: row.get(3)?,
                        cwd: row.get(4)?,
                        started_at: row.get(5)?,
                        last_active_at: row.get(6)?,
                        has_open_turn: row.get(7)?,
                        model: row.get(8)?,
                        input_tokens: row.get(9)?,
                        cache_read_tokens: row.get(10)?,
                        cache_write_tokens: row.get(11)?,
                        output_tokens: row.get(12)?,
                        total_tokens: row.get(13)?,
                    })
                },
            )
            .optional()?;

        let data_in = i64::try_from(data.input_tokens).unwrap_or(i64::MAX);
        let data_cr = i64::try_from(data.cache_read_tokens).unwrap_or(i64::MAX);
        let data_cw = i64::try_from(data.cache_write_tokens).unwrap_or(i64::MAX);
        let data_out = i64::try_from(data.output_tokens).unwrap_or(i64::MAX);
        let data_tot = i64::try_from(data.total_tokens).unwrap_or(i64::MAX);

        let (
            final_steps,
            final_user_inputs,
            final_first_prompt,
            final_prompts,
            final_cwd,
            final_started,
            final_last_active,
            final_open_turn,
            final_model,
            final_in_tok,
            final_cr_tok,
            final_cw_tok,
            final_out_tok,
            final_tot_tok,
            merged_intervals,
        ) = if let (IngestMode::Incremental, Some(old)) = (data.mode, existing) {
            let mut prompts: Vec<i64> =
                serde_json::from_str(&old.user_prompt_timestamps_json).unwrap_or_default();
            prompts.extend_from_slice(&data.prompt_timestamps_ms);
            prompts.sort_unstable();
            prompts.dedup();

            let mut spans = load_intervals_tx(&tx, &data.id)?;
            spans.extend_from_slice(&data.raw_spans);
            let merged = merge_activity_spans(&mut spans, IDLE_GAP_CAP_MS);

            let first_prompt = if old.first_user_prompt.is_empty() {
                data.first_user_prompt.clone()
            } else {
                old.first_user_prompt
            };
            let cwd = data.cwd.clone().or(old.cwd);
            let started = min_positive(old.started_at, data.started_at);
            let last_active = old.last_active_at.max(data.last_active_at);
            let open_turn = if data.saw_turn_state {
                i64::from(data.has_open_turn)
            } else {
                old.has_open_turn
            };
            let model = data.model.clone().filter(|m| !m.is_empty()).or(old.model);
            let (in_tok, cr_tok, cw_tok, out_tok, tot_tok) = if data.usage_is_cumulative {
                if data.total_tokens > 0 {
                    (data_in, data_cr, data_cw, data_out, data_tot)
                } else {
                    (
                        old.input_tokens,
                        old.cache_read_tokens,
                        old.cache_write_tokens,
                        old.output_tokens,
                        old.total_tokens,
                    )
                }
            } else {
                (
                    old.input_tokens.saturating_add(data_in),
                    old.cache_read_tokens.saturating_add(data_cr),
                    old.cache_write_tokens.saturating_add(data_cw),
                    old.output_tokens.saturating_add(data_out),
                    old.total_tokens.saturating_add(data_tot),
                )
            };

            (
                old.step_count
                    .saturating_add(i64::try_from(data.step_count).unwrap_or(i64::MAX)),
                old.user_input_count
                    .saturating_add(i64::try_from(data.user_input_count).unwrap_or(i64::MAX)),
                first_prompt,
                prompts,
                cwd,
                started,
                last_active,
                open_turn,
                model,
                in_tok,
                cr_tok,
                cw_tok,
                out_tok,
                tot_tok,
                merged,
            )
        } else {
            let mut prompts = data.prompt_timestamps_ms.clone();
            prompts.sort_unstable();
            prompts.dedup();

            let mut spans = data.raw_spans.clone();
            let merged = merge_activity_spans(&mut spans, IDLE_GAP_CAP_MS);

            (
                i64::try_from(data.step_count).unwrap_or(i64::MAX),
                i64::try_from(data.user_input_count).unwrap_or(i64::MAX),
                data.first_user_prompt.clone(),
                prompts,
                data.cwd.clone(),
                data.started_at,
                data.last_active_at,
                i64::from(data.has_open_turn),
                data.model.clone().filter(|m| !m.is_empty()),
                data_in,
                data_cr,
                data_cw,
                data_out,
                data_tot,
                merged,
            )
        };

        let known_parent: Option<String> = tx
            .query_row(
                "SELECT parent_session_id FROM known_subagents WHERE child_session_id = ?1",
                params![&data.id],
                |row| row.get(0),
            )
            .optional()?;
        let effective_is_subagent = data.is_subagent || known_parent.is_some();
        let effective_parent = data.parent_session_id.clone().or(known_parent);

        let prompts_json = serde_json::to_string(&final_prompts)?;
        let size_i64 = i64::try_from(data.source_size).unwrap_or(i64::MAX);

        tx.execute(
            "INSERT INTO sessions (
                id, harness, source_path, source_size, source_mtime_ms, cwd,
                step_count, user_input_count, first_user_prompt, user_prompt_timestamps_json,
                archived, hidden, is_subagent, parent_session_id, has_open_turn,
                line_id, started_at, last_active_at,
                model, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, total_tokens
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10,
                ?11, 0, ?12, ?13, ?14,
                NULL, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21, ?22
             )
             ON CONFLICT(id) DO UPDATE SET
                harness = excluded.harness,
                source_path = excluded.source_path,
                source_size = excluded.source_size,
                source_mtime_ms = excluded.source_mtime_ms,
                cwd = COALESCE(excluded.cwd, sessions.cwd),
                step_count = excluded.step_count,
                user_input_count = excluded.user_input_count,
                first_user_prompt = CASE
                    WHEN excluded.first_user_prompt != '' THEN excluded.first_user_prompt
                    ELSE sessions.first_user_prompt
                END,
                user_prompt_timestamps_json = excluded.user_prompt_timestamps_json,
                archived = excluded.archived,
                is_subagent = CASE
                    WHEN excluded.is_subagent = 1 THEN 1
                    ELSE sessions.is_subagent
                END,
                parent_session_id = COALESCE(excluded.parent_session_id, sessions.parent_session_id),
                has_open_turn = excluded.has_open_turn,
                started_at = CASE
                    WHEN sessions.started_at <= 0 THEN excluded.started_at
                    WHEN excluded.started_at <= 0 THEN sessions.started_at
                    ELSE MIN(sessions.started_at, excluded.started_at)
                END,
                last_active_at = MAX(sessions.last_active_at, excluded.last_active_at),
                model = COALESCE(excluded.model, sessions.model),
                input_tokens = excluded.input_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_write_tokens = excluded.cache_write_tokens,
                output_tokens = excluded.output_tokens,
                total_tokens = excluded.total_tokens",
            params![
                &data.id,
                &data.harness,
                &data.source_path,
                size_i64,
                data.source_mtime_ms,
                &final_cwd,
                final_steps,
                final_user_inputs,
                &final_first_prompt,
                &prompts_json,
                i64::from(data.archived),
                i64::from(effective_is_subagent),
                &effective_parent,
                final_open_turn,
                final_started,
                final_last_active,
                &final_model,
                final_in_tok,
                final_cr_tok,
                final_cw_tok,
                final_out_tok,
                final_tot_tok,
            ],
        )?;

        tx.execute(
            "DELETE FROM activity_intervals WHERE session_id = ?1",
            params![&data.id],
        )?;

        {
            let mut ins = tx.prepare(
                "INSERT INTO activity_intervals (session_id, start_ms, end_ms)
                 VALUES (?1, ?2, ?3)",
            )?;
            for iv in merged_intervals {
                ins.execute(params![&data.id, iv.start_ms, iv.end_ms])?;
            }
        }

        sync_session_fts_conn(&tx, &data.id)?;
        tx.commit()?;
        Ok(())
    }

    /// Marks discovered child subagent sessions (`child_id -> parent_id`) so they are excluded
    /// from top-level user session lists. Returns the number of newly/updated rows.
    pub fn mark_subagent_sessions(&mut self, links: &[(String, String)]) -> Result<usize> {
        if links.is_empty() {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut updated = 0_usize;
        {
            let mut ins_known = tx.prepare(
                "INSERT INTO known_subagents (child_session_id, parent_session_id)
                 VALUES (?1, ?2)
                 ON CONFLICT(child_session_id) DO NOTHING",
            )?;
            let mut stmt = tx.prepare(
                "UPDATE sessions
                 SET is_subagent = 1,
                     parent_session_id = COALESCE(parent_session_id, ?2)
                 WHERE id = ?1 AND (is_subagent = 0 OR parent_session_id IS NULL)",
            )?;
            for (child_id, parent_id) in links {
                ins_known.execute(params![child_id, parent_id])?;
                let changed = stmt.execute(params![child_id, parent_id])?;
                if changed > 0 {
                    updated += changed;
                    sync_session_fts_conn(&tx, child_id)?;
                }
            }
        }
        tx.commit()?;
        Ok(updated)
    }

    /// Loads persisted application settings, falling back to `AppSettings::default()`.
    pub fn get_settings(&self) -> Result<AppSettings> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![SETTINGS_ROW_KEY],
                |row| row.get(0),
            )
            .optional()?;

        match raw {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(AppSettings::default()),
        }
    }

    /// Persists `AppSettings` into `app_settings`.
    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let defaults = AppSettings::default();
        let mut sanitized = settings.clone();
        sanitized.fewest_user_inputs = sanitized.fewest_user_inputs.max(FEWEST_USER_INPUTS_MIN);
        sanitized.opening_steps = sanitized.opening_steps.max(SLICER_STEPS_MIN);
        sanitized.closing_steps = sanitized.closing_steps.max(SLICER_STEPS_MIN);
        sanitized.sessions_shown_per_line = sanitized
            .sessions_shown_per_line
            .max(SESSIONS_SHOWN_PER_LINE_MIN);
        sanitized.sync_batch_limit = sanitized
            .sync_batch_limit
            .clamp(SYNC_BATCH_LIMIT_MIN, SYNC_BATCH_LIMIT_MAX);
        sanitized.sync_concurrency = sanitized
            .sync_concurrency
            .clamp(SYNC_CONCURRENCY_MIN, SYNC_CONCURRENCY_MAX);
        if !matches!(sanitized.language.as_str(), "zh" | "en") {
            sanitized.language = defaults.language;
        }
        if !matches!(sanitized.theme.as_str(), "system" | "light" | "dark") {
            sanitized.theme = defaults.theme;
        }
        if !matches!(sanitized.metric_lens.as_str(), "time" | "tokens" | "cost") {
            sanitized.metric_lens = defaults.metric_lens;
        }
        if !matches!(sanitized.processor_mode.as_str(), "cli" | "api") {
            sanitized.processor_mode = defaults.processor_mode;
        }

        let json = serde_json::to_string(&sanitized)?;
        self.conn.execute(
            "INSERT INTO app_settings (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![SETTINGS_ROW_KEY, json],
        )?;
        Ok(())
    }

    /// Creates a pinned Line of work with a unique slug and categorical color.
    pub fn create_line(&self, name: &str) -> Result<String> {
        self.create_line_with_description(name, "")
    }

    /// Creates a pinned Line of work with an optional scope `description` for AI clustering.
    pub fn create_line_with_description(&self, name: &str, description: &str) -> Result<String> {
        let id = insert_line_conn(&self.conn, name, true)?;
        let desc = sanitize_line_description(description);
        if !desc.is_empty() {
            self.conn.execute(
                "UPDATE lines SET description = ?2 WHERE id = ?1",
                params![&id, &desc],
            )?;
        }
        Ok(id)
    }

    /// Updates mutable fields (`name`, `description`, `color`, `priority`, `status`, `pinned`) of a Line.
    pub fn update_line(&self, line_id: &str, update: &LineUpdate) -> Result<()> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT is_system_bucket FROM lines WHERE id = ?1",
                params![line_id],
                |row| row.get(0),
            )
            .optional()?;

        let Some(is_system) = exists else {
            return Err(PlanarError::NotFound(format!("line `{line_id}`")));
        };

        if let Some(ref name) = update.name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(PlanarError::InvalidInput(
                    "line name cannot be empty".into(),
                ));
            }
            self.conn.execute(
                "UPDATE lines SET name = ?2 WHERE id = ?1",
                params![line_id, trimmed],
            )?;
        }

        if let Some(ref description) = update.description
            && is_system == 0
        {
            let desc = sanitize_line_description(description);
            self.conn.execute(
                "UPDATE lines SET description = ?2 WHERE id = ?1",
                params![line_id, &desc],
            )?;
        }

        if let Some(ref color) = update.color
            && is_system == 0
        {
            let trimmed = color.trim();
            let valid_hex = trimmed.len() == 7
                && trimmed.starts_with('#')
                && trimmed[1..].bytes().all(|b| b.is_ascii_hexdigit());
            if !valid_hex {
                return Err(PlanarError::InvalidInput(
                    "line color must be a #RRGGBB hex string".into(),
                ));
            }
            self.conn.execute(
                "UPDATE lines SET color = ?2 WHERE id = ?1",
                params![line_id, trimmed],
            )?;
        }

        if let Some(priority) = update.priority {
            if priority > 4 {
                return Err(PlanarError::InvalidInput(
                    "priority must be between 0 and 4".into(),
                ));
            }
            self.conn.execute(
                "UPDATE lines SET priority = ?2 WHERE id = ?1",
                params![line_id, i64::from(priority)],
            )?;
        }

        if let Some(ref status) = update.status {
            if !matches!(status.as_str(), "active" | "paused" | "done") {
                return Err(PlanarError::InvalidInput(
                    "status must be active, paused, or done".into(),
                ));
            }
            self.conn.execute(
                "UPDATE lines SET status = ?2 WHERE id = ?1",
                params![line_id, status],
            )?;
        }

        if let Some(pinned) = update.pinned
            && is_system == 0
        {
            self.conn.execute(
                "UPDATE lines SET pinned = ?2 WHERE id = ?1",
                params![line_id, i64::from(pinned)],
            )?;
        }

        Ok(())
    }

    /// Merges `source_line_id` into `target_line_id`, reassigning all sessions and removing the source line.
    ///
    /// When `target_line_id` is a non-system line, optionally pins the target and appends the merged
    /// source line's name/description to the target's `description` so future `Sync`/`Regroup` runs
    /// automatically classify related sessions under the target line.
    pub fn merge_lines(
        &mut self,
        source_line_id: &str,
        target_line_id: &str,
        pin_target: bool,
        append_scope_hint: bool,
    ) -> Result<usize> {
        if source_line_id == target_line_id {
            return Err(PlanarError::InvalidInput(
                "cannot merge a line into itself".into(),
            ));
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let source_row: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT name, description, is_system_bucket FROM lines WHERE id = ?1",
                params![source_line_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((src_name, src_desc, src_is_system)) = source_row else {
            return Err(PlanarError::NotFound(format!("line `{source_line_id}`")));
        };
        if src_is_system != 0 {
            return Err(PlanarError::InvalidInput(
                "cannot merge system bucket line away".into(),
            ));
        }

        let target_row: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT name, description, is_system_bucket FROM lines WHERE id = ?1",
                params![target_line_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((tgt_name, tgt_desc, tgt_is_system)) = target_row else {
            return Err(PlanarError::NotFound(format!("line `{target_line_id}`")));
        };

        let moved = tx.execute(
            "UPDATE sessions SET line_id = ?2 WHERE line_id = ?1",
            params![source_line_id, target_line_id],
        )?;

        if tgt_is_system == 0 {
            if pin_target {
                tx.execute(
                    "UPDATE lines SET pinned = 1 WHERE id = ?1",
                    params![target_line_id],
                )?;
            }
            if append_scope_hint {
                let merged_desc =
                    merge_line_descriptions(&tgt_name, &tgt_desc, &src_name, &src_desc);
                if merged_desc != tgt_desc {
                    tx.execute(
                        "UPDATE lines SET description = ?2 WHERE id = ?1",
                        params![target_line_id, &merged_desc],
                    )?;
                }
            }
        }

        tx.execute("DELETE FROM lines WHERE id = ?1", params![source_line_id])?;
        tx.commit()?;
        Ok(moved)
    }

    /// Assigns (or clears) a session's `line_id` and optionally updates `hidden`.
    pub fn update_session(
        &self,
        session_id: &str,
        line_id: &LineIdUpdate,
        hidden: Option<bool>,
    ) -> Result<()> {
        let exists: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(PlanarError::NotFound(format!("session `{session_id}`")));
        }

        match line_id {
            LineIdUpdate::Unchanged => {}
            LineIdUpdate::Clear => {
                self.conn.execute(
                    "UPDATE sessions SET line_id = NULL WHERE id = ?1",
                    params![session_id],
                )?;
            }
            LineIdUpdate::Set(lid) => {
                let line_exists: Option<String> = self
                    .conn
                    .query_row("SELECT id FROM lines WHERE id = ?1", params![lid], |row| {
                        row.get(0)
                    })
                    .optional()?;
                if line_exists.is_none() {
                    return Err(PlanarError::NotFound(format!("line `{lid}`")));
                }
                self.conn.execute(
                    "UPDATE sessions SET line_id = ?2 WHERE id = ?1",
                    params![session_id, lid],
                )?;
            }
        }

        if let Some(is_hidden) = hidden {
            let val = i64::from(is_hidden);
            self.conn.execute(
                "UPDATE sessions SET hidden = ?2 WHERE id = ?1",
                params![session_id, val],
            )?;
            self.conn.execute(
                "UPDATE session_digests SET hidden = ?2 WHERE session_id = ?1",
                params![session_id, val],
            )?;
        }

        Ok(())
    }

    /// Links `from_session_id` to an upstream `to_session_id`, replacing any prior dependency
    /// from `from_session_id` so the UI dependency tree stays single-parent.
    pub fn upsert_dependency(
        &mut self,
        from_session_id: &str,
        to_session_id: &str,
        kind: &str,
        source: &str,
    ) -> Result<()> {
        if from_session_id == to_session_id {
            return Err(PlanarError::InvalidInput(
                "a session cannot depend on itself".into(),
            ));
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for sid in [from_session_id, to_session_id] {
            let exists: Option<String> = tx
                .query_row("SELECT id FROM sessions WHERE id = ?1", params![sid], |r| {
                    r.get(0)
                })
                .optional()?;
            if exists.is_none() {
                return Err(PlanarError::NotFound(format!("session `{sid}`")));
            }
        }

        tx.execute(
            "DELETE FROM session_dependencies WHERE from_session_id = ?1",
            params![from_session_id],
        )?;
        tx.execute(
            "INSERT INTO session_dependencies (from_session_id, to_session_id, kind, source)
             VALUES (?1, ?2, ?3, ?4)",
            params![from_session_id, to_session_id, kind, source],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Removes outgoing dependencies for `from_session_id`.
    pub fn delete_dependencies_from(&self, from_session_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM session_dependencies WHERE from_session_id = ?1",
            params![from_session_id],
        )?;
        Ok(())
    }

    /// Loads source harness metadata needed to launch or resume `session_id`.
    pub fn get_session_launch_info(&self, session_id: &str) -> Result<SessionLaunchInfo> {
        self.conn
            .query_row(
                "SELECT id, harness, cwd, source_path FROM sessions WHERE id = ?1",
                params![session_id],
                |row| {
                    Ok(SessionLaunchInfo {
                        id: row.get(0)?,
                        harness: row.get(1)?,
                        cwd: row.get(2)?,
                        source_path: row.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| PlanarError::NotFound(format!("session `{session_id}`")))
    }

    /// Resolves an exact session ID or unambiguous prefix/suffix into launch metadata for `planar open`.
    pub fn resolve_session_launch_info(&self, query: &str) -> Result<SessionLaunchInfo> {
        let q = query.trim();
        if q.is_empty() {
            return Err(PlanarError::InvalidInput(
                "session identifier cannot be empty".into(),
            ));
        }
        if let Ok(exact) = self.get_session_launch_info(q) {
            return Ok(exact);
        }

        let all_sessions: Vec<SessionRow> = self
            .load_eligible_sessions(0, Local::now().timestamp_millis())?
            .into_iter()
            .filter(|r| !r.first_user_prompt.contains(INTERNAL_DIGEST_SENTINEL))
            .collect();

        match resolve_session_query(q, &all_sessions) {
            SessionMatchOutcome::ExactOrUnique(idx) => {
                let row = &all_sessions[idx];
                Ok(SessionLaunchInfo {
                    id: row.id.clone(),
                    harness: row.harness.clone(),
                    cwd: row.cwd.clone(),
                    source_path: row.source_path.clone(),
                })
            }
            SessionMatchOutcome::NotFound => Err(PlanarError::NotFound(format!("session `{q}`"))),
            SessionMatchOutcome::Ambiguous(candidates) => Err(PlanarError::InvalidInput(format!(
                "ambiguous session identifier `{q}` matched multiple sessions: {}",
                candidates.join(", ")
            ))),
        }
    }

    /// Builds the complete `PlaneResponse` for `./ui` over a `days` lookback ending at `now`'s local date.
    pub fn build_plane_response(
        &self,
        window_days: usize,
        now: DateTime<Local>,
    ) -> Result<PlaneResponse> {
        self.build_plane_response_with_context(window_days, None, None, OsStr::new(""), now)
    }

    /// Builds `PlaneResponse` for `./ui` over `window_days` (`1..=31`) ending at `anchor_date` (or today),
    /// including 20-week daily contribution buckets and per-harness handler/CLI statuses.
    pub fn build_plane_response_with_context(
        &self,
        window_days: usize,
        anchor_date: Option<NaiveDate>,
        base_roots: Option<&HarnessRoots>,
        hydrated_path: &OsStr,
        now: DateTime<Local>,
    ) -> Result<PlaneResponse> {
        let parts = self.build_plane_core(window_days, anchor_date, true, now)?;
        let contributions = self.compute_contribution_calendar(
            &parts.core.settings,
            &parts.intervals_map,
            &parts.subagent_rollups,
            parts.today,
            parts.offset,
            parts.now_ms,
        )?;
        let harnesses =
            self.collect_harness_statuses(&parts.core.settings, base_roots, hydrated_path)?;

        Ok(PlaneResponse {
            days: parts.core.days,
            anchor_date: parts.core.anchor_date,
            now_hour: parts.core.now_hour,
            tz_label: parts.core.tz_label,
            archived_count: parts.core.archived_count,
            settings: parts.core.settings,
            lines: parts.core.lines,
            unsummarized: parts.core.unsummarized,
            short_sessions: parts.core.short_sessions,
            contributions,
            harnesses,
            last_sync: None,
        })
    }

    /// Builds a lightweight `LitePlaneResponse` over `window_days` ending at `anchor_date` (or today),
    /// skipping the 20-week contribution heatmap, CLI `$PATH` probe, and per-session `source_path` disk
    /// `stat()` checks for fast incremental UI refreshes during streaming `Sync`.
    pub fn build_lite_plane_response(
        &self,
        window_days: usize,
        anchor_date: Option<NaiveDate>,
        now: DateTime<Local>,
    ) -> Result<LitePlaneResponse> {
        Ok(self
            .build_plane_core(window_days, anchor_date, false, now)?
            .core)
    }

    fn build_plane_core(
        &self,
        window_days: usize,
        anchor_date: Option<NaiveDate>,
        check_source_exists: bool,
        now: DateTime<Local>,
    ) -> Result<PlaneCoreParts> {
        let settings = self.get_settings()?;
        let effective_days = window_days.clamp(WINDOW_DAYS_MIN, WINDOW_DAYS_MAX);
        let local_fixed: DateTime<FixedOffset> = now.with_timezone(now.offset());
        let offset = *local_fixed.offset();
        let today = local_fixed.date_naive();
        let end_date = anchor_date.unwrap_or(today).min(today);

        let timeline_days = effective_days;
        let timeline_start =
            end_date - Duration::days(i64::try_from(timeline_days - 1).unwrap_or(0));
        let range_start_ms = local_midnight_ms(timeline_start, offset).unwrap_or(0);
        let now_ms = local_fixed.timestamp_millis();
        let window_end_ms = if end_date == today {
            now_ms.saturating_add(60_000)
        } else {
            local_midnight_ms(end_date + Duration::days(1), offset).unwrap_or(now_ms)
        };

        let days: Vec<DayMeta> = (0..timeline_days)
            .map(|idx| {
                let d = timeline_start + Duration::days(i64::try_from(idx).unwrap_or(0));
                let is_today = d == today;
                DayMeta {
                    idx,
                    iso_date: d.format("%Y-%m-%d").to_string(),
                    short: format!("{} {}", d.format("%a"), d.day()),
                    tiny: d.format("%a").to_string(),
                    full: if is_today {
                        "Today".to_string()
                    } else {
                        d.format("%a, %b %-d").to_string()
                    },
                    is_today,
                }
            })
            .collect();

        let line_rows = self.load_lines()?;
        let session_rows = self.load_eligible_sessions(range_start_ms, now_ms)?;
        let subagent_rollups = self.load_subagent_token_rollups()?;
        let intervals_map = self.load_all_intervals()?;
        let digests_map = self.load_digests()?;
        let deps_map = self.load_dependency_map()?;

        let archived_count: usize = {
            let cnt: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM sessions
                 WHERE is_subagent = 0 AND archived = 1 AND step_count > 0 AND last_active_at >= ?1",
                params![range_start_ms],
                |row| row.get(0),
            )?;
            usize::try_from(cnt.max(0)).unwrap_or(0)
        };

        let mut line_sessions: HashMap<String, Vec<SessionDto>> = HashMap::new();
        let mut unsummarized = Vec::new();
        let mut short_all = Vec::new();

        for row in session_rows {
            if !settings.enabled_harnesses.is_enabled(&row.harness) {
                continue;
            }
            // Summarized/filed sessions always remain in their Line so historical
            // agent time, tokens, cost, and swimlane bands never vanish on archive.
            // Unassigned archived sessions stay out of the active Sync/short queues.
            if row.archived && row.line_id.is_none() && !settings.include_archived {
                continue;
            }

            let intervals = intervals_map.get(&row.id).map_or(&[][..], Vec::as_slice);
            let bursts = project_bursts_into_days(
                intervals,
                &row.prompt_timestamps_ms,
                timeline_start,
                timeline_days,
                offset,
            );
            let range_mins =
                sum_interval_minutes_in_range(intervals, range_start_ms, window_end_ms);
            let running = is_session_running(row.has_open_turn, row.last_active_at, now_ms);
            let recency = format_recency(row.last_active_at, now_ms, running);
            let source_exists = check_source_exists.then(|| Path::new(&row.source_path).exists());
            let (total_tokens, cost_usd) =
                resolve_session_usage(&row, subagent_rollups.get(&row.id));

            if let Some(ref line_id) = row.line_id {
                let digest = digests_map.get(&row.id);
                let hidden = row.hidden || digest.is_some_and(|d| d.hidden);
                let title = digest
                    .map(|d| d.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| non_empty_prompt(&row.first_user_prompt));
                let stopped_at = digest.map_or_default(|d| d.stopped_at.clone());
                let left_open = digest.map_or_else(String::new, |d| d.left_open.clone());

                line_sessions
                    .entry(line_id.clone())
                    .or_default()
                    .push(SessionDto {
                        id: row.id.clone(),
                        harness: row.harness,
                        title,
                        raw_title: non_empty_prompt(&row.first_user_prompt),
                        steps: row.step_count,
                        user_input_count: row.user_input_count,
                        recency,
                        running,
                        stopped_at,
                        left_open,
                        depends_on_id: deps_map.get(&row.id).cloned(),
                        hidden,
                        archived: row.archived,
                        cwd: row.cwd,
                        source_path: row.source_path,
                        source_exists,
                        model: row.model,
                        total_tokens,
                        cost_usd,
                        last_active_at: row.last_active_at,
                        bursts,
                    });
                continue;
            }

            if row.user_input_count == 0 || row.hidden {
                continue;
            }
            if end_date < today && range_mins == 0 && bursts.is_empty() {
                continue;
            }

            if row.user_input_count < u64::from(settings.fewest_user_inputs) {
                let day_idx = bursts
                    .last()
                    .map_or(timeline_days.saturating_sub(1), |b| b.0);
                short_all.push(ShortItemDto {
                    id: row.id,
                    harness: row.harness,
                    prompt: non_empty_prompt(&row.first_user_prompt),
                    recency,
                    running,
                    day_idx,
                    mins: range_mins.max(1),
                    steps: row.step_count,
                    user_input_count: row.user_input_count,
                    cwd: row.cwd,
                    source_path: row.source_path,
                    model: row.model,
                    total_tokens,
                    cost_usd,
                    last_active_at: row.last_active_at,
                    bursts,
                });
            } else {
                unsummarized.push(UnsummarizedDto {
                    id: row.id,
                    harness: row.harness,
                    first_prompt: non_empty_prompt(&row.first_user_prompt),
                    resolved_title: None,
                    target_line_id: None,
                    steps: row.step_count,
                    user_input_count: row.user_input_count,
                    recency,
                    running,
                    archived: row.archived,
                    cwd: row.cwd,
                    source_path: row.source_path,
                    source_exists,
                    model: row.model,
                    total_tokens,
                    cost_usd,
                    last_active_at: row.last_active_at,
                    bursts,
                });
            }
        }

        let lines = line_rows
            .into_iter()
            .map(|l| {
                let mut sessions = line_sessions.remove(&l.id).unwrap_or_default();
                sessions.sort_unstable_by_key(|a| std::cmp::Reverse(a.last_active_at));
                LineDto {
                    id: l.id,
                    name: l.name,
                    description: l.description,
                    color: l.color,
                    pinned: l.pinned,
                    is_system_bucket: l.is_system_bucket,
                    priority: l.priority,
                    status: l.status,
                    sessions,
                }
            })
            .collect();

        let short_total_count = short_all.len();
        let short_week_minutes: i64 = short_all.iter().map(|s| s.mins).sum();
        let short_week_tokens: u64 = short_all.iter().map(|s| s.total_tokens).sum();
        let short_week_cost_usd: f64 = short_all.iter().filter_map(|s| s.cost_usd).sum();
        short_all.truncate(MAX_SHORT_ITEMS_RETURNED);

        let core = LitePlaneResponse {
            is_lite: !check_source_exists,
            days,
            anchor_date: end_date.format("%Y-%m-%d").to_string(),
            now_hour: fractional_hour(local_fixed),
            tz_label: format_tz_label(offset),
            archived_count,
            settings,
            lines,
            unsummarized,
            short_sessions: ShortSessionsDto {
                total_count: short_total_count,
                items: short_all,
                week_minutes: short_week_minutes,
                week_tokens: short_week_tokens,
                week_cost_usd: short_week_cost_usd,
            },
            last_sync: None,
        };

        Ok(PlaneCoreParts {
            core,
            intervals_map,
            subagent_rollups,
            today,
            offset,
            now_ms,
        })
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn compute_contribution_calendar(
        &self,
        settings: &AppSettings,
        intervals_map: &HashMap<String, Vec<RawSpan>>,
        subagent_rollups: &HashMap<String, SubagentRollup>,
        today: NaiveDate,
        offset: FixedOffset,
        now_ms: i64,
    ) -> Result<Vec<ContributionDayDto>> {
        let monday_offset = i64::from(today.weekday().num_days_from_monday());
        let cal_start = today - Duration::days(monday_offset + 19 * 7);
        let cal_start_ms = local_midnight_ms(cal_start, offset).unwrap_or(0);

        let total_days = usize::try_from((today - cal_start).num_days() + 1).unwrap_or(1);
        let mut buckets: Vec<(i64, i64, ContributionDayDto)> = Vec::with_capacity(total_days);
        for d_idx in 0..total_days {
            let d = cal_start + Duration::days(i64::try_from(d_idx).unwrap_or(0));
            let ds = local_midnight_ms(d, offset).unwrap_or(0);
            let de = local_midnight_ms(d + Duration::days(1), offset).unwrap_or(ds);
            buckets.push((
                ds,
                de,
                ContributionDayDto {
                    date: d.format("%Y-%m-%d").to_string(),
                    weekday: u8::try_from(d.weekday().num_days_from_monday()).unwrap_or(0),
                    total_minutes: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                    session_count: 0,
                    by_harness: BTreeMap::new(),
                    tokens_by_harness: BTreeMap::new(),
                    cost_by_harness: BTreeMap::new(),
                },
            ));
        }

        let cal_sessions = self.load_eligible_sessions(cal_start_ms, now_ms)?;
        for row in cal_sessions {
            if !settings.enabled_harnesses.is_enabled(&row.harness) {
                continue;
            }
            if row.hidden || (row.line_id.is_none() && row.user_input_count == 0) {
                continue;
            }
            let Some(intervals) = intervals_map.get(&row.id) else {
                continue;
            };

            let (sess_tokens, sess_cost) =
                resolve_session_usage(&row, subagent_rollups.get(&row.id));
            let mut active_hits: Vec<(usize, i64)> = Vec::new();
            let mut total_sess_mins = 0_i64;
            for (idx, (ds, de, _)) in buckets.iter().enumerate() {
                let mins = sum_interval_minutes_in_range(intervals, *ds, *de);
                if mins > 0 {
                    active_hits.push((idx, mins));
                    total_sess_mins += mins;
                }
            }

            for (idx, mins) in active_hits {
                let dto = &mut buckets[idx].2;
                dto.total_minutes += mins;
                dto.session_count += 1;
                *dto.by_harness.entry(row.harness.clone()).or_insert(0) += mins;

                if sess_tokens > 0 && total_sess_mins > 0 {
                    let share = (mins as f64) / (total_sess_mins as f64);
                    let day_tok = ((sess_tokens as f64) * share).round() as u64;
                    dto.total_tokens = dto.total_tokens.saturating_add(day_tok);
                    *dto.tokens_by_harness
                        .entry(row.harness.clone())
                        .or_insert(0) += day_tok;
                    if let Some(c) = sess_cost {
                        let day_cost = c * share;
                        dto.cost_usd += day_cost;
                        *dto.cost_by_harness
                            .entry(row.harness.clone())
                            .or_insert(0.0) += day_cost;
                    }
                }
            }
        }

        Ok(buckets.into_iter().map(|(_, _, dto)| dto).collect())
    }

    fn collect_harness_statuses(
        &self,
        settings: &AppSettings,
        base_roots: Option<&HarnessRoots>,
        hydrated_path: &OsStr,
    ) -> Result<Vec<HarnessStatusDto>> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT harness, COUNT(*) FROM sessions WHERE is_subagent = 0 AND step_count > 0 GROUP BY harness",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for item in rows {
            let (h, c) = item?;
            counts.insert(h, usize::try_from(c.max(0)).unwrap_or(0));
        }

        let fallback_roots = base_roots.map_or_else(
            || HarnessRoots::from_env_or_home().ok(),
            |r| Some(r.clone()),
        );
        let effective_roots = fallback_roots.map(|r| r.effective_for(settings));

        let order = harness_order();
        let mut out = Vec::with_capacity(order.len());
        for hid in order {
            let hid = hid.as_str();
            let bin_name = cli_binary_name_for_harness(hid).unwrap_or(hid);
            let custom_bin = if settings.cli_harness == hid {
                settings.cli_bin_override.as_str()
            } else {
                ""
            };
            let cli_path = resolve_cli_binary(bin_name, custom_bin, hydrated_path)
                .map(|p| p.to_string_lossy().to_string());
            let log_root = effective_roots
                .as_ref()
                .map(|r| r.primary_root_for(hid))
                .map_or_else(String::new, |p| p.to_string_lossy().to_string());
            let log_root_exists = !log_root.is_empty() && Path::new(&log_root).exists();

            out.push(HarnessStatusDto {
                id: hid.to_string(),
                enabled: settings.enabled_harnesses.is_enabled(hid),
                cli_available: cli_path.is_some(),
                cli_path,
                log_root,
                log_root_exists,
                session_count: counts.get(hid).copied().unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// Loads eligible sessions needing summarization/digest regeneration for `Sync`.
    /// Sessions that already possess a saved digest whose source log file has been pruned
    /// by its Harness are permanently locked and skipped during re-summarization.
    pub fn load_sync_candidates(
        &self,
        settings: &AppSettings,
        current_slice_hash: &str,
        force_all: bool,
    ) -> Result<Vec<SyncCandidate>> {
        self.load_sync_candidates_in_scope(
            settings,
            current_slice_hash,
            force_all,
            Some(
                settings
                    .sync_batch_limit
                    .clamp(SYNC_BATCH_LIMIT_MIN, SYNC_BATCH_LIMIT_MAX),
            ),
            None,
            Local::now(),
        )
    }

    /// Loads eligible sessions needing summarization within an optional active view `SyncScope`
    /// (`days` window, optional historical `anchor_date`, and optional `harness` filter) so
    /// `Sync` only processes non-short sessions belonging to the user's current view.
    pub fn load_sync_candidates_in_scope(
        &self,
        settings: &AppSettings,
        current_slice_hash: &str,
        force_all: bool,
        limit_override: Option<usize>,
        scope: Option<&SyncScope>,
        now: DateTime<Local>,
    ) -> Result<Vec<SyncCandidate>> {
        let min_inputs = i64::from(settings.fewest_user_inputs.max(FEWEST_USER_INPUTS_MIN));
        let local_fixed: DateTime<FixedOffset> = now.with_timezone(now.offset());
        let offset = *local_fixed.offset();
        let today = local_fixed.date_naive();
        let now_ms = local_fixed.timestamp_millis();
        let max_future_ms = now_ms.saturating_add(86_400_000);

        let (range_start_ms, historical_window_end_ms, harness_filter) = match scope {
            Some(sc) => {
                let effective_days = sc.days.clamp(WINDOW_DAYS_MIN, WINDOW_DAYS_MAX);
                let end_date = sc.anchor_date.unwrap_or(today).min(today);
                let timeline_start =
                    end_date - Duration::days(i64::try_from(effective_days - 1).unwrap_or(0));
                let start_ms = local_midnight_ms(timeline_start, offset).unwrap_or(0);
                let hist_end = if end_date < today {
                    Some(local_midnight_ms(end_date + Duration::days(1), offset).unwrap_or(now_ms))
                } else {
                    None
                };
                let hf = sc
                    .harness
                    .as_deref()
                    .map(str::trim)
                    .filter(|h| !h.is_empty() && !h.eq_ignore_ascii_case("all"));
                (start_ms, hist_end, hf)
            }
            None => (0_i64, None, None),
        };

        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.harness, s.source_path, s.step_count, s.user_input_count,
                    s.first_user_prompt, s.cwd, s.line_id,
                    COALESCE(d.last_step_index, -1) AS prev_step,
                    COALESCE(d.slice_config_hash, '') AS prev_hash,
                    s.archived
             FROM sessions s
             LEFT JOIN session_digests d ON d.session_id = s.id
             WHERE s.is_subagent = 0
               AND s.step_count > 0
               AND s.hidden = 0
               AND s.user_input_count >= ?1
               AND s.last_active_at >= ?4
               AND s.last_active_at <= ?5
               AND (
                   ?2 = 1
                   OR s.line_id IS NULL
                   OR d.session_id IS NULL
                   OR d.slice_config_hash != ?3
                   OR s.step_count >= d.last_step_index + 2
               )
             ORDER BY CASE WHEN s.line_id IS NULL THEN 0 ELSE 1 END ASC,
                      s.last_active_at DESC",
        )?;

        let rows = stmt.query_map(
            params![
                min_inputs,
                i64::from(force_all),
                current_slice_hash,
                range_start_ms,
                max_future_ms,
            ],
            |row| {
                Ok((
                    SyncCandidate {
                        id: row.get(0)?,
                        harness: row.get(1)?,
                        source_path: row.get(2)?,
                        step_count: u64::try_from(row.get::<_, i64>(3)?.max(0)).unwrap_or(0),
                        user_input_count: u64::try_from(row.get::<_, i64>(4)?.max(0)).unwrap_or(0),
                        first_user_prompt: row.get(5)?,
                        cwd: row.get(6)?,
                        line_id: row.get(7)?,
                    },
                    row.get::<_, i64>(8)? >= 0,
                    row.get::<_, i64>(10)? != 0,
                ))
            },
        )?;

        let mut out = Vec::new();
        let max_out = limit_override.map_or(usize::MAX, |n| n.max(1));
        for item in rows {
            let (cand, has_digest, archived) = item?;
            if archived && !settings.include_archived {
                continue;
            }
            if !settings.enabled_harnesses.is_enabled(&cand.harness) {
                continue;
            }
            if let Some(target_harness) = harness_filter
                && !cand.harness.eq_ignore_ascii_case(target_harness)
            {
                continue;
            }
            if let Some(win_end_ms) = historical_window_end_ms {
                let has_overlap: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM activity_intervals
                     WHERE session_id = ?1 AND end_ms > ?2 AND start_ms < ?3",
                    params![&cand.id, range_start_ms, win_end_ms],
                    |r| r.get(0),
                )?;
                if has_overlap == 0 {
                    continue;
                }
            }
            // Lock existing AI digests once the external source `.jsonl` file is pruned.
            if has_digest && !Path::new(&cand.source_path).exists() {
                continue;
            }
            out.push(cand);
            if out.len() >= max_out {
                break;
            }
        }
        Ok(out)
    }

    /// Reconciles `user_input_count` when full trajectory slicing reveals fewer non-synthetic
    /// human turns than previously recorded, preventing short sessions from clogging `Sync`.
    pub fn reconcile_user_input_count(&self, session_id: &str, user_count: usize) -> Result<()> {
        let count_i64 = i64::try_from(user_count).unwrap_or(i64::MAX);
        self.conn.execute(
            "UPDATE sessions SET user_input_count = ?2 WHERE id = ?1 AND user_input_count != ?2",
            params![session_id, count_i64],
        )?;
        Ok(())
    }

    /// Lists non-system lines with pinned status, scope description, and visible session counts for prompt context.
    pub fn list_line_catalog(&self) -> Result<Vec<LineCatalogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.id,
                    l.name,
                    l.description,
                    l.pinned,
                    COUNT(s.id) AS session_count
             FROM lines l
             LEFT JOIN sessions s
                    ON s.line_id = l.id
                   AND s.is_subagent = 0
                   AND s.hidden = 0
             WHERE l.is_system_bucket = 0
             GROUP BY l.id
             ORDER BY l.pinned DESC, l.sort_order ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let raw_count = row.get::<_, i64>(4)?.max(0);
            Ok(LineCatalogEntry {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                pinned: row.get::<_, i64>(3)? != 0,
                session_count: usize::try_from(raw_count).unwrap_or(0),
            })
        })?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }

    /// Persists a generated digest for `session_id` and assigns/creates its suggested Line
    /// while respecting user-pinned Lines (`pinned = 1` is never overwritten).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_session_digest(
        &mut self,
        session_id: &str,
        step_count: u64,
        slice_hash: &str,
        title: &str,
        stopped_at: &str,
        left_open: &str,
        suggested_line: &str,
        is_one_off: bool,
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply_session_digest_tx(
            &tx,
            session_id,
            step_count,
            slice_hash,
            title,
            stopped_at,
            left_open,
            suggested_line,
            is_one_off,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Commits a batch of generated session digests inside a single `IMMEDIATE` WAL transaction,
    /// minimizing lock acquisitions under concurrent `Sync` while preserving `pinned = 1` immutability.
    pub fn apply_session_digests_batch(&mut self, writes: &[SessionDigestWrite]) -> Result<()> {
        if writes.is_empty() {
            return Ok(());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for w in writes {
            apply_session_digest_tx(
                &tx,
                &w.session_id,
                w.step_count,
                &w.slice_hash,
                &w.title,
                &w.stopped_at,
                &w.left_open,
                &w.suggested_line,
                w.is_one_off,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Loads sessions belonging to unpinned discovered Lines for `Regroup`.
    pub fn list_unpinned_items_for_regroup(&self) -> Result<Vec<UnpinnedClusterItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id,
                    COALESCE(NULLIF(d.title, ''), NULLIF(s.first_user_prompt, ''), '(untitled)'),
                    COALESCE(d.stopped_at, ''),
                    l.name,
                    s.cwd
             FROM sessions s
             JOIN lines l ON l.id = s.line_id
             LEFT JOIN session_digests d ON d.session_id = s.id
             WHERE l.pinned = 0 AND l.is_system_bucket = 0 AND s.is_subagent = 0 AND s.hidden = 0
             ORDER BY s.last_active_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(UnpinnedClusterItem {
                session_id: row.get(0)?,
                title: row.get(1)?,
                stopped_at: row.get(2)?,
                current_line: row.get(3)?,
                cwd: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }

    /// Applies regrouped `session_id -> line_name` assignments across unpinned lines and prunes empty unpinned lines.
    pub fn apply_regroup_mapping(
        &mut self,
        assignments: &HashMap<String, String>,
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut moved = 0_usize;
        for (sid, line_name) in assignments {
            let is_one_off = line_name.eq_ignore_ascii_case("one-off");
            let target_id = resolve_or_create_line_tx(&tx, line_name, is_one_off)?;
            moved += tx.execute(
                "UPDATE sessions
                 SET line_id = ?2
                 WHERE id = ?1
                   AND (line_id IS NULL OR line_id IN (SELECT id FROM lines WHERE pinned = 0 AND is_system_bucket = 0))",
                params![sid, target_id],
            )?;
        }
        tx.execute(
            "DELETE FROM lines
             WHERE pinned = 0
               AND is_system_bucket = 0
               AND id NOT IN (SELECT DISTINCT line_id FROM sessions WHERE line_id IS NOT NULL)",
            [],
        )?;
        tx.commit()?;
        Ok(moved)
    }

    /// Builds a compact L0 status snapshot for `planar status`.
    pub fn build_status_snapshot(
        &self,
        window_days: usize,
        now: DateTime<Local>,
    ) -> Result<StatusSnapshot> {
        self.build_status_snapshot_with_cwd(window_days, None, now)
    }

    /// Builds a workspace-aware L0 status snapshot, flagging Lines active in `cwd` and surfacing
    /// live `has_open_turn` / running sibling sessions across harnesses.
    pub fn build_status_snapshot_with_cwd(
        &self,
        window_days: usize,
        cwd: Option<&str>,
        now: DateTime<Local>,
    ) -> Result<StatusSnapshot> {
        #[derive(Default, Clone, Copy)]
        struct StatusLineAccum {
            today_minutes: i64,
            window_minutes: i64,
            window_tokens: u64,
            window_cost_usd: f64,
            active_today_sessions: usize,
            total_sessions: usize,
            running_sessions: usize,
            open_followups: usize,
            latest_active_at: i64,
            matches_cwd: bool,
        }

        let settings = self.get_settings()?;
        let effective_days = window_days.clamp(WINDOW_DAYS_MIN, STATUS_WINDOW_DAYS_MAX);
        let normalized_cwd = cwd.and_then(normalize_workspace_path);
        let home_norm =
            dirs::home_dir().and_then(|p| normalize_workspace_path(&p.to_string_lossy()));

        let local_fixed: DateTime<FixedOffset> = now.with_timezone(now.offset());
        let offset = *local_fixed.offset();
        let today = local_fixed.date_naive();
        let today_start_ms = local_midnight_ms(today, offset).unwrap_or(0);
        let range_start = today - Duration::days(i64::try_from(effective_days - 1).unwrap_or(6));
        let range_start_ms = local_midnight_ms(range_start, offset).unwrap_or(0);
        let now_ms = local_fixed.timestamp_millis();
        let range_end_ms = now_ms.saturating_add(60_000);

        let line_rows = self.load_lines()?;
        let line_names: HashMap<String, String> = line_rows
            .iter()
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let session_rows = self.load_eligible_sessions(range_start_ms, now_ms)?;
        let digests_map = self.load_digests()?;
        let subagent_rollups = self.load_subagent_token_rollups()?;
        let intervals_map = self.load_all_intervals()?;

        let mut harness_map: HashMap<String, usize> = HashMap::new();
        let mut line_acc: HashMap<String, StatusLineAccum> = HashMap::new();
        let mut live_candidates: Vec<(i64, LiveSessionSummary)> = Vec::new();
        let mut unsummarized_count = 0_usize;
        let mut unsummarized_today_minutes = 0_i64;
        let mut unsummarized_window_minutes = 0_i64;
        let mut unsummarized_window_tokens = 0_u64;
        let mut unsummarized_window_cost_usd = 0.0_f64;
        let mut top_unsummarized = Vec::new();
        let mut short_count = 0_usize;
        let mut short_today_minutes = 0_i64;
        let mut short_window_minutes = 0_i64;
        let mut short_window_tokens = 0_u64;
        let mut short_window_cost_usd = 0.0_f64;
        let mut total_today_minutes = 0_i64;
        let mut total_window_minutes = 0_i64;
        let mut total_window_tokens = 0_u64;
        let mut total_window_cost_usd = 0.0_f64;

        for row in session_rows {
            if !settings.enabled_harnesses.is_enabled(&row.harness) {
                continue;
            }
            if row.first_user_prompt.contains(INTERNAL_DIGEST_SENTINEL) {
                continue;
            }
            let intervals = intervals_map.get(&row.id).map_or(&[][..], Vec::as_slice);
            let today_mins = sum_interval_minutes_in_range(intervals, today_start_ms, range_end_ms);
            let win_mins = sum_interval_minutes_in_range(intervals, range_start_ms, range_end_ms);

            if row.line_id.is_none() && (row.user_input_count == 0 || row.hidden) {
                continue;
            }

            let running = is_session_running(row.has_open_turn, row.last_active_at, now_ms);
            let matches_cwd = match (&normalized_cwd, row.cwd.as_deref()) {
                (Some(query_cwd), Some(sess_cwd)) => {
                    workspace_paths_match(sess_cwd, query_cwd, home_norm.as_deref())
                }
                _ => false,
            };

            if running && !row.hidden {
                let label = digests_map
                    .get(&row.id)
                    .map(|d| d.title.as_str())
                    .filter(|t| !t.trim().is_empty())
                    .map_or_else(
                        || non_empty_prompt(&row.first_user_prompt),
                        ToString::to_string,
                    );
                let line_name = row
                    .line_id
                    .as_ref()
                    .and_then(|lid| line_names.get(lid).cloned());
                live_candidates.push((
                    row.last_active_at,
                    LiveSessionSummary {
                        id: row.id.clone(),
                        harness: row.harness.clone(),
                        line_id: row.line_id.clone(),
                        line_name,
                        cwd: row.cwd.clone(),
                        matches_cwd,
                        has_open_turn: row.has_open_turn,
                        steps: row.step_count,
                        recency: format_recency(row.last_active_at, now_ms, running),
                        label,
                    },
                ));
            }

            let (sess_tokens, sess_cost) =
                resolve_session_usage(&row, subagent_rollups.get(&row.id));
            let cost_val = sess_cost.unwrap_or(0.0);

            *harness_map.entry(row.harness.clone()).or_insert(0) += 1;
            total_today_minutes += today_mins;
            total_window_minutes += win_mins;
            total_window_tokens = total_window_tokens.saturating_add(sess_tokens);
            total_window_cost_usd += cost_val;

            if let Some(ref lid) = row.line_id {
                let entry = line_acc.entry(lid.clone()).or_default();
                entry.today_minutes += today_mins;
                entry.window_minutes += win_mins;
                entry.window_tokens = entry.window_tokens.saturating_add(sess_tokens);
                entry.window_cost_usd += cost_val;
                if today_mins > 0 {
                    entry.active_today_sessions += 1;
                }
                entry.total_sessions += 1;
                if running {
                    entry.running_sessions += 1;
                }
                if !row.hidden
                    && digests_map
                        .get(&row.id)
                        .is_some_and(|d| !d.hidden && !d.left_open.trim().is_empty())
                {
                    entry.open_followups += 1;
                }
                if row.last_active_at > entry.latest_active_at {
                    entry.latest_active_at = row.last_active_at;
                }
                if matches_cwd {
                    entry.matches_cwd = true;
                }
            } else if row.archived && !settings.include_archived {
                // Counted in global totals above; skip from active Sync/short queues.
            } else if row.user_input_count < u64::from(settings.fewest_user_inputs) {
                short_count += 1;
                short_today_minutes += today_mins;
                short_window_minutes += win_mins;
                short_window_tokens = short_window_tokens.saturating_add(sess_tokens);
                short_window_cost_usd += cost_val;
            } else {
                unsummarized_count += 1;
                unsummarized_today_minutes += today_mins;
                unsummarized_window_minutes += win_mins;
                unsummarized_window_tokens = unsummarized_window_tokens.saturating_add(sess_tokens);
                unsummarized_window_cost_usd += cost_val;
                if top_unsummarized.len() < 5 {
                    top_unsummarized.push(StatusSessionPreview {
                        id: row.id,
                        harness: row.harness,
                        recency: format_recency(row.last_active_at, now_ms, running),
                        running,
                        steps: row.step_count,
                        total_tokens: sess_tokens,
                        cost_usd: sess_cost,
                        prompt: non_empty_prompt(&row.first_user_prompt),
                    });
                }
            }
        }

        // Also check older sessions assigned to Lines for `cwd` affinity when a Line had no
        // recent sessions in `window_days` yet belongs to the caller's repository.
        if let Some(ref query_cwd) = normalized_cwd {
            let mut stmt = self.conn.prepare(
                "SELECT line_id, cwd FROM sessions
                 WHERE is_subagent = 0 AND line_id IS NOT NULL AND cwd IS NOT NULL",
            )?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for item in rows {
                let (lid, sess_cwd) = item?;
                if workspace_paths_match(&sess_cwd, query_cwd, home_norm.as_deref()) {
                    line_acc.entry(lid).or_default().matches_cwd = true;
                }
            }
        }

        live_candidates.sort_unstable_by(|(ts_a, a), (ts_b, b)| {
            b.matches_cwd
                .cmp(&a.matches_cwd)
                .then(b.has_open_turn.cmp(&a.has_open_turn))
                .then(ts_b.cmp(ts_a))
        });
        let live_sessions: Vec<LiveSessionSummary> = live_candidates
            .into_iter()
            .take(8)
            .map(|(_, item)| item)
            .collect();

        let mut harness_counts: Vec<HarnessCount> = harness_map
            .into_iter()
            .map(|(harness, count)| HarnessCount { harness, count })
            .collect();
        harness_counts
            .sort_unstable_by(|a, b| b.count.cmp(&a.count).then(a.harness.cmp(&b.harness)));

        let mut cwd_line_ids = Vec::new();
        let lines: Vec<StatusLineSummary> = line_rows
            .into_iter()
            .filter_map(|l| {
                let acc = line_acc.get(&l.id).copied().unwrap_or_default();
                if l.is_system_bucket && acc.total_sessions == 0 {
                    return None;
                }
                if acc.matches_cwd {
                    cwd_line_ids.push(l.id.clone());
                }
                let last_active_recency = if acc.running_sessions > 0 {
                    "running".to_string()
                } else if acc.latest_active_at > 0 {
                    format_recency(acc.latest_active_at, now_ms, false)
                } else {
                    String::new()
                };
                Some(StatusLineSummary {
                    id: l.id,
                    name: l.name,
                    description: l.description,
                    pinned: l.pinned,
                    priority: l.priority,
                    status: l.status,
                    matches_cwd: acc.matches_cwd,
                    today_minutes: acc.today_minutes,
                    window_minutes: acc.window_minutes,
                    window_tokens: acc.window_tokens,
                    window_cost_usd: acc.window_cost_usd,
                    active_today_sessions: acc.active_today_sessions,
                    running_sessions: acc.running_sessions,
                    total_sessions: acc.total_sessions,
                    open_followups: acc.open_followups,
                    last_active_recency,
                })
            })
            .collect();

        let dependencies = self.load_dependencies_list()?;

        Ok(StatusSnapshot {
            generated_at_local: local_fixed.format("%Y-%m-%d %H:%M").to_string(),
            tz_label: format_tz_label(offset),
            window_days: effective_days,
            cwd: normalized_cwd,
            cwd_line_ids,
            total_today_minutes,
            total_window_minutes,
            total_window_tokens,
            total_window_cost_usd,
            harness_counts,
            lines,
            live_sessions,
            unsummarized_count,
            unsummarized_today_minutes,
            unsummarized_window_minutes,
            unsummarized_window_tokens,
            unsummarized_window_cost_usd,
            top_unsummarized,
            short_count,
            short_today_minutes,
            short_window_minutes,
            short_window_tokens,
            short_window_cost_usd,
            dependencies,
        })
    }

    /// Resolves `1..=8` Line identifiers (by exact `id`, case-insensitive `id`, or case-insensitive
    /// `name`) and returns L1 metadata (`description`, `priority`, `status`, active time, ordered
    /// session digests, and upstream/downstream dependencies) with partial-match tolerance.
    pub fn query_lines(
        &self,
        identifiers: &[String],
        limit_per_line: usize,
        now: DateTime<Local>,
    ) -> Result<BatchLinesResponse> {
        if identifiers.is_empty() || identifiers.len() > MAX_L1_BATCH_LINES {
            return Err(PlanarError::InvalidInput(format!(
                "`lines` must contain between 1 and {MAX_L1_BATCH_LINES} line identifiers"
            )));
        }
        let capped_per_line = limit_per_line.clamp(QUERY_LIMIT_MIN, QUERY_LIMIT_MAX);

        let settings = self.get_settings()?;
        let local_fixed: DateTime<FixedOffset> = now.with_timezone(now.offset());
        let offset = *local_fixed.offset();
        let today = local_fixed.date_naive();
        let today_start_ms = local_midnight_ms(today, offset).unwrap_or(0);
        let week_start_ms = local_midnight_ms(today - Duration::days(6), offset).unwrap_or(0);
        let now_ms = local_fixed.timestamp_millis();
        let range_end_ms = now_ms.saturating_add(60_000);

        let line_rows = self.load_lines()?;
        let mut resolved_indices: Vec<usize> = Vec::new();
        let mut seen_line_ids: HashSet<String> = HashSet::new();
        let mut not_found: Vec<String> = Vec::new();

        for raw in identifiers {
            let q = raw.trim();
            if q.is_empty() {
                return Err(PlanarError::InvalidInput(
                    "line identifier cannot be empty".into(),
                ));
            }
            let found_idx = line_rows
                .iter()
                .position(|l| l.id == q)
                .or_else(|| line_rows.iter().position(|l| l.id.eq_ignore_ascii_case(q)))
                .or_else(|| {
                    line_rows
                        .iter()
                        .position(|l| l.name.eq_ignore_ascii_case(q))
                });
            if let Some(idx) = found_idx {
                if seen_line_ids.insert(line_rows[idx].id.clone()) {
                    resolved_indices.push(idx);
                }
            } else {
                not_found.push(q.to_string());
            }
        }

        let available_lines = if not_found.is_empty() {
            Vec::new()
        } else {
            line_rows
                .iter()
                .filter(|l| !l.is_system_bucket || l.id == ONE_OFF_LINE_ID)
                .map(|l| LineHint {
                    id: l.id.clone(),
                    name: l.name.clone(),
                    priority: l.priority,
                    status: l.status.clone(),
                })
                .collect()
        };

        if resolved_indices.is_empty() {
            return Ok(BatchLinesResponse {
                lines: Vec::new(),
                not_found,
                available_lines,
            });
        }

        let all_sessions = self.load_eligible_sessions(0, now_ms)?;
        let digests_map = self.load_digests()?;
        let intervals_map = self.load_all_intervals()?;
        let subagent_rollups = self.load_subagent_token_rollups()?;
        let dep_edges = self.load_dependencies_list()?;
        let dep_lookup = build_dep_graph_lookup(&all_sessions, &digests_map, &dep_edges, now_ms);

        let mut sessions_by_line: HashMap<String, Vec<SessionRow>> = HashMap::new();
        for row in all_sessions {
            if !settings.enabled_harnesses.is_enabled(&row.harness) {
                continue;
            }
            if row.first_user_prompt.contains(INTERNAL_DIGEST_SENTINEL) {
                continue;
            }
            let hidden = row.hidden || digests_map.get(&row.id).is_some_and(|d| d.hidden);
            if hidden {
                continue;
            }
            if let Some(ref lid) = row.line_id {
                sessions_by_line.entry(lid.clone()).or_default().push(row);
            }
        }

        let mut out_lines = Vec::with_capacity(resolved_indices.len());
        for idx in resolved_indices {
            let l = &line_rows[idx];
            let mut rows = sessions_by_line.remove(&l.id).unwrap_or_default();
            rows.sort_unstable_by_key(|r| std::cmp::Reverse(r.last_active_at));

            let mut today_minutes = 0_i64;
            let mut week_minutes = 0_i64;
            let mut total_minutes = 0_i64;
            let mut total_tokens = 0_u64;
            let mut total_cost_usd = 0.0_f64;
            let mut archived_sessions = 0_usize;
            let mut running_sessions = 0_usize;
            let total_sessions = rows.len();
            let mut session_dtos = Vec::with_capacity(rows.len().min(capped_per_line));

            for (pos, row) in rows.into_iter().enumerate() {
                let intervals = intervals_map.get(&row.id).map_or(&[][..], Vec::as_slice);
                let sess_today =
                    sum_interval_minutes_in_range(intervals, today_start_ms, range_end_ms);
                let sess_week =
                    sum_interval_minutes_in_range(intervals, week_start_ms, range_end_ms);
                let sess_total = sum_interval_minutes_in_range(intervals, 0, range_end_ms);
                let (sess_tok, sess_cost) =
                    resolve_session_usage(&row, subagent_rollups.get(&row.id));
                let running = is_session_running(row.has_open_turn, row.last_active_at, now_ms);

                today_minutes += sess_today;
                week_minutes += sess_week;
                total_minutes += sess_total;
                total_tokens = total_tokens.saturating_add(sess_tok);
                total_cost_usd += sess_cost.unwrap_or(0.0);
                if row.archived {
                    archived_sessions += 1;
                }
                if running {
                    running_sessions += 1;
                }

                if pos < capped_per_line {
                    let digest = digests_map.get(&row.id);
                    let title = digest
                        .map(|d| d.title.as_str())
                        .filter(|t| !t.trim().is_empty())
                        .map_or_else(
                            || non_empty_prompt(&row.first_user_prompt),
                            ToString::to_string,
                        );
                    let stopped_at = digest.map_or_else(String::new, |d| d.stopped_at.clone());
                    let left_open = digest.map_or_else(String::new, |d| d.left_open.clone());
                    let (depends_on, depended_by) = dep_lookup.links_for(&row.id);

                    session_dtos.push(LineSessionDigestDto {
                        id: row.id,
                        harness: row.harness,
                        title,
                        stopped_at,
                        left_open,
                        running,
                        has_open_turn: row.has_open_turn,
                        archived: row.archived,
                        recency: format_recency(row.last_active_at, now_ms, running),
                        last_active_at: row.last_active_at,
                        last_active_local: format_local_ms(row.last_active_at, offset),
                        active_minutes: sess_total,
                        steps: row.step_count,
                        user_input_count: row.user_input_count,
                        total_tokens: sess_tok,
                        cost_usd: sess_cost,
                        cwd: row.cwd,
                        depends_on,
                        depended_by,
                    });
                }
            }

            out_lines.push(LineDetailDto {
                id: l.id.clone(),
                name: l.name.clone(),
                description: l.description.clone(),
                pinned: l.pinned,
                is_system_bucket: l.is_system_bucket,
                priority: l.priority,
                status: l.status.clone(),
                today_minutes,
                week_minutes,
                total_minutes,
                total_tokens,
                total_cost_usd,
                total_sessions,
                returned_sessions: session_dtos.len(),
                archived_sessions,
                running_sessions,
                sessions: session_dtos,
            });
        }

        Ok(BatchLinesResponse {
            lines: out_lines,
            not_found,
            available_lines,
        })
    }

    /// Resolves `1..=3` session identifiers (exact ID or unambiguous prefix/suffix) and returns
    /// tool-stripped Head–Middle–Tail trajectory slices (`focus = Full | Handoff`) plus raw `source_path`.
    pub fn query_sessions(
        &self,
        session_ids: &[String],
        focus: SessionFocus,
        now: DateTime<Local>,
    ) -> Result<BatchSessionsResponse> {
        if session_ids.is_empty() || session_ids.len() > MAX_L2_BATCH_SESSIONS {
            return Err(PlanarError::InvalidInput(format!(
                "`session_ids` must contain between 1 and {MAX_L2_BATCH_SESSIONS} session identifiers"
            )));
        }

        let settings = self.get_settings()?;
        let local_fixed: DateTime<FixedOffset> = now.with_timezone(now.offset());
        let offset = *local_fixed.offset();
        let now_ms = local_fixed.timestamp_millis();
        let range_end_ms = now_ms.saturating_add(60_000);

        let all_sessions: Vec<SessionRow> = self
            .load_eligible_sessions(0, now_ms)?
            .into_iter()
            .filter(|r| !r.first_user_prompt.contains(INTERNAL_DIGEST_SENTINEL))
            .collect();

        let mut resolved_indices: Vec<usize> = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut not_found: Vec<String> = Vec::new();
        let mut ambiguous: Vec<AmbiguousSessionHint> = Vec::new();

        for raw in session_ids {
            let q = raw.trim();
            if q.is_empty() {
                return Err(PlanarError::InvalidInput(
                    "session identifier cannot be empty".into(),
                ));
            }
            match resolve_session_query(q, &all_sessions) {
                SessionMatchOutcome::ExactOrUnique(idx) => {
                    if seen_ids.insert(all_sessions[idx].id.clone()) {
                        resolved_indices.push(idx);
                    }
                }
                SessionMatchOutcome::NotFound => not_found.push(q.to_string()),
                SessionMatchOutcome::Ambiguous(candidates) => {
                    ambiguous.push(AmbiguousSessionHint {
                        query: q.to_string(),
                        candidates,
                    });
                }
            }
        }

        if resolved_indices.is_empty() {
            if !ambiguous.is_empty() {
                let first = &ambiguous[0];
                return Err(PlanarError::InvalidInput(format!(
                    "ambiguous session identifier `{}` matched multiple sessions: {}",
                    first.query,
                    first.candidates.join(", ")
                )));
            }
            return Err(PlanarError::NotFound(format!(
                "session(s) not found: {}",
                not_found.join(", ")
            )));
        }

        let line_rows = self.load_lines()?;
        let line_names: HashMap<String, String> =
            line_rows.into_iter().map(|l| (l.id, l.name)).collect();
        let digests_map = self.load_digests()?;
        let intervals_map = self.load_all_intervals()?;
        let subagent_rollups = self.load_subagent_token_rollups()?;
        let dep_edges = self.load_dependencies_list()?;
        let dep_lookup = build_dep_graph_lookup(&all_sessions, &digests_map, &dep_edges, now_ms);

        let mut out_sessions = Vec::with_capacity(resolved_indices.len());
        for idx in resolved_indices {
            let row = &all_sessions[idx];
            let intervals = intervals_map.get(&row.id).map_or(&[][..], Vec::as_slice);
            let active_minutes = sum_interval_minutes_in_range(intervals, 0, range_end_ms);
            let (total_tokens, cost_usd) =
                resolve_session_usage(row, subagent_rollups.get(&row.id));
            let running = is_session_running(row.has_open_turn, row.last_active_at, now_ms);
            let source_exists =
                !row.source_path.is_empty() && Path::new(&row.source_path).is_file();

            let (extracted_dialogue_steps, head, middle_users, tail) = if source_exists {
                let sliced = crate::processor::slice_trajectory_file(
                    Path::new(&row.source_path),
                    &row.harness,
                    &settings,
                )?;
                let map_steps = |steps: Vec<crate::processor::SlicedStep>| -> Vec<SlicedTurnDto> {
                    steps
                        .into_iter()
                        .map(|s| SlicedTurnDto {
                            index: s.index,
                            role: s.role.to_string(),
                            text: s.text,
                        })
                        .collect()
                };
                let (head_dto, tail_dto) = match focus {
                    SessionFocus::Full => (map_steps(sliced.head), map_steps(sliced.tail)),
                    SessionFocus::Handoff => {
                        // When a session has fewer turns than `opening_steps`, `sliced.tail` is
                        // empty because `Full` avoids duplicating `head` into `tail`. Promote
                        // those turns into `tail` under `Handoff` so short/medium sessions still
                        // surface their closing state while omitting `HEAD`.
                        let closing = if sliced.tail.is_empty() {
                            map_steps(sliced.head)
                        } else {
                            map_steps(sliced.tail)
                        };
                        (Vec::new(), closing)
                    }
                };
                (
                    sliced.total_steps,
                    head_dto,
                    map_steps(sliced.middle_users),
                    tail_dto,
                )
            } else {
                (0, Vec::new(), Vec::new(), Vec::new())
            };

            let digest = digests_map.get(&row.id).map(|d| SessionDigestSummaryDto {
                title: d.title.clone(),
                stopped_at: d.stopped_at.clone(),
                left_open: d.left_open.clone(),
            });
            let (depends_on, depended_by) = dep_lookup.links_for(&row.id);
            let line_name = row
                .line_id
                .as_ref()
                .and_then(|lid| line_names.get(lid).cloned());

            out_sessions.push(SessionSliceDto {
                id: row.id.clone(),
                harness: row.harness.clone(),
                line_id: row.line_id.clone(),
                line_name,
                cwd: row.cwd.clone(),
                source_path: row.source_path.clone(),
                source_exists,
                focus,
                running,
                has_open_turn: row.has_open_turn,
                archived: row.archived,
                recency: format_recency(row.last_active_at, now_ms, running),
                last_active_local: format_local_ms(row.last_active_at, offset),
                active_minutes,
                steps: row.step_count,
                extracted_dialogue_steps,
                user_input_count: row.user_input_count,
                total_tokens,
                cost_usd,
                first_user_prompt: non_empty_prompt(&row.first_user_prompt),
                digest,
                depends_on,
                depended_by,
                head,
                middle_users,
                tail,
            });
        }

        Ok(BatchSessionsResponse {
            focus,
            sessions: out_sessions,
            not_found,
            ambiguous,
        })
    }

    /// Searches session digests (`title`, `stopped_at`, `left_open`) and `first_user_prompt`
    /// using the local SQLite `sessions_fts` (`FTS5` + `bm25`) index with CJK/substring fallback.
    pub fn search_sessions(
        &self,
        req: &SearchQuery,
        now: DateTime<Local>,
    ) -> Result<SearchResponse> {
        let raw_query = req.query.trim();
        if raw_query.is_empty() {
            return Err(PlanarError::InvalidInput("`query` cannot be empty".into()));
        }
        let limit = req.limit.clamp(QUERY_LIMIT_MIN, QUERY_LIMIT_MAX);

        let settings = self.get_settings()?;
        let local_fixed: DateTime<FixedOffset> = now.with_timezone(now.offset());
        let offset = *local_fixed.offset();
        let today = local_fixed.date_naive();
        let now_ms = local_fixed.timestamp_millis();
        let range_end_ms = now_ms.saturating_add(60_000);

        let effective_days = req.days.map(|d| d.clamp(WINDOW_DAYS_MIN, SEARCH_DAYS_MAX));
        let range_start_ms = effective_days.map_or(0_i64, |days| {
            let start_date = today - Duration::days(i64::try_from(days - 1).unwrap_or(0));
            local_midnight_ms(start_date, offset).unwrap_or(0)
        });

        let line_rows = self.load_lines()?;
        let resolved_line_id: Option<String> =
            match req.line.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(line_q) => {
                    let found = line_rows
                        .iter()
                        .find(|l| l.id == line_q)
                        .or_else(|| line_rows.iter().find(|l| l.id.eq_ignore_ascii_case(line_q)))
                        .or_else(|| {
                            line_rows
                                .iter()
                                .find(|l| l.name.eq_ignore_ascii_case(line_q))
                        });
                    let Some(matched) = found else {
                        return Err(PlanarError::NotFound(format!("line `{line_q}`")));
                    };
                    Some(matched.id.clone())
                }
                None => None,
            };
        let line_names: HashMap<String, String> =
            line_rows.into_iter().map(|l| (l.id, l.name)).collect();

        let tokens = extract_fts_tokens(raw_query);
        let mut matched_ids: Vec<String> = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();

        if !tokens.is_empty() {
            let and_expr = build_fts_match_expr(&tokens, " AND ");
            self.collect_fts_candidate_ids(
                &and_expr,
                range_start_ms,
                resolved_line_id.as_deref(),
                &settings,
                limit,
                &mut seen_ids,
                &mut matched_ids,
            )?;

            if matched_ids.is_empty() && tokens.len() >= 2 {
                let or_expr = build_fts_match_expr(&tokens, " OR ");
                self.collect_fts_candidate_ids(
                    &or_expr,
                    range_start_ms,
                    resolved_line_id.as_deref(),
                    &settings,
                    limit,
                    &mut seen_ids,
                    &mut matched_ids,
                )?;
            }
        }

        // Supplement with UTF-8 / CJK / punctuation-tolerant substring matching when needed.
        if matched_ids.len() < limit {
            self.collect_substring_candidate_ids(
                raw_query,
                range_start_ms,
                resolved_line_id.as_deref(),
                &settings,
                limit,
                &mut seen_ids,
                &mut matched_ids,
            )?;
        }

        if matched_ids.is_empty() {
            return Ok(SearchResponse {
                query: raw_query.to_string(),
                line_filter: resolved_line_id,
                days: effective_days,
                hits: Vec::new(),
            });
        }

        let all_sessions = self.load_eligible_sessions(range_start_ms, now_ms)?;
        let sess_by_id: HashMap<String, SessionRow> = all_sessions
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        let digests_map = self.load_digests()?;
        let intervals_map = self.load_all_intervals()?;
        let subagent_rollups = self.load_subagent_token_rollups()?;

        let mut hits = Vec::with_capacity(matched_ids.len());
        for sid in matched_ids {
            let Some(row) = sess_by_id.get(&sid) else {
                continue;
            };
            let intervals = intervals_map.get(&row.id).map_or(&[][..], Vec::as_slice);
            let active_minutes = sum_interval_minutes_in_range(intervals, 0, range_end_ms);
            let (total_tokens, cost_usd) =
                resolve_session_usage(row, subagent_rollups.get(&row.id));
            let running = is_session_running(row.has_open_turn, row.last_active_at, now_ms);
            let digest = digests_map.get(&row.id);
            let title = digest
                .map(|d| d.title.as_str())
                .filter(|t| !t.trim().is_empty())
                .map_or_else(
                    || non_empty_prompt(&row.first_user_prompt),
                    ToString::to_string,
                );
            let stopped_at = digest.map_or_else(String::new, |d| d.stopped_at.clone());
            let left_open = digest.map_or_else(String::new, |d| d.left_open.clone());
            let line_name = row
                .line_id
                .as_ref()
                .and_then(|lid| line_names.get(lid).cloned());

            hits.push(SearchHitDto {
                session_id: row.id.clone(),
                harness: row.harness.clone(),
                line_id: row.line_id.clone(),
                line_name,
                title,
                stopped_at,
                left_open,
                first_user_prompt: non_empty_prompt(&row.first_user_prompt),
                running,
                archived: row.archived,
                recency: format_recency(row.last_active_at, now_ms, running),
                last_active_local: format_local_ms(row.last_active_at, offset),
                active_minutes,
                steps: row.step_count,
                total_tokens,
                cost_usd,
                cwd: row.cwd.clone(),
            });
            if hits.len() >= limit {
                break;
            }
        }

        Ok(SearchResponse {
            query: raw_query.to_string(),
            line_filter: resolved_line_id,
            days: effective_days,
            hits,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_fts_candidate_ids(
        &self,
        match_expr: &str,
        range_start_ms: i64,
        line_id: Option<&str>,
        settings: &AppSettings,
        limit: usize,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.harness, s.first_user_prompt, COALESCE(d.hidden, 0)
             FROM sessions_fts
             JOIN sessions s ON s.rowid = sessions_fts.rowid
             LEFT JOIN session_digests d ON d.session_id = s.id
             WHERE sessions_fts MATCH ?1
               AND s.is_subagent = 0
               AND s.step_count > 0
               AND s.hidden = 0
               AND s.last_active_at >= ?2
               AND (?3 IS NULL OR s.line_id = ?3)
             ORDER BY bm25(sessions_fts, 0.0, 5.0, 2.5, 2.5, 1.5) ASC,
                      s.last_active_at DESC
             LIMIT ?4",
        )?;
        let fetch_cap = i64::try_from(limit.saturating_mul(3).max(20)).unwrap_or(100);
        let rows = stmt.query_map(
            params![match_expr, range_start_ms, line_id, fetch_cap],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? != 0,
                ))
            },
        )?;

        for item in rows {
            let (sid, harness, first_prompt, digest_hidden) = item?;
            if digest_hidden
                || !settings.enabled_harnesses.is_enabled(&harness)
                || first_prompt.contains(INTERNAL_DIGEST_SENTINEL)
            {
                continue;
            }
            if seen.insert(sid.clone()) {
                out.push(sid);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_substring_candidate_ids(
        &self,
        raw_query: &str,
        range_start_ms: i64,
        line_id: Option<&str>,
        settings: &AppSettings,
        limit: usize,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.harness, s.first_user_prompt,
                    COALESCE(d.title, ''), COALESCE(d.stopped_at, ''), COALESCE(d.left_open, ''),
                    COALESCE(d.hidden, 0)
             FROM sessions s
             LEFT JOIN session_digests d ON d.session_id = s.id
             WHERE s.is_subagent = 0
               AND s.step_count > 0
               AND s.hidden = 0
               AND s.last_active_at >= ?1
               AND (?2 IS NULL OR s.line_id = ?2)
               AND (
                   INSTR(LOWER(COALESCE(d.title, '')), LOWER(?3)) > 0
                   OR INSTR(LOWER(COALESCE(d.stopped_at, '')), LOWER(?3)) > 0
                   OR INSTR(LOWER(COALESCE(d.left_open, '')), LOWER(?3)) > 0
                   OR INSTR(LOWER(s.first_user_prompt), LOWER(?3)) > 0
               )
             ORDER BY CASE WHEN INSTR(LOWER(COALESCE(d.title, '')), LOWER(?3)) > 0 THEN 0 ELSE 1 END ASC,
                      s.last_active_at DESC
             LIMIT ?4",
        )?;
        let fetch_cap = i64::try_from(limit.saturating_mul(3).max(20)).unwrap_or(100);
        let rows = stmt.query_map(
            params![range_start_ms, line_id, raw_query, fetch_cap],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(6)? != 0,
                ))
            },
        )?;

        for item in rows {
            let (sid, harness, first_prompt, digest_hidden) = item?;
            if digest_hidden
                || !settings.enabled_harnesses.is_enabled(&harness)
                || first_prompt.contains(INTERNAL_DIGEST_SENTINEL)
            {
                continue;
            }
            if seen.insert(sid.clone()) {
                out.push(sid);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(())
    }

    fn load_lines(&self) -> Result<Vec<LineRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, color, pinned, is_system_bucket, priority, status
             FROM lines
             ORDER BY is_system_bucket ASC,
                      pinned DESC,
                      CASE WHEN priority = 0 THEN 99 ELSE priority END ASC,
                      sort_order ASC,
                      name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LineRow {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                color: row.get(3)?,
                pinned: row.get::<_, i64>(4)? != 0,
                is_system_bucket: row.get::<_, i64>(5)? != 0,
                priority: u8::try_from(
                    row.get::<_, i64>(6)?
                        .clamp(LINE_PRIORITY_MIN, LINE_PRIORITY_MAX),
                )
                .unwrap_or(0),
                status: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }

    fn load_eligible_sessions(&self, range_start_ms: i64, now_ms: i64) -> Result<Vec<SessionRow>> {
        let max_future_ms = now_ms.saturating_add(86_400_000);
        let mut stmt = self.conn.prepare(
            "SELECT id, harness, source_path, cwd, step_count, user_input_count,
                    first_user_prompt, user_prompt_timestamps_json, archived, hidden,
                    has_open_turn, line_id, last_active_at,
                    model, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, total_tokens
             FROM sessions
             WHERE is_subagent = 0
               AND step_count > 0
               AND last_active_at >= ?1
               AND last_active_at <= ?2
             ORDER BY last_active_at DESC",
        )?;
        let rows = stmt.query_map(params![range_start_ms, max_future_ms], |row| {
            let prompts_json: String = row.get(7)?;
            let prompt_timestamps_ms: Vec<i64> =
                serde_json::from_str(&prompts_json).unwrap_or_default();
            Ok(SessionRow {
                id: row.get(0)?,
                harness: row.get(1)?,
                source_path: row.get(2)?,
                cwd: row.get(3)?,
                step_count: u64::try_from(row.get::<_, i64>(4)?.max(0)).unwrap_or(0),
                user_input_count: u64::try_from(row.get::<_, i64>(5)?.max(0)).unwrap_or(0),
                first_user_prompt: row.get(6)?,
                prompt_timestamps_ms,
                archived: row.get::<_, i64>(8)? != 0,
                hidden: row.get::<_, i64>(9)? != 0,
                has_open_turn: row.get::<_, i64>(10)? != 0,
                line_id: row.get(11)?,
                last_active_at: row.get(12)?,
                model: row.get(13)?,
                input_tokens: u64::try_from(row.get::<_, i64>(14)?.max(0)).unwrap_or(0),
                cache_read_tokens: u64::try_from(row.get::<_, i64>(15)?.max(0)).unwrap_or(0),
                cache_write_tokens: u64::try_from(row.get::<_, i64>(16)?.max(0)).unwrap_or(0),
                output_tokens: u64::try_from(row.get::<_, i64>(17)?.max(0)).unwrap_or(0),
                total_tokens: u64::try_from(row.get::<_, i64>(18)?.max(0)).unwrap_or(0),
            })
        })?;

        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }

    fn load_subagent_token_rollups(&self) -> Result<HashMap<String, SubagentRollup>> {
        let mut stmt = self.conn.prepare(
            "SELECT parent_session_id, model, input_tokens, cache_read_tokens,
                    cache_write_tokens, output_tokens, total_tokens
             FROM sessions
             WHERE is_subagent = 1
               AND parent_session_id IS NOT NULL
               AND total_tokens > 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                u64::try_from(row.get::<_, i64>(2)?.max(0)).unwrap_or(0),
                u64::try_from(row.get::<_, i64>(3)?.max(0)).unwrap_or(0),
                u64::try_from(row.get::<_, i64>(4)?.max(0)).unwrap_or(0),
                u64::try_from(row.get::<_, i64>(5)?.max(0)).unwrap_or(0),
                u64::try_from(row.get::<_, i64>(6)?.max(0)).unwrap_or(0),
            ))
        })?;

        let mut map: HashMap<String, SubagentRollup> = HashMap::new();
        for item in rows {
            let (pid, model, in_t, cr_t, cw_t, out_t, tot_t) = item?;
            let entry = map.entry(pid).or_default();
            entry.total_tokens = entry.total_tokens.saturating_add(tot_t);
            if let Some(c) = estimate_cost_usd(model.as_deref(), in_t, cr_t, cw_t, out_t) {
                entry.cost_usd += c;
                entry.has_cost = true;
            }
        }
        Ok(map)
    }

    fn load_all_intervals(&self) -> Result<HashMap<String, Vec<RawSpan>>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, start_ms, end_ms
             FROM activity_intervals
             ORDER BY session_id ASC, start_ms ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                RawSpan::clamped(row.get(1)?, row.get(2)?),
            ))
        })?;
        let mut map: HashMap<String, Vec<RawSpan>> = HashMap::new();
        for item in rows {
            let (sid, span) = item?;
            map.entry(sid).or_default().push(span);
        }
        Ok(map)
    }

    fn load_digests(&self) -> Result<HashMap<String, DigestRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, title, stopped_at, left_open, hidden FROM session_digests",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                DigestRow {
                    title: row.get(1)?,
                    stopped_at: row.get(2)?,
                    left_open: row.get(3)?,
                    hidden: row.get::<_, i64>(4)? != 0,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for item in rows {
            let (sid, d) = item?;
            map.insert(sid, d);
        }
        Ok(map)
    }

    fn load_dependency_map(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_session_id, to_session_id FROM session_dependencies")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for item in rows {
            let (from_id, to_id) = item?;
            map.insert(from_id, to_id);
        }
        Ok(map)
    }

    fn load_dependencies_list(&self) -> Result<Vec<StatusDependency>> {
        let mut stmt = self.conn.prepare(
            "SELECT from_session_id, to_session_id, kind, source
             FROM session_dependencies
             ORDER BY from_session_id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StatusDependency {
                from_session_id: row.get(0)?,
                to_session_id: row.get(1)?,
                kind: row.get(2)?,
                source: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }
}

/// Active view filter scoping `Sync` to the displayed window and optional harness lens.
#[derive(Debug, Clone, Default)]
pub struct SyncScope {
    pub days: usize,
    pub anchor_date: Option<NaiveDate>,
    pub harness: Option<String>,
}

/// Session record eligible for trajectory slicing and LLM digest generation.
#[derive(Debug, Clone)]
pub struct SyncCandidate {
    pub id: String,
    pub harness: String,
    pub source_path: String,
    pub step_count: u64,
    pub user_input_count: u64,
    pub first_user_prompt: String,
    pub cwd: Option<String>,
    pub line_id: Option<String>,
}

/// Non-system Line summary passed as world-state context into `Sync` and `Regroup` prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub pinned: bool,
    pub session_count: usize,
}

/// Unpinned session item passed into AI `Regroup` clustering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpinnedClusterItem {
    pub session_id: String,
    pub title: String,
    pub stopped_at: String,
    pub current_line: String,
    pub cwd: Option<String>,
}

fn load_line_ids_conn(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM lines")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut set = HashSet::new();
    for item in rows {
        set.insert(item?);
    }
    Ok(set)
}

fn insert_line_conn(conn: &Connection, name: &str, pinned: bool) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(PlanarError::InvalidInput(
            "line name cannot be empty".into(),
        ));
    }

    let base_slug = slugify_line_name(trimmed);
    let existing_ids = load_line_ids_conn(conn)?;
    let mut candidate = base_slug.clone();
    let mut suffix = 2_usize;
    while existing_ids.contains(&candidate) || candidate.starts_with("__") {
        candidate = format!("{base_slug}-{suffix}");
        suffix += 1;
    }

    let non_system_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lines WHERE is_system_bucket = 0",
        [],
        |row| row.get(0),
    )?;
    let max_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) FROM lines WHERE is_system_bucket = 0",
        [],
        |row| row.get(0),
    )?;

    let color = palette_color(usize::try_from(non_system_count.max(0)).unwrap_or(0));

    conn.execute(
        "INSERT INTO lines (id, name, color, pinned, is_system_bucket, priority, status, sort_order)
         VALUES (?1, ?2, ?3, ?4, 0, 0, 'active', ?5)",
        params![&candidate, trimmed, color, i64::from(pinned), max_order + 1],
    )?;

    Ok(candidate)
}

/// Owned digest payload passed to `Store::apply_session_digests_batch`.
#[derive(Debug, Clone)]
pub struct SessionDigestWrite {
    pub session_id: String,
    pub step_count: u64,
    pub slice_hash: String,
    pub title: String,
    pub stopped_at: String,
    pub left_open: String,
    pub suggested_line: String,
    pub is_one_off: bool,
}

#[allow(clippy::too_many_arguments)]
fn apply_session_digest_tx(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    step_count: u64,
    slice_hash: &str,
    title: &str,
    stopped_at: &str,
    left_open: &str,
    suggested_line: &str,
    is_one_off: bool,
) -> Result<()> {
    tx.execute(
        "INSERT INTO session_digests (session_id, last_step_index, slice_config_hash, title, stopped_at, left_open, hidden)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
         ON CONFLICT(session_id) DO UPDATE SET
            last_step_index = excluded.last_step_index,
            slice_config_hash = excluded.slice_config_hash,
            title = excluded.title,
            stopped_at = excluded.stopped_at,
            left_open = excluded.left_open",
        params![
            session_id,
            i64::try_from(step_count).unwrap_or(i64::MAX),
            slice_hash,
            title.trim(),
            stopped_at.trim(),
            left_open.trim(),
        ],
    )?;

    let existing_assignment: Option<(Option<String>, Option<i64>)> = tx
        .query_row(
            "SELECT s.line_id, l.pinned
             FROM sessions s
             LEFT JOIN lines l ON l.id = s.line_id
             WHERE s.id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((cur_line_id, cur_pinned)) = existing_assignment {
        let can_assign = cur_line_id.is_none() || cur_pinned == Some(0);
        if can_assign {
            let target_line_id = resolve_or_create_line_tx(tx, suggested_line, is_one_off)?;
            tx.execute(
                "UPDATE sessions SET line_id = ?2 WHERE id = ?1",
                params![session_id, target_line_id],
            )?;
        }
    }

    sync_session_fts_conn(tx, session_id)?;
    Ok(())
}

fn resolve_or_create_line_tx(
    tx: &rusqlite::Transaction<'_>,
    line_name: &str,
    is_one_off: bool,
) -> Result<String> {
    let trimmed = line_name.trim();
    if is_one_off || trimmed.is_empty() || trimmed.eq_ignore_ascii_case("one-off") {
        return Ok(ONE_OFF_LINE_ID.to_string());
    }

    if let Some(existing_id) = tx
        .query_row(
            "SELECT id FROM lines WHERE is_system_bucket = 0 AND LOWER(name) = LOWER(?1) LIMIT 1",
            params![trimmed],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(existing_id);
    }

    insert_line_conn(tx, trimmed, false)
}

struct PlaneCoreParts {
    core: LitePlaneResponse,
    intervals_map: HashMap<String, Vec<RawSpan>>,
    subagent_rollups: HashMap<String, SubagentRollup>,
    today: NaiveDate,
    offset: FixedOffset,
    now_ms: i64,
}

struct LineRow {
    id: String,
    name: String,
    description: String,
    color: String,
    pinned: bool,
    is_system_bucket: bool,
    priority: u8,
    status: String,
}

fn sanitize_line_description(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(400).collect()
}

fn merge_line_descriptions(
    target_name: &str,
    target_desc: &str,
    source_name: &str,
    source_desc: &str,
) -> String {
    let cur = sanitize_line_description(target_desc);
    let src_name = source_name.trim();
    let src_desc = sanitize_line_description(source_desc);

    let mut additions: Vec<String> = Vec::new();
    let haystack_lower = format!(
        "{} {}",
        target_name.to_ascii_lowercase(),
        cur.to_ascii_lowercase()
    );
    if !src_name.is_empty() && !haystack_lower.contains(&src_name.to_ascii_lowercase()) {
        additions.push(src_name.to_string());
    }
    if !src_desc.is_empty()
        && !cur
            .to_ascii_lowercase()
            .contains(&src_desc.to_ascii_lowercase())
    {
        additions.push(src_desc);
    }

    if additions.is_empty() {
        return cur;
    }

    let suffix = additions.join("; ");
    let combined = if cur.is_empty() {
        format!("Includes {suffix}")
    } else {
        format!("{cur}; {suffix}")
    };
    sanitize_line_description(&combined)
}

struct SessionRow {
    id: String,
    harness: String,
    source_path: String,
    cwd: Option<String>,
    step_count: u64,
    user_input_count: u64,
    first_user_prompt: String,
    prompt_timestamps_ms: Vec<i64>,
    archived: bool,
    hidden: bool,
    has_open_turn: bool,
    line_id: Option<String>,
    last_active_at: i64,
    model: Option<String>,
    input_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

#[derive(Default)]
struct SubagentRollup {
    total_tokens: u64,
    cost_usd: f64,
    has_cost: bool,
}

fn resolve_session_usage(row: &SessionRow, sub: Option<&SubagentRollup>) -> (u64, Option<f64>) {
    let own_cost = estimate_cost_usd(
        row.model.as_deref(),
        row.input_tokens,
        row.cache_read_tokens,
        row.cache_write_tokens,
        row.output_tokens,
    );
    let total_tokens = row
        .total_tokens
        .saturating_add(sub.map_or(0, |s| s.total_tokens));
    let combined_cost = match (own_cost, sub) {
        (Some(c), Some(s)) if s.has_cost => Some(c + s.cost_usd),
        (Some(c), _) => Some(c),
        (None, Some(s)) if s.has_cost => Some(s.cost_usd),
        _ => None,
    };
    (total_tokens, combined_cost)
}

struct DigestRow {
    title: String,
    stopped_at: String,
    left_open: String,
    hidden: bool,
}

fn load_intervals_tx(tx: &rusqlite::Transaction<'_>, session_id: &str) -> Result<Vec<RawSpan>> {
    let mut stmt = tx.prepare(
        "SELECT start_ms, end_ms FROM activity_intervals WHERE session_id = ?1 ORDER BY start_ms ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(RawSpan::clamped(row.get(0)?, row.get(1)?))
    })?;
    let mut spans = Vec::new();
    for item in rows {
        spans.push(item?);
    }
    Ok(spans)
}

fn min_positive(a: i64, b: i64) -> i64 {
    match (a > 0, b > 0) {
        (true, true) => a.min(b),
        (true, false) => a,
        (false, true) => b,
        (false, false) => 0,
    }
}

fn is_session_running(has_open_turn: bool, last_active_at: i64, now_ms: i64) -> bool {
    let age_ms = now_ms.saturating_sub(last_active_at);
    age_ms <= 90_000 || (has_open_turn && age_ms <= 15 * 60_000)
}

fn format_recency(last_active_at: i64, now_ms: i64, running: bool) -> String {
    if running {
        return "running".to_string();
    }
    let age_secs = (now_ms.saturating_sub(last_active_at) / 1000).max(0);
    if age_secs < 60 {
        return "just now".to_string();
    }
    let mins = age_secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    format!("{days}d ago")
}

fn non_empty_prompt(raw: &str) -> String {
    if raw.trim().is_empty() {
        "(untitled session)".to_string()
    } else {
        raw.to_string()
    }
}

fn slugify_line_name(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() || slug == ONE_OFF_LINE_ID {
        "line".to_string()
    } else {
        slug
    }
}

fn sync_session_fts_conn(conn: &Connection, session_id: &str) -> Result<()> {
    let row: Option<(i64, i64, i64, String, String, String, String)> = conn
        .query_row(
            "SELECT s.rowid,
                    s.is_subagent,
                    s.step_count,
                    s.first_user_prompt,
                    COALESCE(d.title, ''),
                    COALESCE(d.stopped_at, ''),
                    COALESCE(d.left_open, '')
             FROM sessions s
             LEFT JOIN session_digests d ON d.session_id = s.id
             WHERE s.id = ?1",
            params![session_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?;

    let Some((rowid, is_subagent, step_count, first_prompt, title, stopped_at, left_open)) = row
    else {
        return Ok(());
    };

    conn.execute("DELETE FROM sessions_fts WHERE rowid = ?1", params![rowid])?;
    if is_subagent == 0 && step_count > 0 && !first_prompt.contains(INTERNAL_DIGEST_SENTINEL) {
        conn.execute(
            "INSERT INTO sessions_fts (rowid, session_id, title, stopped_at, left_open, first_user_prompt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![rowid, session_id, title, stopped_at, left_open, first_prompt],
        )?;
    }
    Ok(())
}

fn normalize_workspace_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = if let Some(rest) = trimmed.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        home.join(rest).to_string_lossy().into_owned()
    } else if trimmed == "~"
        && let Some(home) = dirs::home_dir()
    {
        home.to_string_lossy().into_owned()
    } else {
        trimmed.to_string()
    };

    let clean = if expanded.len() > 1 {
        expanded.trim_end_matches('/').to_string()
    } else {
        expanded
    };
    (!clean.is_empty()).then_some(clean)
}

fn workspace_paths_match(
    raw_sess_cwd: &str,
    norm_query_cwd: &str,
    norm_home: Option<&str>,
) -> bool {
    let Some(sess_cwd) = normalize_workspace_path(raw_sess_cwd) else {
        return false;
    };
    if sess_cwd == norm_query_cwd {
        return true;
    }
    let is_broad_root = |p: &str| p == "/" || norm_home == Some(p);
    if is_broad_root(&sess_cwd) || is_broad_root(norm_query_cwd) {
        return false;
    }
    is_subpath_of(&sess_cwd, norm_query_cwd) || is_subpath_of(norm_query_cwd, &sess_cwd)
}

fn is_subpath_of(child: &str, parent: &str) -> bool {
    child.len() > parent.len()
        && child.starts_with(parent)
        && child.as_bytes().get(parent.len()) == Some(&b'/')
}

fn format_local_ms(ts_ms: i64, offset: FixedOffset) -> String {
    if ts_ms <= 0 {
        return "(unknown)".to_string();
    }
    DateTime::from_timestamp_millis(ts_ms).map_or_else(
        || "(unknown)".to_string(),
        |utc| {
            utc.with_timezone(&offset)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

struct DepGraphLookup {
    outgoing: HashMap<String, Vec<SessionDepLink>>,
    incoming: HashMap<String, Vec<SessionDepLink>>,
}

impl DepGraphLookup {
    fn links_for(&self, session_id: &str) -> (Vec<SessionDepLink>, Vec<SessionDepLink>) {
        (
            self.outgoing.get(session_id).cloned().unwrap_or_default(),
            self.incoming.get(session_id).cloned().unwrap_or_default(),
        )
    }
}

fn build_dep_graph_lookup(
    sessions: &[SessionRow],
    digests: &HashMap<String, DigestRow>,
    edges: &[StatusDependency],
    now_ms: i64,
) -> DepGraphLookup {
    let mut meta: HashMap<&str, (String, Option<String>, bool)> =
        HashMap::with_capacity(sessions.len());
    for s in sessions {
        let title = digests
            .get(&s.id)
            .map(|d| d.title.as_str())
            .filter(|t| !t.trim().is_empty())
            .map_or_else(
                || non_empty_prompt(&s.first_user_prompt),
                ToString::to_string,
            );
        let running = is_session_running(s.has_open_turn, s.last_active_at, now_ms);
        meta.insert(s.id.as_str(), (title, s.line_id.clone(), running));
    }

    let mut outgoing: HashMap<String, Vec<SessionDepLink>> = HashMap::new();
    let mut incoming: HashMap<String, Vec<SessionDepLink>> = HashMap::new();

    for edge in edges {
        let (up_title, up_line, up_running) = meta
            .get(edge.to_session_id.as_str())
            .cloned()
            .unwrap_or_else(|| (edge.to_session_id.clone(), None, false));
        outgoing
            .entry(edge.from_session_id.clone())
            .or_default()
            .push(SessionDepLink {
                session_id: edge.to_session_id.clone(),
                kind: edge.kind.clone(),
                source: edge.source.clone(),
                title: up_title,
                line_id: up_line,
                running: up_running,
            });

        let (down_title, down_line, down_running) = meta
            .get(edge.from_session_id.as_str())
            .cloned()
            .unwrap_or_else(|| (edge.from_session_id.clone(), None, false));
        incoming
            .entry(edge.to_session_id.clone())
            .or_default()
            .push(SessionDepLink {
                session_id: edge.from_session_id.clone(),
                kind: edge.kind.clone(),
                source: edge.source.clone(),
                title: down_title,
                line_id: down_line,
                running: down_running,
            });
    }

    DepGraphLookup { outgoing, incoming }
}

enum SessionMatchOutcome {
    ExactOrUnique(usize),
    NotFound,
    Ambiguous(Vec<String>),
}

fn resolve_session_query(query: &str, sessions: &[SessionRow]) -> SessionMatchOutcome {
    if let Some(idx) = sessions.iter().position(|s| s.id == query) {
        return SessionMatchOutcome::ExactOrUnique(idx);
    }

    let exact_native: Vec<usize> = sessions
        .iter()
        .enumerate()
        .filter_map(|(idx, s)| {
            let nat = s.id.split_once(':').map_or(s.id.as_str(), |(_, r)| r);
            (nat == query).then_some(idx)
        })
        .collect();
    if exact_native.len() == 1 {
        return SessionMatchOutcome::ExactOrUnique(exact_native[0]);
    }
    if exact_native.len() > 1 {
        return SessionMatchOutcome::Ambiguous(
            exact_native
                .into_iter()
                .take(5)
                .map(|i| sessions[i].id.clone())
                .collect(),
        );
    }

    let q_lower = query.to_ascii_lowercase();
    let ci_exact: Vec<usize> = sessions
        .iter()
        .enumerate()
        .filter_map(|(idx, s)| {
            let nat = s.id.split_once(':').map_or(s.id.as_str(), |(_, r)| r);
            (s.id.eq_ignore_ascii_case(query) || nat.eq_ignore_ascii_case(query)).then_some(idx)
        })
        .collect();
    if ci_exact.len() == 1 {
        return SessionMatchOutcome::ExactOrUnique(ci_exact[0]);
    }
    if ci_exact.len() > 1 {
        return SessionMatchOutcome::Ambiguous(
            ci_exact
                .into_iter()
                .take(5)
                .map(|i| sessions[i].id.clone())
                .collect(),
        );
    }

    let affixes: Vec<usize> = sessions
        .iter()
        .enumerate()
        .filter_map(|(idx, s)| {
            let id_lower = s.id.to_ascii_lowercase();
            let nat_lower = id_lower
                .split_once(':')
                .map_or(id_lower.as_str(), |(_, r)| r);
            let matched = id_lower.starts_with(&q_lower)
                || id_lower.ends_with(&q_lower)
                || nat_lower.starts_with(&q_lower)
                || nat_lower.ends_with(&q_lower);
            matched.then_some(idx)
        })
        .collect();

    match affixes.len() {
        0 => SessionMatchOutcome::NotFound,
        1 => SessionMatchOutcome::ExactOrUnique(affixes[0]),
        _ => SessionMatchOutcome::Ambiguous(
            affixes
                .into_iter()
                .take(5)
                .map(|i| sessions[i].id.clone())
                .collect(),
        ),
    }
}

fn extract_fts_tokens(raw_query: &str) -> Vec<String> {
    raw_query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(str::trim)
        .filter(|tok| !tok.is_empty())
        .take(12)
        .map(ToString::to_string)
        .collect()
}

fn build_fts_match_expr(tokens: &[String], joiner: &str) -> String {
    tokens
        .iter()
        .map(|tok| {
            let escaped = tok.replace('"', "\"\"");
            format!("\"{escaped}\"*")
        })
        .collect::<Vec<_>>()
        .join(joiner)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tempfile::TempDir;

    use super::*;

    fn sample_session(id: &str, now_ms: i64, user_inputs: u64) -> IngestedSession {
        IngestedSession {
            id: id.to_string(),
            harness: "codex".to_string(),
            source_path: format!("/tmp/synthetic/{id}.jsonl"),
            source_size: 256,
            source_mtime_ms: now_ms,
            mode: IngestMode::FullReplace,
            cwd: Some("/tmp/workspace".to_string()),
            step_count: 8,
            user_input_count: user_inputs,
            first_user_prompt: format!("Synthetic prompt for {id}"),
            prompt_timestamps_ms: vec![now_ms - 600_000, now_ms - 300_000],
            raw_spans: vec![RawSpan::clamped(now_ms - 600_000, now_ms - 120_000)],
            archived: false,
            is_subagent: false,
            parent_session_id: None,
            saw_turn_state: true,
            has_open_turn: false,
            started_at: now_ms - 600_000,
            last_active_at: now_ms - 120_000,
            model: Some("gpt-5".to_string()),
            input_tokens: 10_000,
            cache_read_tokens: 90_000,
            cache_write_tokens: 0,
            output_tokens: 2_000,
            total_tokens: 102_000,
            usage_is_cumulative: true,
        }
    }

    /// Deterministic local mid-afternoon anchor so relative minute offsets in tests
    /// never straddle local midnight regardless of wall-clock time or runner timezone.
    fn fixed_local_midday() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, 14, 15, 0, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn store_open_enforces_wal_and_busy_timeout_and_seeds_one_off() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("nested").join("planar.db");
        let store = Store::open(&db_path).unwrap();

        let mode: String = store
            .connection()
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        let timeout: i64 = store
            .connection()
            .pragma_query_value(None, "busy_timeout", |r| r.get(0))
            .unwrap();
        let sync_mode: i64 = store
            .connection()
            .pragma_query_value(None, "synchronous", |r| r.get(0))
            .unwrap();

        assert_eq!(mode.to_lowercase(), "wal");
        assert_eq!(timeout, 5000);
        assert_eq!(sync_mode, 1); // NORMAL = 1

        let one_off: String = store
            .connection()
            .query_row(
                "SELECT name FROM lines WHERE id = ?1",
                params![ONE_OFF_LINE_ID],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(one_off, "One-off");
    }

    #[test]
    fn batch_default_4_migration_handles_fresh_and_legacy_databases() {
        // 1. Fresh DB: migration marks completion without materializing an `app_settings` row.
        let fresh_dir = TempDir::new().unwrap();
        let fresh_path = fresh_dir.path().join("fresh.db");
        let fresh_store = Store::open(&fresh_path).unwrap();
        let persisted: Option<String> = fresh_store
            .connection()
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![SETTINGS_ROW_KEY],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(
            persisted.is_none(),
            "fresh database should not persist an app_settings row during migration"
        );
        let fresh_flag: Option<String> = fresh_store
            .connection()
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'schema_batch_default_4_v1'",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(fresh_flag.as_deref(), Some("1"));
        assert_eq!(fresh_store.get_settings().unwrap(), AppSettings::default());
        drop(fresh_store);

        // 2. Legacy DB with explicitly persisted syncBatchLimit = 8 and custom apiModel override:
        //    migrates 8 -> 4 while preserving the saved provider and model override, and is idempotent.
        let legacy_8_dir = TempDir::new().unwrap();
        let legacy_8_path = legacy_8_dir.path().join("legacy8.db");
        {
            let conn = Connection::open(&legacy_8_path).unwrap();
            conn.execute(
                "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value_json) VALUES (?1, ?2)",
                params![
                    SETTINGS_ROW_KEY,
                    r#"{"syncBatchLimit":8,"apiProvider":"openai","apiModel":"gpt-4.1-mini"}"#
                ],
            )
            .unwrap();
        }
        let legacy_8_store = Store::open(&legacy_8_path).unwrap();
        let migrated = legacy_8_store.get_settings().unwrap();
        assert_eq!(migrated.sync_batch_limit, 4);
        assert_eq!(migrated.api_provider, "openai");
        assert_eq!(migrated.api_model, "gpt-4.1-mini");

        // Explicitly re-saving 8 after the one-time migration must stick across reopens.
        let mut user_set_8 = migrated;
        user_set_8.sync_batch_limit = 8;
        legacy_8_store.save_settings(&user_set_8).unwrap();
        drop(legacy_8_store);
        let reopened_8 = Store::open(&legacy_8_path).unwrap();
        assert_eq!(reopened_8.get_settings().unwrap().sync_batch_limit, 8);
        drop(reopened_8);

        // 3. Legacy DB with persisted partial row omitting syncBatchLimit and apiModel:
        //    migration leaves the raw row untouched and deserialization derives the provider's model.
        let legacy_partial_dir = TempDir::new().unwrap();
        let legacy_partial_path = legacy_partial_dir.path().join("legacy_partial.db");
        let raw_partial = r#"{"language":"en","apiProvider":"deepseek"}"#;
        {
            let conn = Connection::open(&legacy_partial_path).unwrap();
            conn.execute(
                "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value_json) VALUES (?1, ?2)",
                params![SETTINGS_ROW_KEY, raw_partial],
            )
            .unwrap();
        }
        let partial_store = Store::open(&legacy_partial_path).unwrap();
        let stored_after: String = partial_store
            .connection()
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![SETTINGS_ROW_KEY],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored_after, raw_partial);
        let loaded_partial = partial_store.get_settings().unwrap();
        assert_eq!(loaded_partial.language, "en");
        assert_eq!(loaded_partial.api_provider, "deepseek");
        assert_eq!(
            loaded_partial.api_model,
            crate::defaults::api_provider("deepseek")
                .unwrap()
                .default_model
        );
        drop(partial_store);

        // 4. Legacy DB with explicitly persisted custom syncBatchLimit (12) stays untouched.
        let legacy_12_dir = TempDir::new().unwrap();
        let legacy_12_path = legacy_12_dir.path().join("legacy12.db");
        {
            let conn = Connection::open(&legacy_12_path).unwrap();
            conn.execute(
                "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value_json) VALUES (?1, ?2)",
                params![SETTINGS_ROW_KEY, r#"{"syncBatchLimit":12}"#],
            )
            .unwrap();
        }
        let store_12 = Store::open(&legacy_12_path).unwrap();
        assert_eq!(store_12.get_settings().unwrap().sync_batch_limit, 12);
    }

    #[test]
    fn store_crud_dependencies_and_plane_projection() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        let fixed_now = offset
            .with_ymd_and_hms(2026, 11, 14, 15, 0, 0)
            .single()
            .unwrap();
        let now: DateTime<Local> = fixed_now.with_timezone(&Local);
        let now_ms = now.timestamp_millis();

        let line_id = store.create_line("Planar Core").unwrap();
        assert_eq!(line_id, "planar-core");

        store
            .upsert_ingested_session(&sample_session("codex:s1", now_ms, 4))
            .unwrap();
        store
            .upsert_ingested_session(&sample_session("codex:s2", now_ms, 5))
            .unwrap();
        store
            .upsert_ingested_session(&sample_session("codex:short", now_ms, 1))
            .unwrap();

        store
            .update_session("codex:s1", &LineIdUpdate::Set(line_id.clone()), None)
            .unwrap();
        store
            .update_session("codex:s2", &LineIdUpdate::Set(line_id.clone()), None)
            .unwrap();
        store
            .upsert_dependency("codex:s2", "codex:s1", "depends_on", "manual")
            .unwrap();

        let plane = store.build_plane_response(7, now).unwrap();
        assert_eq!(plane.days.len(), 7);
        assert_eq!(plane.short_sessions.total_count, 1);
        assert_eq!(plane.unsummarized.len(), 0);

        let line = plane.lines.iter().find(|l| l.id == line_id).unwrap();
        assert_eq!(line.sessions.len(), 2);
        let s2 = line.sessions.iter().find(|s| s.id == "codex:s2").unwrap();
        assert_eq!(s2.depends_on_id.as_deref(), Some("codex:s1"));
    }

    #[test]
    fn month_and_contribution_calendar_projection_and_sentinel_filter() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        let fixed_now = offset
            .with_ymd_and_hms(2026, 11, 14, 15, 0, 0)
            .single()
            .unwrap();
        let now: DateTime<Local> = fixed_now.with_timezone(&Local);
        let now_ms = now.timestamp_millis();

        let mut dsh_sess = sample_session("dsh:active-1", now_ms, 4);
        dsh_sess.harness = "dsh".to_string();
        store.upsert_ingested_session(&dsh_sess).unwrap();

        let mut claude_sess = sample_session("claude:active-2", now_ms - 86_400_000 * 10, 4);
        claude_sess.harness = "claude".to_string();
        store.upsert_ingested_session(&claude_sess).unwrap();

        // Internal digest prompts must never enter the SQLite store.
        let mut sentinel_sess = sample_session("codex:internal-echo", now_ms, 1);
        sentinel_sess.first_user_prompt =
            format!("{INTERNAL_DIGEST_SENTINEL} summarize session payload");
        store.upsert_ingested_session(&sentinel_sess).unwrap();
        assert!(
            store
                .get_session_launch_info("codex:internal-echo")
                .is_err()
        );

        let plane_month = store.build_plane_response(30, now).unwrap();
        assert_eq!(plane_month.days.len(), 30);
        assert!(
            (134..=140).contains(&plane_month.contributions.len()),
            "expected 20-week calendar (~134-140 days), got {}",
            plane_month.contributions.len()
        );

        let total_contrib_sessions: usize = plane_month
            .contributions
            .iter()
            .map(|c| c.session_count)
            .sum();
        assert_eq!(total_contrib_sessions, 2);

        let mut today_match = plane_month
            .contributions
            .iter()
            .filter(|c| c.date == "2026-11-14")
            .collect::<Vec<_>>();
        assert_eq!(today_match.len(), 1);
        let today_contrib = today_match.pop().unwrap();
        assert_eq!(today_contrib.by_harness.get("dsh"), Some(&8));

        // Disabling `dsh` filters it out of `unsummarized` while keeping harness status metadata.
        let mut settings = store.get_settings().unwrap();
        settings.enabled_harnesses.dsh = false;
        store.save_settings(&settings).unwrap();

        let filtered = store.build_plane_response(30, now).unwrap();
        assert!(filtered.unsummarized.iter().all(|s| s.harness != "dsh"));
        assert!(
            filtered
                .harnesses
                .iter()
                .any(|h| h.id == "dsh" && !h.enabled)
        );
    }

    #[test]
    fn update_line_validates_and_persists_hex_color() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("planar.db")).unwrap();

        let line_id = store.create_line("Design Polish").unwrap();
        store
            .update_line(
                &line_id,
                &LineUpdate {
                    color: Some("#2656C4".to_string()),
                    ..LineUpdate::default()
                },
            )
            .unwrap();

        let saved_color: String = store
            .connection()
            .query_row(
                "SELECT color FROM lines WHERE id = ?1",
                params![&line_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved_color, "#2656C4");

        let err = store
            .update_line(
                &line_id,
                &LineUpdate {
                    color: Some("rgb(1,2,3)".to_string()),
                    ..LineUpdate::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, PlanarError::InvalidInput(_)));
    }

    #[test]
    fn pruned_source_trajectory_locks_existing_digest_and_marks_source_missing() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let source_file = dir.path().join("rollout-pruned.jsonl");
        fs::write(&source_file, "{}\n").unwrap();

        let now = fixed_local_midday();
        let now_ms = now.timestamp_millis();
        let mut sess = sample_session("codex:pruned-1", now_ms, 5);
        sess.source_path = source_file.to_string_lossy().into_owned();
        store.upsert_ingested_session(&sess).unwrap();

        store
            .apply_session_digest(
                "codex:pruned-1",
                sess.step_count,
                "initial-slice-hash",
                "Preserved Monthly Summary",
                "Completed monthly retention architecture",
                "",
                "Planar Core",
                false,
            )
            .unwrap();

        // While source file exists, changing slice hash schedules re-summarization.
        let settings = AppSettings::default();
        let cands_before = store
            .load_sync_candidates(&settings, "updated-slice-hash", false)
            .unwrap();
        assert_eq!(cands_before.len(), 1);

        // Simulate Harness auto-pruning the old `.jsonl` file on disk.
        fs::remove_file(&source_file).unwrap();

        // Existing digest is now locked even if slice config changes or `force_all` is true.
        let cands_after = store
            .load_sync_candidates(&settings, "updated-slice-hash", true)
            .unwrap();
        assert_eq!(cands_after.len(), 0);

        // Month plane response retains full bursts and AI digest, flagging `source_exists = false`.
        let plane = store.build_plane_response(30, now).unwrap();
        let planar_line = plane
            .lines
            .iter()
            .find(|l| l.name == "Planar Core")
            .unwrap();
        let dto = planar_line
            .sessions
            .iter()
            .find(|s| s.id == "codex:pruned-1")
            .unwrap();
        assert_eq!(dto.title, "Preserved Monthly Summary");
        assert_eq!(dto.source_exists, Some(false));
        assert_eq!(dto.bursts.len(), 1);

        // Lite snapshot skips disk stat() and explicitly marks `source_exists` as `None` (unknown).
        let lite = store.build_lite_plane_response(30, None, now).unwrap();
        assert!(lite.is_lite);
        let lite_dto = lite
            .lines
            .iter()
            .find(|l| l.name == "Planar Core")
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id == "codex:pruned-1")
            .unwrap();
        assert_eq!(lite_dto.source_exists, None);
    }

    #[test]
    fn migration_adds_token_columns_and_resets_only_existing_file_cursors() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("legacy.db");
        let existing_jsonl = dir.path().join("existing.jsonl");
        let pruned_jsonl = dir.path().join("pruned.jsonl");
        fs::write(&existing_jsonl, "{}\n").unwrap();

        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                    id                          TEXT PRIMARY KEY,
                    harness                     TEXT NOT NULL,
                    source_path                 TEXT NOT NULL,
                    source_size                 INTEGER NOT NULL DEFAULT 0,
                    source_mtime_ms             INTEGER NOT NULL DEFAULT 0,
                    cwd                         TEXT,
                    step_count                  INTEGER NOT NULL DEFAULT 0,
                    user_input_count            INTEGER NOT NULL DEFAULT 0,
                    first_user_prompt           TEXT NOT NULL DEFAULT '',
                    user_prompt_timestamps_json TEXT NOT NULL DEFAULT '[]',
                    archived                    INTEGER NOT NULL DEFAULT 0,
                    hidden                      INTEGER NOT NULL DEFAULT 0,
                    is_subagent                 INTEGER NOT NULL DEFAULT 0,
                    parent_session_id           TEXT,
                    has_open_turn               INTEGER NOT NULL DEFAULT 0,
                    line_id                     TEXT,
                    started_at                  INTEGER NOT NULL DEFAULT 0,
                    last_active_at              INTEGER NOT NULL DEFAULT 0
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions (id, harness, source_path, source_size, source_mtime_ms, step_count)
                 VALUES ('codex:alive', 'codex', ?1, 4096, 1700000000, 12)",
                params![existing_jsonl.to_string_lossy().as_ref()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions (id, harness, source_path, source_size, source_mtime_ms, step_count)
                 VALUES ('codex:pruned', 'codex', ?1, 8192, 1700000000, 24)",
                params![pruned_jsonl.to_string_lossy().as_ref()],
            )
            .unwrap();
        }

        let store = Store::open(&db_path).unwrap();
        let cursors = store.load_file_cursors().unwrap();
        let alive_cur = cursors
            .get(&existing_jsonl.to_string_lossy().into_owned())
            .unwrap();
        let pruned_cur = cursors
            .get(&pruned_jsonl.to_string_lossy().into_owned())
            .unwrap();

        assert_eq!(alive_cur.source_size, 0);
        assert_eq!(alive_cur.source_mtime_ms, 0);
        assert_eq!(pruned_cur.source_size, 8192);
        assert_eq!(pruned_cur.source_mtime_ms, 1_700_000_000);
    }

    #[test]
    fn line_catalog_and_regroup_items_preserve_counts_cwd_and_unicode_slugs() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let zh_id = store.create_line("向量检索评测").unwrap();
        assert_eq!(zh_id, "向量检索评测");

        let now_ms = Local::now().timestamp_millis();
        let mut sess = sample_session("claude:vault-1", now_ms, 4);
        sess.cwd = Some("/Users/demo/notes-workspace".to_string());
        store.upsert_ingested_session(&sess).unwrap();
        store
            .apply_session_digest(
                "claude:vault-1",
                sess.step_count,
                "hash-1",
                "Weekly Recall Eval",
                "Calibrated top-30 recall benchmark",
                "",
                "search-eval",
                false,
            )
            .unwrap();

        let catalog = store.list_line_catalog().unwrap();
        let pinned_entry = catalog.iter().find(|c| c.name == "向量检索评测").unwrap();
        assert!(pinned_entry.pinned);
        assert_eq!(pinned_entry.session_count, 0);

        let disc_entry = catalog.iter().find(|c| c.name == "search-eval").unwrap();
        assert!(!disc_entry.pinned);
        assert_eq!(disc_entry.session_count, 1);

        let regroup_items = store.list_unpinned_items_for_regroup().unwrap();
        assert_eq!(regroup_items.len(), 1);
        assert_eq!(
            regroup_items[0].cwd.as_deref(),
            Some("/Users/demo/notes-workspace")
        );
    }

    #[test]
    fn archived_sessions_preserve_line_totals_contributions_and_status() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let now = fixed_local_midday();
        let now_ms = now.timestamp_millis();

        // Summarized session later archived in Codex
        let mut filed_arch = sample_session("codex:filed-arch", now_ms, 5);
        filed_arch.archived = true;
        filed_arch.raw_spans = vec![RawSpan::clamped(now_ms - 45 * 60_000, now_ms)];
        store.upsert_ingested_session(&filed_arch).unwrap();
        store
            .apply_session_digest(
                "codex:filed-arch",
                filed_arch.step_count,
                "h-arch",
                "Heavy archived task",
                "Shipped feature",
                "",
                "search-core",
                false,
            )
            .unwrap();

        // Unassigned archived session
        let mut raw_arch = sample_session("codex:raw-arch", now_ms, 4);
        raw_arch.archived = true;
        raw_arch.raw_spans = vec![RawSpan::clamped(now_ms - 20 * 60_000, now_ms)];
        store.upsert_ingested_session(&raw_arch).unwrap();

        let plane = store.build_plane_response(7, now).unwrap();
        // Filed archived session stays in its Line so Line totals & swimlane never shrink
        let line = plane
            .lines
            .iter()
            .find(|l| l.name == "search-core")
            .unwrap();
        assert_eq!(line.sessions.len(), 1);
        assert!(line.sessions[0].archived);
        // Unassigned archived session stays out of active Sync queue
        assert_eq!(plane.unsummarized.len(), 0);

        // Contribution heatmap counts BOTH archived sessions (45m + 20m = 65m)
        let today_iso = now.date_naive().format("%Y-%m-%d").to_string();
        let today_contrib = plane
            .contributions
            .iter()
            .find(|c| c.date == today_iso)
            .unwrap();
        assert_eq!(today_contrib.total_minutes, 65);

        // L0 status counts BOTH in global totals and filed session in Line summary
        let snap = store.build_status_snapshot(7, now).unwrap();
        assert_eq!(snap.total_window_minutes, 65);
        assert_eq!(snap.unsummarized_count, 0);
        let status_line = snap.lines.iter().find(|l| l.name == "search-core").unwrap();
        assert_eq!(status_line.window_minutes, 45);
    }

    #[test]
    fn line_description_and_merge_lines_move_sessions_pin_and_append_scope() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let now = fixed_local_midday();
        let now_ms = now.timestamp_millis();

        let s1 = sample_session("codex:sc-1", now_ms, 4);
        let s2 = sample_session("codex:rw-1", now_ms, 4);
        store.upsert_ingested_session(&s1).unwrap();
        store.upsert_ingested_session(&s2).unwrap();

        store
            .apply_session_digest(
                "codex:sc-1",
                10,
                "h1",
                "Core index pipeline",
                "Done",
                "",
                "search-core",
                false,
            )
            .unwrap();
        store
            .apply_session_digest(
                "codex:rw-1",
                10,
                "h2",
                "Rank worker work",
                "Done",
                "",
                "rank-worker",
                false,
            )
            .unwrap();

        let plane_before = store.build_plane_response(7, now).unwrap();
        let sc_id = plane_before
            .lines
            .iter()
            .find(|l| l.name == "search-core")
            .unwrap()
            .id
            .clone();
        let rw_id = plane_before
            .lines
            .iter()
            .find(|l| l.name == "rank-worker")
            .unwrap()
            .id
            .clone();

        // Self-merge & system-bucket source are rejected
        assert!(store.merge_lines(&rw_id, &rw_id, true, true).is_err());
        assert!(
            store
                .merge_lines(ONE_OFF_LINE_ID, &sc_id, true, true)
                .is_err()
        );

        // Merge rank-worker -> search-core
        let moved = store.merge_lines(&rw_id, &sc_id, true, true).unwrap();
        assert_eq!(moved, 1);

        let plane_after = store.build_plane_response(7, now).unwrap();
        assert!(!plane_after.lines.iter().any(|l| l.id == rw_id));
        let merged_sc = plane_after.lines.iter().find(|l| l.id == sc_id).unwrap();
        assert!(merged_sc.pinned);
        assert_eq!(merged_sc.sessions.len(), 2);
        assert!(merged_sc.description.contains("rank-worker"));

        // Updating description and metric_lens = "cost" persists cleanly
        store
            .update_line(
                &sc_id,
                &LineUpdate {
                    description: Some("Core search pipeline & rank-worker evaluation".to_string()),
                    ..LineUpdate::default()
                },
            )
            .unwrap();
        let catalog = store.list_line_catalog().unwrap();
        let sc_cat = catalog.iter().find(|c| c.id == sc_id).unwrap();
        assert_eq!(
            sc_cat.description,
            "Core search pipeline & rank-worker evaluation"
        );

        let mut settings = store.get_settings().unwrap();
        settings.metric_lens = "cost".to_string();
        store.save_settings(&settings).unwrap();
        assert_eq!(store.get_settings().unwrap().metric_lens, "cost");
    }

    #[test]
    fn cwd_status_batch_l1_bounded_l2_and_fts5_search_end_to_end() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("planar.db")).unwrap();

        let now = fixed_local_midday();
        let now_ms = now.timestamp_millis();

        let planar_line_id = store
            .create_line_with_description(
                "Planar Core",
                "Ship read-only MCP server and FTS5 recall before UI planning sprint",
            )
            .unwrap();
        store
            .update_line(
                &planar_line_id,
                &LineUpdate {
                    priority: Some(1),
                    status: Some("active".to_string()),
                    ..LineUpdate::default()
                },
            )
            .unwrap();

        let recall_line_id = store
            .create_line_with_description("向量检索评测", "Weekly vector recall evaluation")
            .unwrap();

        // Write real synthetic JSONL files containing tool outputs and [PLANAR_INTERNAL_DIGEST] echoes.
        let s1_log = dir.path().join("s1.jsonl");
        fs::write(
            &s1_log,
            concat!(
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Design SQLite FTS5 migration in crates/planar-core/src/db.rs\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"bash\",\"input\":{\"cmd\":\"cat secret\"}},{\"type\":\"text\",\"text\":\"Opened migration plan.\"}]}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"RAW_TOOL_OUTPUT_MUST_BE_STRIPPED_999\"}]}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Step 3: define schema columns\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Step 4: virtual table DDL ready.\"}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Step 5: hook upsert transaction\"}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Middle pivot: add bm25 ranking across title and stopped_at\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Middle agent reply omitted by slicer.\"}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"[PLANAR_INTERNAL_DIGEST] internal echo should vanish\"}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Step 8: wire digest sync\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Step 9: digest sync wired.\"}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Step 10: add CJK fallback\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Step 11: CJK fallback wired.\"}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Verify backward-compatible backfill test\"}}\n"
            ),
        )
        .unwrap();

        let s2_log = dir.path().join("s2.jsonl");
        fs::write(
            &s2_log,
            concat!(
                "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Wire POST /mcp onto crates/planar-core/src/server.rs\"}]}}\n",
                "{\"timestamp\":\"2026-11-20T10:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call_output\",\"output\":[{\"text\":\"CODEX_TOOL_BLOB_MUST_BE_STRIPPED_888\"}]}}\n",
                "{\"timestamp\":\"2026-11-20T10:02:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Mounted Streamable HTTP endpoint and 3s scan throttle.\"}]}}\n"
            ),
        )
        .unwrap();

        let mut s1 = sample_session("claude:alpha-1111", now_ms - 600_000, 3);
        s1.harness = "claude".to_string();
        s1.source_path = s1_log.to_string_lossy().into_owned();
        s1.cwd = Some("/Users/demo/repos/planar".to_string());
        s1.first_user_prompt =
            "Design SQLite FTS5 migration in crates/planar-core/src/db.rs".to_string();
        store.upsert_ingested_session(&s1).unwrap();

        let mut s2 = sample_session("codex:beta-2222", now_ms - 10_000, 3);
        s2.harness = "codex".to_string();
        s2.source_path = s2_log.to_string_lossy().into_owned();
        s2.cwd = Some("/Users/demo/repos/planar/crates/planar-core".to_string());
        s2.has_open_turn = true;
        store.upsert_ingested_session(&s2).unwrap();

        let mut s3 = sample_session("antigravity:gamma-3333", now_ms - 300_000, 4);
        s3.harness = "antigravity".to_string();
        s3.cwd = Some("/Users/demo/repos/recall".to_string());
        store.upsert_ingested_session(&s3).unwrap();

        store
            .update_session(
                "claude:alpha-1111",
                &LineIdUpdate::Set(planar_line_id.clone()),
                None,
            )
            .unwrap();
        store
            .apply_session_digest(
                "claude:alpha-1111",
                8,
                "h1",
                "SQLite FTS5 index & migration",
                "Landed sessions_fts virtual table and backfill",
                "",
                "Planar Core",
                false,
            )
            .unwrap();

        store
            .update_session(
                "codex:beta-2222",
                &LineIdUpdate::Set(planar_line_id.clone()),
                None,
            )
            .unwrap();
        store
            .apply_session_digest(
                "codex:beta-2222",
                6,
                "h2",
                "Axum Streamable HTTP /mcp",
                "Mounted POST /mcp handler",
                "Run integration smoke test against agy and codex",
                "Planar Core",
                false,
            )
            .unwrap();

        store
            .update_session(
                "antigravity:gamma-3333",
                &LineIdUpdate::Set(recall_line_id.clone()),
                None,
            )
            .unwrap();
        store
            .apply_session_digest(
                "antigravity:gamma-3333",
                10,
                "h3",
                "每周向量检索评估基线",
                "完成 Top-30 向量检索离线评测",
                "补充冷启动分桶对比",
                "向量检索评测",
                false,
            )
            .unwrap();

        store
            .upsert_dependency(
                "codex:beta-2222",
                "claude:alpha-1111",
                "depends_on",
                "manual",
            )
            .unwrap();

        // 1. Workspace-aware L0 status (including morning follow-up & recency signals)
        let snap = store
            .build_status_snapshot_with_cwd(7, Some("/Users/demo/repos/planar"), now)
            .unwrap();
        assert_eq!(snap.cwd_line_ids, vec![planar_line_id.clone()]);
        assert_eq!(snap.live_sessions.len(), 1);
        assert_eq!(snap.live_sessions[0].id, "codex:beta-2222");
        assert!(snap.live_sessions[0].matches_cwd);
        assert!(snap.live_sessions[0].has_open_turn);
        let planar_status_line = snap.lines.iter().find(|l| l.id == planar_line_id).unwrap();
        assert_eq!(planar_status_line.open_followups, 1);
        assert_eq!(planar_status_line.last_active_recency, "running");
        let recall_status_line = snap.lines.iter().find(|l| l.id == recall_line_id).unwrap();
        assert_eq!(recall_status_line.open_followups, 1);
        assert_ne!(recall_status_line.last_active_recency, "");

        // Prefix/suffix resolution for `planar open`
        let resolved_launch = store.resolve_session_launch_info("beta-2222").unwrap();
        assert_eq!(resolved_launch.id, "codex:beta-2222");

        // Growing a digested session by +2 steps (one user+agent exchange) schedules refresh on next Sync
        let mut s1_grown = s1;
        s1_grown.step_count = 10;
        store.upsert_ingested_session(&s1_grown).unwrap();
        let grown_cands = store
            .load_sync_candidates(&AppSettings::default(), "h1", false)
            .unwrap();
        assert!(
            grown_cands.iter().any(|c| c.id == "claude:alpha-1111"),
            "session that grew by 2 steps must be re-queued for digest refresh"
        );

        // 2. Batch L1 (`query_lines`): partial-match tolerant, case-insensitive name + slug, deps
        assert!(store.query_lines(&[], 8, now).is_err());
        let nine: Vec<String> = (0..9).map(|i| format!("l-{i}")).collect();
        assert!(store.query_lines(&nine, 8, now).is_err());

        let l1 = store
            .query_lines(
                &[
                    "planar core".to_string(),
                    recall_line_id.clone(),
                    "missing-line".to_string(),
                ],
                8,
                now,
            )
            .unwrap();
        assert_eq!(l1.lines.len(), 2);
        assert_eq!(l1.not_found, vec!["missing-line".to_string()]);
        assert_eq!(l1.available_lines.len(), 3);
        assert_eq!(
            l1.lines[0].description,
            "Ship read-only MCP server and FTS5 recall before UI planning sprint"
        );
        assert_eq!(l1.lines[0].sessions.len(), 2);
        assert_eq!(l1.lines[0].sessions[0].id, "codex:beta-2222");
        assert_eq!(
            l1.lines[0].sessions[0].depends_on[0].session_id,
            "claude:alpha-1111"
        );
        assert_eq!(
            l1.lines[0].sessions[1].depended_by[0].session_id,
            "codex:beta-2222"
        );

        // 3. Bounded-batch L2 (`query_sessions`): prefix/suffix resolution, full vs handoff, tool/sentinel stripping
        assert!(store.query_sessions(&[], SessionFocus::Full, now).is_err());
        assert!(
            store
                .query_sessions(
                    &[
                        "s1".to_string(),
                        "s2".to_string(),
                        "s3".to_string(),
                        "s4".to_string()
                    ],
                    SessionFocus::Full,
                    now
                )
                .is_err()
        );

        let mut settings = store.get_settings().unwrap();
        settings.opening_steps = 5;
        settings.closing_steps = 5;
        store.save_settings(&settings).unwrap();

        let l2_full = store
            .query_sessions(
                &["alpha-1111".to_string(), "2222".to_string()],
                SessionFocus::Full,
                now,
            )
            .unwrap();
        assert_eq!(l2_full.sessions.len(), 2);
        assert_eq!(l2_full.sessions[0].head.len(), 5);
        assert_eq!(l2_full.sessions[0].middle_users.len(), 1);
        let serialized_full = serde_json::to_string(&l2_full).unwrap();
        assert!(!serialized_full.contains("RAW_TOOL_OUTPUT_MUST_BE_STRIPPED_999"));
        assert!(!serialized_full.contains("CODEX_TOOL_BLOB_MUST_BE_STRIPPED_888"));
        assert!(!serialized_full.contains(INTERNAL_DIGEST_SENTINEL));

        let l2_handoff = store
            .query_sessions(&["alpha-1111".to_string()], SessionFocus::Handoff, now)
            .unwrap();
        assert_eq!(l2_handoff.sessions[0].head.len(), 0);
        assert!(l2_handoff.sessions[0].digest.is_some());
        assert_eq!(l2_handoff.sessions[0].middle_users.len(), 1);
        assert_eq!(l2_handoff.sessions[0].tail.len(), 5);

        // 4. SQLite FTS5 search (`search_sessions`): English tokens, path punctuation, CJK substring, line filter
        let hit_fts = store
            .search_sessions(
                &SearchQuery {
                    query: "Streamable HTTP agy".to_string(),
                    line: None,
                    days: Some(7),
                    limit: 10,
                },
                now,
            )
            .unwrap();
        assert_eq!(hit_fts.hits.len(), 1);
        assert_eq!(hit_fts.hits[0].session_id, "codex:beta-2222");

        let hit_punct = store
            .search_sessions(
                &SearchQuery {
                    query: "crates/planar-core/src/db.rs".to_string(),
                    line: Some("planar-core".to_string()),
                    days: None,
                    limit: 10,
                },
                now,
            )
            .unwrap();
        assert_eq!(hit_punct.hits.len(), 1);
        assert_eq!(hit_punct.hits[0].session_id, "claude:alpha-1111");

        let hit_cjk = store
            .search_sessions(
                &SearchQuery {
                    query: "向量检索离线评测".to_string(),
                    line: None,
                    days: None,
                    limit: 10,
                },
                now,
            )
            .unwrap();
        assert_eq!(hit_cjk.hits.len(), 1);
        assert_eq!(hit_cjk.hits[0].session_id, "antigravity:gamma-3333");
    }
}
