# Elevon

Elevon is a lightweight deployment platform for running containerized
applications on your own Linux VMs. It handles the workflow from source to
running service: building images, publishing them to a registry, deploying
releases, routing traffic, and rolling back when needed.

It is designed for teams that want a simple, self-hosted deployment workflow
without adopting a large managed platform or operating a full container
orchestration system.


<div align="center">

[Documentation](https://elevon.ccxlv.dev)

</div>


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

See the [Installation](https://elevon.ccxlv.dev/docs/install) guide for
supported platforms and full setup.

### Agent: system-wide installation

```bash
curl -fsSL https://elevon.ccxlv.dev/get/agent.sh | sudo bash
```

### CLI: current-user installation

```bash
curl -fsSL https://elevon.ccxlv.dev/get/cli.sh | bash
```

## Configuration

[CLI configuration](https://elevon.ccxlv.dev/docs/cli/configuration) and
[Agent configuration](https://elevon.ccxlv.dev/docs/agent/configuration).

## CLI

See the [CLI](https://elevon.ccxlv.dev/docs/cli) and
[Agent](https://elevon.ccxlv.dev/docs/agent) docs for commands and day-to-day use.

## GitHub Actions

See [GitHub Actions](https://elevon.ccxlv.dev/docs/ci) for deploying from CI.

See [`DEVELOPMENT.md`](DEVELOPMENT.md) for workspace details and development
checks. See [`TODO.md`](TODO.md) for planned work.

## License

See [`LICENSE`](LICENSE).
