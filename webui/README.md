# Rove WebUI

The WebUI is the hosted shell for Rove. It is built with Next.js App Router and
talks to the local daemon over authenticated HTTP and WebSocket endpoints on
`https://127.0.0.1:47630`.

The canonical product flow is:

- `app.roveai.co` serves the UI shell
- the local daemon remains the execution authority
- the browser authenticates against the daemon with password-backed bearer
  sessions

## Development

```bash
cd /Users/as/workspace/rove/core/webui
npm install
npm run dev
```

For a production validation build:

```bash
npm run build
```

Run the daemon separately:

```bash
cargo run -p engine --bin rove -- daemon --profile desktop --port 47630
```

## Current Pages

| Route | Purpose |
| ----- | ------- |
| `/` | Unlock/setup flow, task submission, and task history |
| `/config` | Daemon profile, auth timing, secret backend, and node identity |
| `/approvals` | Pending approvals and approval-rule CRUD |
| `/brains` | Brain-family status |
| `/plugins` | Extension and system-surface management |
| `/policy` | Active policy status and policy surfaces |
| `/remote` | Remote peers, node identity, and ZeroTier transport status |
| `/settings` | Service install state, auth/session state, and daemon controls |

## Client Structure

```text
webui/
  src/app/           Next.js routes
  src/components/    shared UI components
  src/lib/daemon.ts  daemon HTTP and WS client
  src/stores/        Zustand state and workflow coordination
```

## Daemon Contract

The UI currently depends on:

- `/v1/hello`
- `/v1/auth/*`
- `/v1/tasks`
- `/v1/approvals`
- `/v1/config`
- `/v1/services/install`
- `/v1/remote/*`
- `/v1/events/ws`

The UI does not trust local state alone. Lock state, reauth, and session expiry
are enforced by the daemon.

## Notes

- `npm run build` validates the hosted-shell client, but the daemon is not the
  canonical UI host.
- Local development can use either `http://127.0.0.1:47630` or
  `https://127.0.0.1:47630`; production expects the HTTPS local daemon path.
