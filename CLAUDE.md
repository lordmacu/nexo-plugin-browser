# nexo-rs-plugin-browser — Project Guide

Chrome DevTools Protocol client for the nexo agent framework. See
the root architecture doc at
[`/home/familia/chat/CLAUDE.md`](../CLAUDE.md) for the workspace
layout and retry policy.

## Commands

```bash
cargo build                          # dev (mold + sccache active globally)
cargo build --profile release-fast   # release-grade, no LTO — ~50% faster
cargo build --release                # publish/dist binary
cargo nextest run                    # parallel tests
cargo bench --bench tool_latency     # stdio round-trip latency bench
```

## Build toolchain

Machine-wide config in `~/.cargo/config.toml`: `mold` linker via
`clang`, `sccache` as `rustc-wrapper`, `debug = "line-tables-only"`
on dev.

Profiles defined in this crate's `Cargo.toml`:

- `release` — `opt-level = 3`, `lto = "thin"`, `strip = "symbols"`.
- `release-fast` — same opt-level, `lto = false`, `codegen-units = 16`.
  Use for local validation; reserve `--release` for publish.

If a build breaks with "linker `clang` not found" or "failed to spawn
sccache", reinstall:

```bash
sudo apt install -y mold clang
cargo install sccache cargo-nextest --locked
```

## Language rules

- Code identifiers + comments + repo Markdown in **English**.
- Conversations in **Spanish**; code artifacts always English.
