# Planar Issue Templates Guide

Planar uses **AI-friendly, contexts-first GitHub Issue Forms** so every issue doubles as a self-contained handoff spec for both human contributors and clean-session coding agents.

## Available Templates

| Template | File | Default Labels | Typical Review Tier (`REVIEW.md`) |
| :--- | :--- | :--- | :--- |
| **Bug Report** | `bug.yml` | `bug` | `T1` (UI/CLI) or `T2` (Core/DB/Ingest/MCP) |
| **Feature Request** | `feature.yml` | `enhancement` | `T1`–`T3` depending on blast radius |
| **New Harness / Provider Adapter** | `harness-adapter.yml` | `enhancement`, `area:harness` | `T2` (new harness ingestor) or `T1` (OpenAI-compat API provider) |
| **Context / Workflow Update** | `context-update.yml` | `documentation`, `area:context` | `T0` (docs/context) or `T2`/`T3` (hooks/RFC) |

> See [`CONTRIBUTING.md` §4](../../CONTRIBUTING.md#4-issue--pr-labeling-guide-agent-native-taxonomy) for the full agent-native label taxonomy (`Type`, `area:*`, `tier:*`, `agent:*`, `P0`–`P2`).

## For Coding Agents Filing Issues via `gh issue create`

Follow [`WORKFLOW.md` §1](../../WORKFLOW.md#1-issue-first-intake--filing):
1. Populate `Source Context`, `Suggested Review Tier`, `Context Action`, `Target Context`, and `Verification Plan`.
2. Write the issue draft marker (`scripts/hooks/pre_tool_use_issue_guard.py --write-marker ...`) before running `gh issue create`.
