# Agent Telemetry

Observe how AI agents act, decide, and improve.

Agent Telemetry is a vendor- and storage-neutral observability and decision-intelligence layer for AI agents. It normalizes agent-specific telemetry into a canonical model and persists it through pluggable storage adapters.

## Architecture

```text
Claude Code / Codex / Cursor Agent / Gemini CLI / ...
                    |
                    v
          OTLP + Agent Hooks
                    |
                    v
         Agent Telemetry Receiver
                    |
                    v
           Semantic Adapters
   /       |       |       |        \
Claude   Codex   Cursor  Gemini  Generic OTel
   \       |       |       |        /
             Canonical AgentEvent
                    |
            TelemetryStore port
              |           |
         GreptimeDB    DuckDB
                    |
                    v
         Token + Decision Analytics
```

The canonical event model includes `Observation -> Decision -> Action -> Outcome -> Learning` as first-class event kinds. Vendor-specific raw data is retained on every event so semantic adapters can evolve without losing source facts.

## Current status

The Rust MVP currently includes:

- canonical `AgentEvent`, `DecisionContext`, and `TokenUsage` models
- declarative storage boundary via `TelemetryStore`
- GreptimeDB storage adapter
- optional DuckDB storage adapter
- JSONL import
- OTLP HTTP/protobuf receiver for logs and traces
- Cursor Agent hook receiver at `/v1/hooks/cursor`
- Claude Code semantic adapter
- Codex semantic adapter
- Cursor Agent semantic adapter
- Gemini CLI semantic adapter
- generic OpenTelemetry fallback adapter
- session/turn token analytics and Decision -> Action -> Outcome flow correlation
- cross-session and cross-agent efficiency comparison
- token-efficiency metrics such as tokens per success and retry token ratio
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

## Quick start with Codex

Codex configures OpenTelemetry in `~/.codex/config.toml`. Point its log and trace exporters at Agent Telemetry using OTLP HTTP protobuf:

```toml
[otel]
environment = "dev"
log_user_prompt = false
exporter = { otlp-http = { endpoint = "http://127.0.0.1:4318/v1/logs", protocol = "binary" } }
trace_exporter = { otlp-http = { endpoint = "http://127.0.0.1:4318/v1/traces", protocol = "binary" } }
metrics_exporter = "none"
```

Start Agent Telemetry in one terminal:

```bash
cargo run -- init
cargo run -- serve
```

Then start Codex in another terminal:

```bash
codex
```

Then inspect normalized Codex events:

```bash
cargo run -- recent --agent codex --limit 20
```

The Codex adapter normalizes `codex.user_prompt`, `codex.api_request`, `codex.sse_event`, `codex.tool_decision`, `codex.tool_result`, and other Codex telemetry while preserving the original attributes and raw signal. Prompt text remains disabled in the example configuration.

## Quick start with Cursor Agent

Cursor command hooks receive JSON over stdin. Agent Telemetry can accept these events directly at `/v1/hooks/cursor`.

Start Agent Telemetry first:

```bash
cargo run -- init
cargo run -- serve
```

For a project-level setup, copy the example hook forwarder and config into your repository:

```bash
mkdir -p .cursor/hooks
cp examples/cursor-agent-hook.sh .cursor/hooks/agent-telemetry.sh
cp examples/cursor-hooks.json .cursor/hooks.json
chmod +x .cursor/hooks/agent-telemetry.sh
```

Then run Cursor Agent normally. The hook integration captures session lifecycle, prompt submission metadata, tool decisions, tool outcomes, subagents, file edits, compaction, and agent completion without storing prompt text, response text, tool inputs/outputs, or thought content by default.

Inspect normalized Cursor events with:

```bash
cargo run -- recent --agent cursor-agent --limit 20
```

The hook forwarder defaults to `http://127.0.0.1:4318/v1/hooks/cursor`. Override it with `ATEL_CURSOR_HOOK_URL` when needed. Telemetry forwarding is fail-open so an unavailable collector does not block Cursor's agent loop.

Cursor Enterprise can also export server-side OpenTelemetry logs. Agent Telemetry recognizes CLI exports through Cursor resource attributes such as `service.name=cursor` and `cursor.surface=cli` and normalizes model usage and hook/skill events through the same Cursor adapter.

