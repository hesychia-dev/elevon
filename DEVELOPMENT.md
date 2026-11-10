# Development

Elevon is a Rust workspace for building and operating the CLI and agent.

## Workspace

The main crates are:

- `elevon-agent`: host-side API, proxy, Docker orchestration, and lifecycle.
- `elevon-cli`: user-facing command-line entry point.
- `elevon-deploy`: deployment configuration and image operations.
- `elevon-fs`: agent paths, permissions, and service files.
- `elevon-http`: shared HTTP and authentication behavior.
- `elevon-contracts`: shared API and deployment types.

## Checks

```bash
cargo check --workspace
cargo test --workspace
bash -n scripts/install-agent.sh scripts/install-cli.sh scripts/resolve-release-archive.sh
```

Automated tests are still limited. Beta validation also requires manually
checking installation, systemd startup and shutdown, deployment, rollback,
proxy routing, and uninstall behavior.

## Releases

Current release artifacts are built for the agent on Linux `x86_64`, and for
the CLI on Linux `x86_64` and macOS `aarch64`. Other platform and architecture combinations are not currently supported.
