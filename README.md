# Purpose
A visible running application that can control open port and long-running SSH commands. I'm tired of having:
- local services stop running and
- SSH tunneling / SSH local port forwarding commands fall over and
- not being sure where I was running the command.

This app is the home for such long-lived commands. It keeps track of standing them up on command, shutting them down if requested, showing a dashboard of statuses, and logging a history of restarts.

# Target uses

The dashboard currently targets long-running commands like SSH tunnels and local services. Here are natural extensions:

## Development Infrastructure
- Local dev servers — webpack-dev-server, vite, next dev, cargo watch
- Database proxies — cloud SQL proxy, PgBouncer, connection poolers
- Mock/stub servers — WireMock, json-server, local API fakes for frontend dev
- Hot-reload watchers — file watchers that trigger builds, test runners in watch mode

## Networking & Tunnels
- VPN connections — WireGuard, OpenVPN client tunnels
- Reverse tunnels — ngrok, cloudflared, bore for exposing local services
- SOCKS proxies — SSH SOCKS proxy for browser routing
- DNS-over-HTTPS — local DoH proxies like dnscrypt-proxy

## Data & Messaging
- Queue consumers — Kafka consumers, RabbitMQ workers, Redis pub/sub listeners
- Log tailers — persistent tail -f on remote logs via SSH
- Sync agents — rsync watchers, unison, mutagen file sync sessions
- Database replication streams — CDC listeners, binlog tailers

## Security & Auth
- SSH agents — ssh-agent with specific key lifecycles
- Token refreshers — scripts that periodically refresh OAuth/OIDC tokens
- Certificate watchers — mTLS cert renewal daemons

## Infrastructure Tools
- Container sidecars — local containers that support dev (Redis, Postgres, MinIO)
- Port forwards to Kubernetes — kubectl port-forward sessions that die frequently
- Service mesh proxies — Envoy, linkerd-proxy for local service mesh dev

## Monitoring & Observability
- Metric exporters — Prometheus node_exporter, custom metric scrapers
- Health check pingers — scripts that poll upstream dependencies and alert
- Log shippers — fluentbit, vector, filebeat forwarding local logs

The strongest use cases share three traits: they're long-running, they fail silently (no visible error when they drop), and restarting them is the correct recovery action. Kubernetes port-forwards and reverse tunnels are particularly good fits — they're notoriously flaky and exactly the kind of thing people lose track of across terminals.

# Known problems
1. ssh key passphrase prompt
You can avoid this by registering your SSH key before calling `ssh-dashboard`::
```
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/some-machine-private-key-file
```
You will be prompted once for the passphrase of the key file, but not in `ssh-dashboard`.

2. ssh user password prompt
Try to avoid being prompted for a user password. For a start, don't allow password login as an option on your machines.
Secondly, pass `-i ~/.ssh/somemachine-private-key-file` as a command line option to your port-forwarding command to ssh.