## Quick start with Gemini CLI

Gemini CLI can export OpenTelemetry directly over OTLP HTTP. Start Agent Telemetry first:

```bash
cargo run -- init
cargo run -- serve
```

Then configure `.gemini/settings.json`:

```json
{
  "telemetry": {
    "enabled": true,
    "target": "local",
    "otlpEndpoint": "http://127.0.0.1:4318",
    "otlpProtocol": "http",
    "useCollector": true,
    "logPrompts": false,
    "traces": true
  }
}
```

Run Gemini CLI normally:

```bash
gemini
```

Inspect normalized events with:

```bash
cargo run -- recent --agent gemini-cli --limit 20
```

The Gemini adapter maps prompt submission, tool execution, API calls, model routing, security verdicts, file operations, and GenAI spans into the canonical event model. Sensitive attributes such as user email, prompt/response text, tool arguments, GenAI input/output messages, system instructions, tool definitions, and model-routing reasoning are removed before persistence.

`telemetry.logPrompts` defaults to enabled in Gemini CLI, so the example explicitly disables it. Detailed trace collection is opt-in and enabled here to capture GenAI operation spans; Agent Telemetry still strips content-bearing trace attributes before storing them.

## Token and decision analytics

`TokenUsage` keeps input/output tokens canonical while also supporting optional cached-input, reasoning-token, cost, and vendor-specific breakdowns. Existing events that only contain the legacy `input_tokens`, `output_tokens`, and `cost_usd` fields remain analyzable; Agent Telemetry hydrates the richer model when ingesting or analyzing them.

Analyze one session:

```bash
cargo run -- analyze --session <session-id> --agent codex
```

The JSON report includes:

- total input/output/cached/reasoning tokens and cost
- token totals by model
- per-turn token totals
- Decision / Action / ToolCall / Outcome / Error counts
- an ordered Decision -> Action/ToolCall -> Outcome flow per turn
- successful outcomes and retries
- `tokens_per_success`
- `cost_per_success`
- `tokens_per_decision`
- `tokens_per_tool_call`
- `retry_token_ratio`

This keeps token analysis tied to agent behavior and outcomes instead of treating token consumption as a standalone cost metric.

## Cross-session and cross-agent comparison

Compare recent sessions across all agents:

```bash
cargo run -- compare --limit 5000
```

Compare sessions for one agent:

```bash
cargo run -- compare --agent codex --limit 5000
```

The comparison report includes, without ranking agents:

- session count and successful-session count per agent
- session success rate
- total and average tokens/cost per session
- tokens/cost per successful session
- decisions, tools, actions, outcomes, errors, and retries
- the same efficiency metrics used by single-session analysis
- model-level token totals across agents
- per-session comparison rows

The comparison command operates on the most recent events returned by the selected backend. Use a sufficiently large `--limit` for the comparison window you intend to analyze.

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
atel analyze --session <id> [--agent <name>] [--limit <n>]
atel compare [--agent <name>] [--limit <n>]
atel serve [--bind 127.0.0.1:4318]
```

## Next

- OTLP metrics receiver
- task/cohort labels for like-for-like agent comparisons
- MCP query interface


## Canonical event ingest

The OTLP receiver also accepts already-normalized Agent Telemetry events:

```text
POST /v1/events
Content-Type: application/json
```

The body may be one `AgentEvent` or a JSON array of up to 500 events, with a 16 MiB request-body
limit on this route. Events are hydrated for
token usage and written through the configured `TelemetryStore`, so the same endpoint works with
GreptimeDB and the optional DuckDB backend.

Decision events can carry calibrated decision metadata in `decision`:

```json
{
  "kind": "decision",
  "name": "agent_trace_triage",
  "model": "jev-1.13.0",
  "decision": {
    "question": "agent trace triage",
    "selected": "REVIEW",
    "confidence": 0.82,
    "probabilities": {
      "HEALTHY": 0.10,
      "REVIEW": 0.82,
      "RETRY": 0.06,
      "INCIDENT": 0.02
    },
    "risk": 0.5,
    "provider": "cloudflare-workers-ai",
    "route": ["rule", "jev"],
    "details": {
      "answers": {}
    }
  }
}
```

`details` preserves the full typed decision payload so calibration and analysis can evolve without
discarding the model's original probabilities or provenance.
