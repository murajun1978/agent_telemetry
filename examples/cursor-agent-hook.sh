#!/usr/bin/env bash
set -u

endpoint="${ATEL_CURSOR_HOOK_URL:-http://127.0.0.1:4318/v1/hooks/cursor}"
payload="$(cat)"

# Telemetry must never block or fail the Cursor agent loop.
curl --fail --silent --show-error \
  --connect-timeout 1 \
  --max-time 2 \
  -H 'content-type: application/json' \
  --data-binary "$payload" \
  "$endpoint" >/dev/null 2>&1 || true

exit 0
