# mdm — Media Download Manager

Rust download manager wrapping **yt-dlp** (1000+ video sites) and **aria2c**
(torrents, magnets, direct files) behind one local queue with a REST API,
embedded **web UI**, and a **Tauri desktop shell**.

Setup installs its own dependencies: `mdm setup` (and daemon first-run)
auto-downloads yt-dlp / aria2c / ffmpeg into `~/.local/share/mdm/bin` with
SHA-256 verification when published — no root, servers already on PATH win.

## Quick start

```sh
cargo run -q --bin mdm -- setup        # installs any missing engines (no root)
cargo run -q --bin mdm -- add "https://www.youtube.com/watch?v=jNQXAC9IVRw"
cargo run -q --bin mdm -- ls --watch   # live progress
mdm rm <short-id>

xdg-open http://localhost:5757         # web UI (same daemon)
```

`mdm add` auto-starts a detached daemon on `127.0.0.1:5757`. State lives in
`~/.local/share/mdm` (SQLite queue + logs + config.toml); files land in
`~/Downloads/mdm` by default (`output_dir` in config).

**Desktop app** (same UI, native window, own daemon):

```sh
cargo run -p mdm-app                   # opens the mdm window
```

## Layout

| Crate | Role |
| --- | --- |
| `mdm-core` | job model, SQLite queue, engine trait, manager loop |
| `mdm-engine-ytdlp` | yt-dlp subprocess driving (progress parse, kill-on-cancel) |
| `mdm-engine-aria2` | spawns aria2c `--enable-rpc`, JSON-RPC client, torrent/magnet loop |
| `mdm-bins` | auto-installer: yt-dlp (GitHub, SHA2-256SUMS), ffmpeg (johnvansickle, SHA), aria2c (P3TERX static) |
| `mdm-server` | axum REST + WS (`/api/jobs`, `/api/events`) + embedded web UI at `/` |
| `mdm-cli` | `mdm setup / add / ls / watch / pause / resume / rm / daemon / doctor` |
| `mdm-app` | Tauri 2 shell: boots the daemon in-process, window loads `http://127.0.0.1:5757` |

## API (localhost, Phase 1 — no token yet)

```sh
curl -s localhost:5757/api/health
curl -s -X POST localhost:5757/api/jobs -H 'content-type: application/json' \
     -d '{"url":"magnet:?xt=...","kind":"magnet"}'
curl -s localhost:5757/api/jobs
curl -s localhost:5757/api/config          # GET/PUT download settings
```

## Roadmap

1. **Done — Phase 1**: core queue, both engines, CLI, localhost API.
2. **Done — Phase 2**: setup auto-installs engines ✓, embedded web UI ✓,
   Tauri desktop shell ✓; remaining nice-to-haves: packaging (.deb/AppImage),
   engine auto-update.
3. Phase 3: iOS Shortcut + Android share-target posting to this API
   (LAN or Tailscale).
4. Phase 4: Android on-device (aria2c + ffmpeg as `lib*.so`; embedded Python
   for yt-dlp). iOS stays remote-control only (no subprocess spawning).

## Tests

```sh
cargo test --workspace
```
