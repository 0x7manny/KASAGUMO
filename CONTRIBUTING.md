# Contributing

Thanks for taking a look at Kasagumo.

## Getting set up

You need [Rust](https://rustup.rs) (edition 2024, stable toolchain) and
[Docker](https://www.docker.com) to run real workloads. The test suite does not
need Docker.

```bash
cargo build
cargo test
```

## Before you open a pull request

CI runs on Linux and macOS and is strict, so run the same checks locally:

```bash
cargo clippy --all-targets -- -D warnings
cargo test
```

Both must pass. Clippy warnings fail the build.

## Conventions

- **Commit messages** follow Conventional Commits: `feat(node): …`,
  `fix(storage): …`, `docs: …`, `test: …`, `perf: …`, `refactor: …`.
- **Code comments and user-facing messages** are in French, matching the rest of
  the source. Documentation and commit messages are in English.
- Keep the permission model in mind: anything cluster-wide
  (`Forward`, `Schedule`, `Put`, `Get`) must stay local-only. See
  [docs/architecture.md](docs/architecture.md).
- Prefer small, focused commits that each build and pass tests.

## Where things live

| Area | File |
|------|------|
| CLI parsing and commands | `src/cli.rs`, `src/commands.rs` |
| Daemon, scheduling, gossip, recovery | `src/daemon.rs` |
| Persistent state | `src/store.rs` |
| Node identity and the handshake | `src/identity.rs` |
| TLS transport | `src/secure.rs` |
| Container runtime | `src/runtime.rs` |
| Content-addressed blocks | `src/blobs.rs` |
| Wire protocol types | `src/protocol.rs` |

A tour of how they fit together is in [docs/architecture.md](docs/architecture.md).
