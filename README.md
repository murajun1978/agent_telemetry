# Agent Telemetry

**Observe how AI agents decide, act, and improve.**

Agent Telemetry is a local-first observability and decision-intelligence platform for AI agents. The core is vendor-neutral: Claude Code, Codex, Gemini CLI, and future agents are normalized into a canonical event model, while storage/query backends remain replaceable.

## Architecture

```text
Agent -> Ingest Adapter -> Canonical Agent Event -> TelemetryStore
                                             |-> GreptimeDB
                                             |-> DuckDB
                                             `-> future backends
```

The canonical lifecycle is:

```text
Observation -> Decision -> Action -> Outcome -> Learning
```

Raw vendor telemetry is retained in `AgentEvent.raw`; normalized attributes live alongside it. Decision events can carry evidence, alternatives, constraints, assumptions, confidence and expected outcomes without storing private chain-of-thought.

## Status

The first Rust MVP includes:

- canonical `AgentEvent` and `DecisionContext`
- storage port (`TelemetryStore`)
- GreptimeDB backend over the HTTP SQL API
- optional DuckDB backend
- JSONL import adapter
- `atel` CLI for initialization, import, and recent-event queries

Native OTLP ingestion and vendor-specific semantic adapters are the next implementation slice.

## Build

```bash
cargo build
```

DuckDB is feature-gated because the bundled native library makes builds heavier:

```bash
cargo build --features duckdb-backend
```

## GreptimeDB quick start

Assuming GreptimeDB is running locally on port 4000:

```bash
cargo run -- init
cargo run -- import ./examples/events.jsonl
cargo run -- recent --limit 20
```

Configuration can also be supplied through environment variables:

```bash
export ATEL_GREPTIME_ENDPOINT=http://127.0.0.1:4000
export ATEL_GREPTIME_DATABASE=public
```

GreptimeDB's SQL HTTP endpoint is `/v1/sql`; Agent Telemetry keeps that backend detail behind `TelemetryStore`.

## DuckDB

```bash
cargo run --features duckdb-backend -- \
  --backend duckdb \
  --duckdb-path ./agent_telemetry.duckdb \
  init
```

## Canonical event example

```json
{
  "timestamp": "2026-09-15T12:00:00Z",
  "agent": "claude-code",
  "session_id": "session-1",
  "kind": "decision",
  "name": "diagnose_test_failure",
  "decision": {
    "question": "Why did the test fail?",
    "evidence": ["rspec output", "postgres log"],
    "alternatives": ["fixture issue", "database connection"],
    "selected": "database connection",
    "constraints": [],
    "assumptions": ["postgres should be reachable"],
    "confidence": 0.72,
    "expected_outcome": "test passes after fixing connection"
  },
  "attributes": {},
  "raw": {}
}
```

## Design direction

The product core is the canonical model and query semantics, not a specific database. GreptimeDB is the natural real-time OTLP backend; DuckDB is the natural local/ad-hoc analytics backend. Additional sinks, archives, and query adapters can be introduced without changing the domain model.
