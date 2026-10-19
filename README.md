# Claude MCP Tray

![100% vibe coded](https://img.shields.io/badge/vibe%20coded-100%25-D97757?style=for-the-badge)

> **100% vibe coded.** Not a single line of this was typed by a human. I described what I wanted, Claude wrote it.

I cannot be the only person who's annoyed every time an MCP connector fails to load. You get the toast,
you open settings, you hunt through JSON, you comment something out, you quit Claude, you relaunch it… and
then you do it all again in reverse. This puts every MCP server and extension one click away in your tray,
with a toggle and a restart button.

A small tray / menu-bar app (Rust + Tauri v2) to switch Claude Desktop's MCP servers and
Desktop Extensions on and off.

## Install

Download the latest build from [Releases](https://github.com/ezascanbe/claude-mcp-tray/releases):

- **Windows:** the `.msi` or `-setup.exe`.
- **macOS (Intel + Apple Silicon):** the `.dmg`. The app isn't notarised, so on first launch right-click it
  and choose **Open**, or run `xattr -cr "/Applications/Claude MCP Tray.app"`.

## Use

- **Left-click** the tray icon to see config servers (from `claude_desktop_config.json`) and installed extensions, each with a toggle.
- Flip a toggle and a **Restart Claude to apply** prompt appears; *Restart now* quits Claude and relaunches it.
- **Right-click** for: Show MCP servers & extensions · Open config folder · View logs · Restart Claude · Quit.
  *View logs* opens Claude's log folder (`%LOCALAPPDATA%\Claude\Logs` on Windows, `~/Library/Logs/Claude` on macOS),
  where each server writes `mcp-server-<name>.log`.

## How disabling works

Claude Desktop has no "disabled" flag, so a disabled server's definition is moved out of `mcpServers`
into `claude-mcp-tray.disabled-servers.json`, next to Claude's config. Enabling moves it back.
(It lives in Claude's folder because the Windows Store build of Claude redirects other AppData
folders for processes started from inside it, which would hide the store from a normal launch.
Stores from older versions are imported automatically.) All other keys and their order are kept.
Before the first edit the original config is copied to `claude_desktop_config.mcp-tray-backup.json`.

Extensions are simpler: each has `Claude Extensions Settings/<id>.json` with an `isEnabled` flag,
which the toggle flips (other settings in that file are untouched).

Config locations checked:

| OS | Path |
|---|---|
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` (also MSIX `LocalCache` variants) |
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |

## Build

```bash
cargo build --release
```

The binary lands in `target/release/`. For installers (`.msi`/`.exe`, `.app`/`.dmg`), use the Tauri CLI:

```bash
cargo install tauri-cli --version "^2" && cargo tauri build
```

macOS builds must be made on a Mac. Releases are built by GitHub Actions
([release.yml](.github/workflows/release.yml)): push a tag like `v0.1.0` and it builds Windows and
universal macOS installers and attaches them to a GitHub release. Icons are regenerated from `assets/mcp.svg` with
`cargo run --example make_icons`. The Windows tray icon switches between white and black to match the
taskbar theme; on macOS it's a template image that the system recolours.

## Development overrides

| Env var | Effect |
|---|---|
| `CLAUDE_MCP_TRAY_CONFIG` | Use this config file instead of Claude's |
| `CLAUDE_MCP_TRAY_STORE` | Use this file for disabled-server storage |
| `CLAUDE_MCP_TRAY_SHOW` | Open the popup on launch (debug builds also keep it pinned) |
