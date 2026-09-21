# Cloudflare Tunnel deployment

This deployment exposes Agent Telemetry through a remotely-managed Cloudflare Tunnel without
publishing the receiver port on the host.

## Topology

```text
Decision Runtime Worker
  -> Cloudflare Access Service Auth
  -> public HTTPS hostname
  -> Cloudflare Tunnel
  -> agent-telemetry:4318
  -> GreptimeDB
```

The Compose file has no `ports:` entry. Agent Telemetry is reachable only from the Compose bridge
network.

## 1. Create the Access application first

Create a self-hosted Access application for the **canonical ingest path only** before publishing
the tunnel route:

```text
telemetry.example.com/v1/events*
```

Use a **Service Auth** policy whose Include rule selects the service token used by Apocrypha
Decision Runtime. Access path rules are intentionally narrower than the receiver's other OTLP and
hook endpoints.

Do not use an Access Bypass policy.

Enable **Protect with Access** on the Tunnel route. Cloudflare Access validates the service-token
policy at the edge before the request reaches the Tunnel; `cloudflared` only forwards the request
after Access has authorized it.

## 2. Create a remotely-managed Tunnel

Create a remotely-managed Tunnel and configure a published application route:

```text
Hostname: telemetry.example.com
Path:     /v1/events
Service:  http://agent-telemetry:4318
```

Cloudflare Tunnel path routing preserves the request path, so Agent Telemetry still receives
`POST /v1/events`. Do not create a catch-all route for this hostname unless another protected
integration needs it.

Copy the Tunnel token from the "Add a replica" flow.

The token is a bearer credential. Keep it outside Git and out of the container command line.

## 3. Configure the host

```bash
cd deploy/cloudflare-tunnel
cp .env.example .env

install -m 700 -d secrets
read -rsp 'Tunnel token: ' CLOUDFLARE_TUNNEL_TOKEN; echo
printf '%s' "$CLOUDFLARE_TUNNEL_TOKEN" > secrets/cloudflare_tunnel_token
unset CLOUDFLARE_TUNNEL_TOKEN
sudo chgrp 65532 secrets/cloudflare_tunnel_token
chmod 640 secrets/cloudflare_tunnel_token
```

The host user remains the file owner, while group `65532` matches the UID/GID used by the pinned
`cloudflared` image. Mode `0640` lets that non-root container user read the bind-mounted Compose
secret without making the token world-readable.

Set `ATEL_GREPTIME_ENDPOINT` in `.env` and optionally override
`ATEL_GREPTIME_DATABASE`.

Compose mounts `secrets/cloudflare_tunnel_token` at
`/run/secrets/cloudflare_tunnel_token`, and `cloudflared` reads it with `--token-file`.
The token is therefore not interpolated into Compose command metadata or injected into the
container environment.

Then start the stack:

```bash
docker compose up -d --build
```

No host port needs to be opened for 4318. Agent Telemetry exposes an internal `/healthz` endpoint;
Compose waits for that healthcheck to pass before starting `cloudflared`.

## 4. Verify Access

Requests to `/v1/events` without the service token should be rejected by Access. Other Agent
Telemetry endpoints should not have a Tunnel route at all in this deployment.

A valid machine request uses:

```text
CF-Access-Client-Id: <client id>
CF-Access-Client-Secret: <client secret>
```

Test only the canonical Decision endpoint used by Apocrypha:

```bash
curl --fail-with-body \
  -H "CF-Access-Client-Id: $CF_ACCESS_CLIENT_ID" \
  -H "CF-Access-Client-Secret: $CF_ACCESS_CLIENT_SECRET" \
  -H "Content-Type: application/json" \
  --data @event.json \
  https://telemetry.example.com/v1/events
```

A successful ingest returns HTTP 204.

## 5. Configure Apocrypha

Set the Access-protected origin in `deployment.jsonc`:

```json
{
  "decisionRuntime": {
    "enabled": true,
    "telemetryUrl": "https://telemetry.example.com",
    "timeoutMs": 5000
  }
}
```

Install the same Access service-token pair as Decision Runtime Worker secrets:

```bash
pnpm exec wrangler secret put AGENT_TELEMETRY_CF_ACCESS_CLIENT_ID \
  --name apocrypha-decision-runtime

pnpm exec wrangler secret put AGENT_TELEMETRY_CF_ACCESS_CLIENT_SECRET \
  --name apocrypha-decision-runtime
```

Then redeploy Apocrypha.

## Security notes

- Tunnel token, Access Client ID, and Access Client Secret never belong in Git.
- The Tunnel token is mounted as a Compose secret and read with `--token-file`.
- Rotate the Tunnel token and service token independently.
- Restrict the Access application to `/v1/events*` and Service Auth for the Decision Runtime token.
- The receiver remains bound to the container network rather than the host network.
- GreptimeDB remains a separate private dependency and is not exposed by this Compose stack.
- `cloudflared` is pinned to a reviewed version and multi-architecture image digest; upgrades are deliberate.
