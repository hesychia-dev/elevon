# Elevon

Elevon is a lightweight deployment platform for running containerized
applications on your own Linux VMs. It handles the workflow from source to
running service: building images, publishing them to a registry, deploying
releases, routing traffic, and rolling back when needed.

It is designed for teams that want a simple, self-hosted deployment workflow
without adopting a large managed platform or operating a full container
orchestration system.

## What It Is

Elevon has two components:

- **CLI:** Runs on your development machine to configure, build, publish, and
  deploy applications.
- **Agent:** Runs on the target VM to manage deployments, containers, routing,
  and local state.

The CLI communicates with the agent through authenticated API requests. The
agent uses Unix sockets for communication between its internal services.

## Goals

- Be simple to understand, install, and use.
- Make production deployments fast in both execution time and setup time.
- Give individuals and teams of all sizes a practical alternative to expensive
  managed deployment platforms.
- Keep the operational model lightweight so a deployment does not require a
  large platform or a dedicated operations team.
- Make the common deployment workflow predictable and approachable.
- (SOON) Have a dedicated dashboard for monitoring and managing your applications and agents from one place.
- And more...

## Installation

Install the agent on the target VM and the CLI on your development machine:

| Component | Supported platforms             |
| --------- | ------------------------------- |
| CLI       | Linux `x86_64`, macOS `aarch64` |
| Agent     | Linux `x86_64`                  |

### Agent: system-wide installation

```bash
curl -fsSL https://el.hesychia-dev.dev/install-agent.sh | sudo bash
```

### CLI: current-user installation

```bash
curl -fsSL https://el.hesychia-dev.dev/install-cli.sh | bash
```

## Configuration

A deployment configuration describes the application image, registry,
routing, build context, environment, and app-specific settings:

```yaml
name: elevon
image: hesychia-dev/elevon

elevon:
  agent:
    url: $AGENT_URL
    key: $AGENT_KEY

registry:
  username: $GHCR_USERNAME
  password: $GHCR_PASSWORD

routing:
  domain: el.hesychia-dev.dev
  port: $PORT
  tls:
    cert: $TLS_CERT
    key: $TLS_KEY

build: ../

env:
  inherit:
    - PORT
```

Use [`crates/elevon-deploy/templates/config.yml`](crates/elevon-deploy/templates/config.yml)
as the full template, including comments about defaults and environment
variable expansion.

## CLI

Initialize and deploy an application with:

```bash
elevon init
elevon check
elevon deploy
elevon rollback
```

The CLI uses `.elevon/deploy.yml` by default. Run `elevon --help` for the
complete command reference.

Manage the agent separately on the target VM:

```bash
elevon-agent init
sudo elevon-agent install --enable
elevon-agent key --help
sudo elevon-agent uninstall
```

See the installation and configuration sections above for setup details.

## GitHub Actions

Install the Elevon CLI in GitHub Actions with
[`hesychia-dev/setup-elevon`](https://github.com/hesychia-dev/setup-elevon).

See [`DEVELOPMENT.md`](DEVELOPMENT.md) for workspace details and development
checks. See [`TODO.md`](TODO.md) for planned work.

## License

See [`LICENSE`](LICENSE).
