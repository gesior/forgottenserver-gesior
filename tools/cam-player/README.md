# Cam Player Server in Rust (`tools/cam-player`)

A high-performance, fully asynchronous cam playback server written in **Rust** (Tokio, SQLx, pure-Rust XTEA & RSA, sevenz-rust). It emulates the Tibia 10.97–10.98 `ProtocolGame` behavior on port `7173`.

## Features

1. **Tibia 10.98 protocol**:
   - Challenge packet `0x1F` (timestamp + random Adler32).
   - Login packet `0x0A` reception, RSA decryption (128 bytes, no padding, using `key.pem`).
   - Session XTEA encryption (32 rounds) and Adler-32 checksums.
2. **Authorization and MySQL**:
   - Character name parsing from the recording list: `"Date Player, Level Profession, HEX_ID"`.
   - Recording lookup in the `cams` table.
   - A recording with `public != 0` plays without a password. A private recording requires the `access_code` or the owner's password. `access_code` opens any recording.
3. **Decompression and fast-seek**:
   - On-the-fly LZMA2 decompression of `.7z` archives.
   - RAM holds a recording only while it is playing. The unpacked file on disk is cached up to `max_disk_cache_mb` (5 GB by default); the files that have gone the longest without playback are deleted.
   - Instant jump (**fast-seek**) to `offset_start` using `0x03` sync snapshots.
   - Recording cut-off at the given `offset_end`, followed by the message `------- CAM FINISHED -------`.
4. **Playback controls**:
   - Chat commands: `/goto <seconds>`, `/forward <seconds>`, `/back <seconds>`, `/speed <multiplier>`, `/help`.
   - Arrow / turn keys (North/South = speed up/slow down, East/West = jump 10s forward/backward).
   - Keepalive Ping `0x1D` -> Pong `0x1E` replies.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `LISTEN_ADDR` | `0.0.0.0:7173` | TCP listen address |
| `CAMS_DIR` | `cams-ready/` | Directory with `.7z` archives |
| `RSA_KEY_PATH` | `key.pem` | Path to the RSA private key |
| `CACHE_DIR` | `cam-cache/` | Directory for the unpacked-recording cache |
| `MAX_DISK_CACHE_MB` | `5120` | Disk cache limit in MB |
| `MAX_CONCURRENT_PLAYBACKS` | `20` | Concurrent playbacks without a secret |
| `MAX_CONCURRENT_ACCESS_CODE_PLAYBACKS` | `30` | Concurrent playbacks when the password is the recording's `access_code` |
| `DB_HOST` | `127.0.0.1` | MySQL host |
| `DB_PORT` | `3306` | MySQL port |
| `DB_NAME` | `forgottenserver` | Database name |
| `DB_USER` | `forgottenserver` | Database user |
| `DB_PASS` | *(empty)* | Database password |

## Building and running with Docker

Commands below are run from the repository root and build this image alone. Both tools together: [../README.md](../README.md).

Build the image:

```bash
docker build -t cam-player ./tools/cam-player
```

Run the container:

```bash
docker run --rm --name cam-player --env-file .env -p 7173:7173 -v ${PWD}/cams-ready:/srv/camsReady:ro -v ${PWD}/key.pem:/srv/key.pem:ro cam-player
```
