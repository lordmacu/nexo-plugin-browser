# nexo-plugin-browser

[![crates.io](https://img.shields.io/crates/v/nexo-plugin-browser.svg)](https://crates.io/crates/nexo-plugin-browser)
[![release workflow](https://github.com/lordmacu/nexo-plugin-browser/actions/workflows/release.yml/badge.svg)](https://github.com/lordmacu/nexo-plugin-browser/actions/workflows/release.yml)
[![license](https://img.shields.io/crates/l/nexo-plugin-browser.svg)](#license)

Out-of-tree subprocess plugin for the
[nexo-rs](https://github.com/lordmacu/nexo-rs) agent framework.
Drives a real Chrome / Chromium instance through the Chrome DevTools
Protocol (CDP) and exposes 12 `browser_*` tools (navigate, click,
fill, screenshot, evaluate, snapshot, scroll_to, current_url,
wait_for, go_back, go_forward, press_key) to LLM agents.

The daemon loads this binary at boot via discovery + auto-subprocess
fallback (Phase 81.17.b); tool calls round-trip over JSON-RPC stdio
through `tool.invoke` (contract v1.10.0).

## Quick start

```bash
# 1. Install the binary from crates.io.
cargo install nexo-plugin-browser

# 2. Drop the binary + manifest into a directory the daemon's
#    `plugins.discovery.search_paths` covers.
mkdir -p ~/.local/share/nexo/plugins/browser
cp "$(which nexo-plugin-browser)" ~/.local/share/nexo/plugins/browser/
curl -L -o ~/.local/share/nexo/plugins/browser/nexo-plugin.toml \
  https://raw.githubusercontent.com/lordmacu/nexo-plugin-browser/v0.2.0/nexo-plugin.toml

# 3. Tell the daemon where to look.
cat >> ~/.config/nexo/plugins.yaml <<EOF
plugins:
  discovery:
    search_paths:
      - ~/.local/share/nexo/plugins
EOF

# 4. Restart the daemon. It will spawn the binary, handshake,
#    and register the 12 browser_* tools per agent that lists
#    `plugins: [browser]` in its yaml.
nexo --config ~/.config/nexo/agents.yaml
```

## Requirements

- **Rust 1.80+** to build from source (`cargo install` handles
  this automatically when toolchain is on PATH).
- **Chromium / Chrome** on the daemon's host. Install via your
  package manager (`apt install chromium-browser` on Debian /
  Ubuntu; `brew install --cask chromium` on macOS) or pin the
  binary path with `NEXO_PLUGIN_BROWSER_EXECUTABLE`.
- **bubblewrap** (`bwrap`) on Linux for the sandbox profile —
  optional but recommended (see § Sandbox below).
- **A live nexo-rs daemon** at v0.1.x or later that supports
  contract v1.10.0 (`tool.invoke` + `extends.tools` — Phase
  81.29).

## How it fits in

```
    JSON-RPC over stdio (initialize + tool.invoke + paused/composing)
    │
nexo-rs daemon  ─────────────────────►  nexo-plugin-browser (this binary)
    │            ◄─────────────────────                  │
    │            replies / broker.event                  │
RemoteToolHandler                                  PluginAdapter
per-tool dispatch (auto-discovered                 + on_tool dispatcher
from manifest's extends.tools)                            │
                                                          ▼
                                                  BrowserPlugin
                                                  (CDP → Chrome)
```

Every LLM tool call lands in the daemon's per-agent
`ScopedToolRegistry`, hops through a `RemoteToolHandler`, and
arrives at this binary as a `tool.invoke` request. We dispatch
to `BrowserPlugin::execute(BrowserCmd::*)` which drives the CDP
session and replies with `{ ok: true, … }` (or the
`-33401..-33405` error band on failure).

## Configuration

The daemon translates its `cfg.plugins.browser` YAML into env
vars set on the subprocess at spawn time
(`proyecto/src/main.rs::seed_browser_subprocess_env`). Operators
edit only the YAML; the env-var layer is internal.

```yaml
# ~/.config/nexo/agents.yaml (or whatever your daemon reads)
plugins:
  browser:
    headless: true                  # NEXO_PLUGIN_BROWSER_HEADLESS
    executable: ""                  # NEXO_PLUGIN_BROWSER_EXECUTABLE  (empty = auto-discover)
    cdp_url: ""                     # NEXO_PLUGIN_BROWSER_CDP_URL  (empty = launch new Chrome)
    user_data_dir: ./.browser-profile  # NEXO_PLUGIN_BROWSER_USER_DATA_DIR
    window_width: 1280              # NEXO_PLUGIN_BROWSER_WINDOW_WIDTH
    window_height: 800              # NEXO_PLUGIN_BROWSER_WINDOW_HEIGHT
    connect_timeout_ms: 8000        # NEXO_PLUGIN_BROWSER_CONNECT_TIMEOUT_MS
    command_timeout_ms: 30000       # NEXO_PLUGIN_BROWSER_COMMAND_TIMEOUT_MS
    args: []                        # NEXO_PLUGIN_BROWSER_ARGS  (comma-separated CLI flags)
```

| Knob | Default | Notes |
|------|---------|-------|
| `headless` | `false` | Headless Chrome (no window). Required for headless servers. |
| `executable` | `""` | Override the Chrome binary path. Empty = search PATH for `chromium`/`chromium-browser`/`google-chrome`/`google-chrome-stable`. |
| `cdp_url` | `""` | Empty = launch a new Chrome. Set to e.g. `http://127.0.0.1:9222` to attach to an already-running Chrome with `--remote-debugging-port=9222`. |
| `user_data_dir` | `./.browser-profile` | Chrome profile directory. Persists cookies / logins between runs. Create per-agent dirs if multiple agents need isolated state. |
| `window_width` / `window_height` | `1280` / `800` | Initial viewport (px). |
| `connect_timeout_ms` | `8000` | How long to wait for Chrome / CDP handshake on startup. |
| `command_timeout_ms` | `30000` | Per-CDP-command execution timeout. |
| `args` | `[]` | Extra CLI flags forwarded verbatim to the spawned Chrome (e.g. `--no-sandbox,--disable-dev-shm-usage` on hardened containers). Ignored when `cdp_url` is set. |

## Direct env override (advanced)

Operators not running through the nexo daemon (e.g. driving the
binary from a custom orchestrator) set the env vars directly:

```bash
NEXO_PLUGIN_BROWSER_HEADLESS=true \
NEXO_PLUGIN_BROWSER_USER_DATA_DIR=/tmp/profile-x \
NEXO_PLUGIN_BROWSER_CDP_URL=http://127.0.0.1:9222 \
nexo-plugin-browser
```

The binary speaks JSON-RPC line frames over stdin / stdout per
the [nexo plugin contract v1.10.0](https://github.com/lordmacu/nexo-rs/blob/main/nexo-plugin-contract.md).

## Sandbox

The shipped `nexo-plugin.toml` enables a bubblewrap sandbox
profile (Phase 81.22):

```toml
[plugin.sandbox]
enabled = true
network = "host"            # Chromium fetches arbitrary URLs.
fs_read_paths = [
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
]
fs_write_paths = ["${state_dir}"]   # user_data_dir lives here.
drop_user = false           # Chromium needs user namespaces.
```

| Behaviour | Trigger |
|---|---|
| Sandbox enforced | `bwrap` on PATH + `enabled = true`. |
| Sandbox missing — degrade with warn | `bwrap` not installed; daemon logs `tracing::warn!`. |
| Sandbox missing — refuse to spawn | Daemon env `NEXO_PLUGIN_SANDBOX_REQUIRE=1`. |

`drop_user = false` is **required** because Chromium uses Linux
user namespaces for its own renderer sandbox. Setting it to
`true` works only with `--no-sandbox` in `args` (not recommended
in production).

Install bubblewrap:

```bash
# Debian / Ubuntu
sudo apt install bubblewrap
# Fedora
sudo dnf install bubblewrap
# macOS — bwrap is Linux-only; the sandbox degrades on Darwin.
```

## Supervisor

The shipped manifest's `[plugin.supervisor]` profile (Phase 81.21):

```toml
[plugin.supervisor]
respawn = false             # auto-respawn loop ships in 81.21.b.b.
max_attempts = 0
backoff_ms = 1000
stderr_tail_lines = 64      # last 64 stderr lines included in crash event.
```

On crash the daemon emits a `plugin.lifecycle.browser.crashed`
broker event with `{ exit_code, stderr_tail }`. Operators
recover via daemon restart until 81.21.b.b ships auto-respawn.

## Tools advertised

| Tool | Args | Behaviour |
|---|---|---|
| `browser_navigate` | `{url}` | Load URL; wait for `load`. |
| `browser_click` | `{target}` | Click ref (`@e12`) or selector. |
| `browser_fill` | `{target, value}` | Replace input value. |
| `browser_screenshot` | `{}` | Returns base64 PNG. |
| `browser_evaluate` | `{script}` | Run JS, return as JSON. |
| `browser_snapshot` | `{}` | DOM tree with `@eN` refs. |
| `browser_scroll_to` | `{target}` | Scroll element into view. |
| `browser_current_url` | `{}` | Read `location.href`. |
| `browser_wait_for` | `{selector, timeout_ms?}` | Poll until selector appears. |
| `browser_go_back` | `{}` | `history.back()`. |
| `browser_go_forward` | `{}` | `history.forward()`. |
| `browser_press_key` | `{key}` | Synthesize KeyboardEvent. |

Element refs from `browser_snapshot` are stable within a single
snapshot turn — invalidated by any subsequent DOM mutation.
Rule: snapshot, act on refs, snapshot again, repeat.

## Latency

Pre-Chrome dispatch round-trip (pure SDK overhead) on a Linux
x86_64 dev laptop:

| Tool | n | avg | p95 | p99 |
|---|---|---|---|---|
| `browser_press_key` (rejected before Chrome boot) | 200 | 164 µs | 156 µs | 4.2 ms |

Live Chrome path (full CDP roundtrip) — depends heavily on
hardware. Measure on your target with:

```bash
CHROMIUM_BIN=$(command -v chromium) cargo bench --bench tool_latency
```

## Development

```bash
git clone https://github.com/lordmacu/nexo-plugin-browser
cd nexo-plugin-browser

# Build the binary.
cargo build --release

# Run the test suite.
cargo test                                    # 23 tests, no Chromium required.
CHROMIUM_BIN=/usr/bin/chromium cargo test -- --ignored   # full live tests.

# Run the latency bench.
cargo bench --bench tool_latency

# Smoke the JSON-RPC handshake by hand.
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  | ./target/release/nexo-plugin-browser
```

The repo depends on four crates from crates.io alone — no
proyecto / nexo-rs sibling checkout required:

| Dep | Version | Purpose |
|---|---|---|
| `nexo-microapp-sdk` | `0.1.2` | `PluginAdapter`, `on_tool`, `declare_tools` |
| `nexo-broker` | `0.1.2` | `LocalBroker` for the broker bridge events |
| `nexo-cdp` | `0.1.0` | CDP `WebSocket` client + session |
| `nexo-config` | `0.1.4` | `BrowserConfig` shape |

## Releasing

`.github/workflows/release.yml` builds linux-x64 +
macos-arm64 binaries on tag push and creates a GitHub Release
with the assets. Tag format: `v0.2.x`.

```bash
# Bump version in Cargo.toml + nexo-plugin.toml.
cargo publish                                # crates.io
git tag -a v0.2.1 -m "..."
git push origin v0.2.1                       # triggers Release workflow
```

## Versioning

This crate's binary version is independent of the `nexo-rs`
daemon version. The manifest declares
`min_nexo_version = ">=0.1.0"` — any daemon shipping contract
v1.10.0 or later loads this plugin.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Daemon log: `extension tool registered ... browser_*` missing | Plugin not on a `discovery.search_paths` directory. | Verify `plugins.yaml` includes the install path. |
| Tool call replies `-33404 Unavailable` with `chromium binary not found` | Chromium not on PATH and `executable` not set. | Install Chromium or set `NEXO_PLUGIN_BROWSER_EXECUTABLE`. |
| `bwrap: cannot create directory ...` in plugin stderr | Sandbox enabled but `bwrap` lacks privileges. | `sudo apt install bubblewrap` (most distros provide a setuid binary). |
| Tool calls hang for 30 s then time out | CDP session lost; Chrome may have crashed. | Restart daemon (auto-respawn arrives in Phase 81.21.b.b). |
| Peer phone shows "escribiendo…" instead of audio | Unrelated — that's Phase 88's WhatsApp recording-presence; check `cfg.voice_mode`. | — |

## Phase 81.17.c summary

This is the **pilot extraction** of an in-tree daemon plugin
to a standalone repo + crates.io publish, validating the
subprocess + manifest + sandbox + remote-tool-handler
infrastructure end-to-end against a real binary (not a bash
mock or in-workspace template). It gates the upcoming
extraction of `plugin-telegram` (81.18) and `plugin-whatsapp`
+ `plugin-email` (81.19).

| Sub-phase | Status |
|---|---|
| 81.17.c.1 — SDK `on_tool` + `declare_tools` helpers | ✅ shipped (`nexo-microapp-sdk v0.1.2`) |
| 81.17.c.2 — Standalone repo skeleton + binary | ✅ shipped |
| 81.17.c.3 — Manifest extends.tools + sandbox + supervisor | ✅ shipped |
| 81.17.c.4 — In-tree wiring cleanup (daemon main.rs) | ✅ shipped |
| 81.17.c.5 — Docs + close-out | ✅ shipped |
| 81.17.c.publish-github | ✅ live at this URL |
| 81.17.c.crates-publish | ✅ `nexo-plugin-browser v0.2.0` |
| 81.17.c.in-tree-removal | ✅ `proyecto/crates/plugins/browser/` deleted |
| 81.17.c.e2e-test-fixture | ✅ `tests/e2e_handshake.rs` |
| 81.17.c.hot-reload-test | ✅ `tests/e2e_persistence.rs` |
| 81.17.c.latency-numbers | ✅ measured baseline in this README |
| `nexo-cdp-extract` | ✅ `nexo-cdp v0.1.0` |
| 81.17.c.multi-profile | ⬜ deferred (demand-driven) |

## License

MIT OR Apache-2.0 (same as the rest of the nexo-rs ecosystem).
