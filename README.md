# mdm — Media Download Manager

Rust download manager wrapping **yt-dlp** (1000+ video sites) and **aria2c**
(torrents, magnets, direct files) behind one local queue with a REST API.

Phase 1 status: engine + CLI + local daemon work on Linux (Windows/macOS
should build untested; the Tauri desktop app is Phase 2).

## Quick start

```sh
# prerequisites: yt-dlp + ffmpeg on PATH (aria2c for torrents)
sudo apt install -y aria2        # torrents/magnets; otherwise video downloads still work
cargo run -q --bin mdm -- doctor # checks your setup

cargo run -q --bin mdm -- add "https://www.youtube.com/watch?v=jNQXAC9IVRw"
cargo run -q --bin mdm -- ls --watch     # live progress
mdm rm <short-id>
```

`mdm add` auto-starts a detached daemon on `127.0.0.1:5757`. State lives in
`~/.local/share/mdm` (SQLite queue + logs + config.toml); files land in
`~/Downloads/mdm` by default (`output_dir` in config).

## Layout

| Crate | Role |
| --- | --- |
| `mdm-core` | job model, SQLite queue, engine trait, manager loop |
| `mdm-engine-ytdlp` | yt-dlp subprocess driving (progress parse, kill-on-cancel) |
| `mdm-engine-aria2` | spawns aria2c `--enable-rpc`, JSON-RPC client, torrent/magnet loop |
| `mdm-server` | axum REST + WS (`/api/jobs`, `/api/events`) |
| `mdm-cli` | `mdm add / ls / watch / pause / resume / rm / daemon / doctor` |

## API (localhost, Phase 1 — no token yet)

```sh
curl -s localhost:5757/api/health
curl -s -X POST localhost:5757/api/jobs -H 'content-type: application/json' \
     -d '{"url":"magnet:?xt=...","kind":"magnet"}'
curl -s localhost:5757/api/jobs
```

## Roadmap

1. **Done — Phase 1**: core queue, both engines, CLI, localhost API.
2. Phase 2: Tauri desktop shell + web UI + auto-installed/updated engines
   (yt-dlp, aria2c, ffmpeg with SHA-256 check), pause/resume polish.
3. Phase 3: iOS Shortcut + Android share-target posting to this API
   (LAN or Tailscale).
4. Phase 4: Android on-device (aria2c + ffmpeg as `lib*.so`; embedded Python
   for yt-dlp). iOS stays remote-control only (no subprocess spawning).

## Tests

```sh
cargo test --workspace
```
