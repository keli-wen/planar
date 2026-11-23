//! Core local-first work plane engine for Planar.
//!
//! Provides read-only streaming trajectory ingestion across Claude Code, Codex,
//! Antigravity, and `DeepSeek` Harness (`dsh`) logs, clamped burst active-time union,
//! 20-week contribution aggregation, SQLite WAL persistence, Head–Middle–Tail
//! trajectory slicing with dual CLI/API processors, and the local Axum HTTP API
//! serving `./ui`.

pub mod burst;
pub mod db;
pub mod env;
pub mod error;
pub mod ingest;
pub mod mcp;
pub mod model;
pub mod pricing;
pub mod processor;
pub mod render;
pub mod server;
