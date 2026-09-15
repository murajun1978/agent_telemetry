# Agent Telemetry

Observe how AI agents act, decide, and improve.

Agent Telemetry is a vendor- and storage-neutral observability and decision-intelligence layer for AI agents. It normalizes agent-specific telemetry into a canonical model and persists it through pluggable storage adapters.

## Architecture

```text
Claude Code / Codex / Gemini / ...
              |
              v
      OTLP HTTP/protobuf
              |
              v
     Agent Telemetry Receiver
              |
              v
       Semantic Adapters
        |             |
  Claude Code     Generic OTel
        \             /
         Canonical AgentEvent
              |
      TelemetryStore port
        |           |
   GreptimeDB    DuckDB
```

The canonical event model includes `Observation -> Decision -> Action -> Outcome -> Learning` as first-class event kinds. Vendor-specific raw data is retained on every event so semantic adapters can evolve without losing source facts.

## Current status

The Rust MVP currently includes:

- canonical `AgentEvent` and `DecisionContext` models
- declarative storage boundary via `TelemetryStore`
- GreptimeDB storage adapter
- optional DuckDB storage adapter
- JSONL import
- OTLP HTTP/protobuf receiver for logs and traces
- Claude Code semantic adapter
- generic OpenTelemetry fallback adapter
- `atel` CLI

## Quick start with Claude Code

Start GreptimeDB locally on `http://127.0.0.1:4000`, then run Agent Telemetry:

```bash
cargo run -- init
cargo run -- serve
```

In another shell, configure Claude Code to send logs and beta traces to Agent Telemetry:

```bash
export CLAUDE_CODE_ENABLE_TELEMETRY=1
export CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1
export OTEL_LOGS_EXPORTER=otlp
export OTEL_TRACES_EXPORTER=otlp
export OTEL_METRICS_EXPORTER=none
export OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318
claude
```

Then inspect normalized events:

```bash
cargo run -- recent --agent claude-code --limit 20
```

Claude Code prompt and response content remains redacted by default. Agent Telemetry does not require enabling `OTEL_LOG_USER_PROMPTS`, `OTEL_LOG_ASSISTANT_RESPONSES`, or `OTEL_LOG_TOOL_DETAILS` for the basic observability flow.

## DuckDB backend

```bash
cargo run --features duckdb-backend -- \
  --backend duckdb \
  --duckdb-path agent_telemetry.duckdb \
  init
```

The same backend can run the OTLP receiver:

```bash
cargo run --features duckdb-backend -- \
  --backend duckdb \
  --duckdb-path agent_telemetry.duckdb \
  serve
```

## CLI

```text
atel init
atel import <events.jsonl>
atel recent [--agent <name>] [--session <id>] [--limit <n>]
atel serve [--bind 127.0.0.1:4318]
```

## Next

- OTLP metrics receiver
- Codex semantic adapter
- Gemini CLI semantic adapter
- Decision -> Action -> Outcome correlation queries
- MCP query interface
