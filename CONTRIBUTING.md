# Contributing

Humans and agents are both welcome. The rules in [ARCHITECTURE.md](ARCHITECTURE.md) apply to everyone.

## Before opening a PR

```sh
cargo fmt
cargo clippy --all-targets   # no warnings
cargo test                   # includes the determinism tests
cargo run --release -- demo
node scripts/gui-smoke.mjs   # if you touched the GUI, the session or an adapter (needs Node 22+ and Chromium)
```

On Debian/Ubuntu you need `libpipewire-0.3-dev`, `pkg-config` and `clang`. On Arch, `pipewire` and `clang`.

## Agent contributions

Agents can open PRs like anyone else. A few extra expectations:

- Say in the PR description that an agent wrote it, and which one.
- Run the checks above and paste the `cargo test` summary.
- Keep each PR to one concern. A new device, a new path and a new adapter are three PRs.
- New devices need a test showing they're deterministic across block sizes. Copy the pattern in `icarus-render`.
