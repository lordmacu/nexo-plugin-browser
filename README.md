# nexo-plugin-browser

Out-of-tree subprocess plugin for the [nexo-rs](https://github.com/nexo-rs)
daemon.  Ships the 12 `browser_*` tools (Chrome DevTools Protocol via raw CDP)
as a standalone binary loaded by the daemon at boot via discovery +
auto-subprocess fallback.

> **Phase 81.17.c.** Pilot extraction of an in-tree plugin to a separate repo,
> validating the subprocess + manifest + sandbox + remote-tool-handler
> infrastructure end-to-end against a real binary (not a bash mock or
> in-workspace template).

## Why this is a separate repo

`nexo-rs` previously bundled this plugin inside the daemon binary
(`crates/plugins/browser/`). That meant:

- Operators couldn't upgrade the browser plugin independently of the daemon.
- The daemon's tool registry held a direct `Arc<BrowserPlugin>` and called
  `plugin.execute(...)` in-process — not portable to multi-tenant /
  per-agent sandbox isolation.
- Community-tier plugins had no proven precedent for the wire shape.

Phase 81.17.c flips browser to the same wire-protocol every external plugin
will use:

```
                    JSON-RPC over stdio
   nexo-rs daemon  ─────────────────────►  nexo-plugin-browser (this binary)
       │            ◄─────────────────────                  │
       │            tool.invoke / replies                   │
   RemoteToolHandler                                  PluginAdapter
   per-tool dispatch                                  + on_tool dispatcher
                                                            │
                                                            ▼
                                                     BrowserPlugin
                                                     (CDP → Chrome)
```

The in-tree `crates/plugins/browser/` stays dormant in the workspace until
external operators validate the standalone path, then gets removed
(follow-up `81.17.c.in-tree-removal`).

## Install (developer / dev daemon)

```bash
cd /path/to/nexo-rs-plugin-browser
cargo build --release
```

Drop the resulting binary + manifest into a directory the daemon's
`plugins.discovery.search_paths` covers:

```bash
mkdir -p ~/.local/share/nexo/plugins/browser
cp target/release/nexo-plugin-browser ~/.local/share/nexo/plugins/browser/
cp nexo-plugin.toml                   ~/.local/share/nexo/plugins/browser/
```

In the daemon's `plugins.yaml`:

```yaml
plugins:
  discovery:
    search_paths:
      - ~/.local/share/nexo/plugins
```

Restart the daemon. On boot the discovery walker picks up
`~/.local/share/nexo/plugins/browser/nexo-plugin.toml`, the auto-subprocess
fallback (Phase 81.17.b) spawns `nexo-plugin-browser`, and
`register_remote_tool_handlers_after_init` registers all 12
`browser_*` tool handlers in the agent's scoped registry.

## Configuration (env vars)

The daemon translates its `cfg.plugins.browser` YAML into env vars before
spawning the subprocess (see
`proyecto/src/main.rs::seed_browser_subprocess_env`). Operators only edit
the YAML; the env-var layer is internal.

| Env var | Type | Default | Notes |
|---|---|---|---|
| `NEXO_PLUGIN_BROWSER_HEADLESS` | bool (`true`/`1`/`false`/`0`) | `false` | Headless Chrome (no window). |
| `NEXO_PLUGIN_BROWSER_EXECUTABLE` | path | (auto-detect) | Override the Chrome binary path. |
| `NEXO_PLUGIN_BROWSER_CDP_URL` | URL | (empty) | Empty = launch new Chrome. Set to e.g. `http://127.0.0.1:9222` to attach to an existing instance. |
| `NEXO_PLUGIN_BROWSER_USER_DATA_DIR` | path | `./.browser-profile` | Chrome user-data directory. |
| `NEXO_PLUGIN_BROWSER_WINDOW_WIDTH` | u32 | `1280` | Initial window width (px). |
| `NEXO_PLUGIN_BROWSER_WINDOW_HEIGHT` | u32 | `800` | Initial window height (px). |
| `NEXO_PLUGIN_BROWSER_CONNECT_TIMEOUT_MS` | u64 | `8000` | CDP connection timeout. |
| `NEXO_PLUGIN_BROWSER_COMMAND_TIMEOUT_MS` | u64 | `30000` | Per-CDP-command timeout. |
| `NEXO_PLUGIN_BROWSER_ARGS` | comma-separated list | (empty) | Extra Chrome CLI flags, e.g. `--no-sandbox,--disable-dev-shm-usage`. |

## Sandbox

The shipped `nexo-plugin.toml` sets `[plugin.sandbox] enabled = true`. When
`bwrap` is on PATH the daemon wraps the spawn under bubblewrap with:

- `network = "host"` — Chromium fetches arbitrary URLs.
- `fs_read_paths` — Chromium binary candidates (`/usr/bin/chromium`,
  `/usr/bin/google-chrome`, …).
- `fs_write_paths = ["${state_dir}"]` — the user-data dir
  (`NEXO_PLUGIN_BROWSER_USER_DATA_DIR`) lives here.
- `drop_user = false` — **Chromium's own sandbox needs user
  namespaces**. Setting `drop_user = true` will work for Chromium 's
  no-sandbox path (`--no-sandbox`) but is unsupported.

Operators on hosts without `bwrap`: the daemon degrades with a
`tracing::warn!` and spawns un-sandboxed. Set
`NEXO_PLUGIN_SANDBOX_REQUIRE=1` to refuse the degrade and block boot.

## Supervisor

`[plugin.supervisor] respawn = false`. On Chrome / plugin crash the daemon
emits `plugin.lifecycle.browser.crashed { exit_code, stderr_tail }` (Phase
81.21 + 81.21.b) — operator restarts the daemon to recover. Auto-respawn
ships in Phase 81.21.b.b.

`stderr_tail_lines = 64`: the last 64 lines of the subprocess's stderr
ring-buffer into the crash event for post-mortem.

## Tools

12 `browser_*` tools advertised in the `initialize` reply:

| Name | Schema | Notes |
|---|---|---|
| `browser_navigate` | `{url}` | Navigate the active page. |
| `browser_click` | `{target}` | `target` = `@eN` ref or CSS selector. |
| `browser_fill` | `{target, value}` | Replace the element's content. |
| `browser_screenshot` | `{}` | Returns base64 PNG. |
| `browser_evaluate` | `{script}` | Run JS in page; returns `result`. |
| `browser_snapshot` | `{}` | Text+structure snapshot with `@eN` refs. |
| `browser_scroll_to` | `{target}` | Scroll element into view. |
| `browser_current_url` | `{}` | Read `location.href`. |
| `browser_wait_for` | `{selector, timeout_ms?}` | Poll until element appears. |
| `browser_go_back` | `{}` | `history.back()`. |
| `browser_go_forward` | `{}` | `history.forward()`. |
| `browser_press_key` | `{key}` | Allowed key names + single char. |

All wire shapes mirror the in-tree `crates/plugins/browser/src/tool.rs`
defs 1:1.

## Latency budget

**Placeholder.** The bench in `benches/tool_latency.rs` measures
in-process direct CDP vs subprocess RPC. Run on representative hardware
and populate this section.

```
TODO: cargo bench  (post-extraction; follow-up 81.17.c.latency-numbers)

| Tool                  | In-tree direct (ms p95) | Subprocess (ms p95) | Delta |
|-----------------------|-------------------------|---------------------|-------|
| browser_navigate      |  TBD                    | TBD                 | TBD   |
| browser_snapshot      |  TBD                    | TBD                 | TBD   |
| browser_evaluate      |  TBD                    | TBD                 | TBD   |
| browser_current_url   |  TBD                    | TBD                 | TBD   |
```

## Build prerequisites

- Rust 1.75+ (`rust-version` in Cargo.toml).
- Linux: `bwrap` (bubblewrap) for sandbox; degrades cleanly when missing.
- Chromium / Chrome on PATH or pinned via `NEXO_PLUGIN_BROWSER_EXECUTABLE`.

## Releasing

`.github/workflows/release.yml` builds `linux-x64` + `macos-arm64` binaries
on tag push and creates a GitHub Release with the assets. Tag format:
`v0.2.x`. Workflow is dormant until the repo is published to public
`github.com/nexo-rs/plugin-browser` (follow-up
`81.17.c.publish-github`).

## Versioning

This repo's binary version is independent from `nexo-rs` daemon version.
`min_nexo_version = ">=0.1.0"` in the manifest declares the contract
floor — any daemon shipping contract v1.10.0 or later loads this plugin.

## License

MIT OR Apache-2.0.
