# Cam Mover Daemon (`tools/cam-mover`)

A PHP daemon (PHP 8.2+ CLI) that processes cam recordings. It automatically detects new closed recordings (`*.cam.ready`) produced by OTS, compresses them with 7-Zip LZMA2 while strictly limiting the work to a single CPU core (`-mx=1 -mmt=1`), writes metadata to the `cams` table in a MySQL database, and safely deletes the source file.

[`crash-mover.php`](crash-mover.php) recovers unfinished recordings left as `*.cam.crash` (when `camRemoveOnCrash = false`). It computes the missing end time from the last complete packet and renames each file to `*.cam.ready` so this daemon can compress and import it. Details: [Crash recovery](../../doc/features/cam-system.md#crash-recovery).

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `CAMS_SOURCE_DIR` | `cams/` | Source directory containing `*.cam.ready` files |
| `CAMS_DEST_DIR` | `cams-ready/` | Destination directory for compressed `.7z` files |
| `DB_HOST` | `127.0.0.1` | MySQL host address |
| `DB_PORT` | `3306` | MySQL port |
| `DB_NAME` | `forgottenserver` | Database name |
| `DB_USER` | `forgottenserver` | MySQL user |
| `DB_PASS` | *(empty)* | MySQL user password |
| `SEVENZIP_PATH` | `7z` | Path to the 7-Zip executable |
| `POLL_INTERVAL_US`| `100000` | Directory poll interval in microseconds (0.1s) |
| `CRASH_FILE_SLEEP_US` | `500000` | Optional. Pause in `crash-mover.php` between `*.cam.crash` files, in microseconds (0.5s). `0` disables the pause. |
| `SINGLE_RUN` | `false` | When `true`, processes the current files and then exits |

## Building and running with Docker

Commands below are run from the repository root and build this image alone. Both tools together: [../README.md](../README.md).

Build the image:
```bash
docker build -t cam-mover ./tools/cam-mover
```

Run a single test pass:
```bash
docker run --rm --name cam-mover-single --env-file .env -e SINGLE_RUN=true -v ${PWD}/cams:/srv/cams -v ${PWD}/cams-ready:/srv/camsReady cam-mover
```

Run the container:
```bash
docker run --rm --name cam-mover --env-file .env -v ${PWD}/cams:/srv/cams -v ${PWD}/cams-ready:/srv/camsReady cam-mover
```
