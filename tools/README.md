# Tools

Optional programs that sit next to the game server. The root `docker-compose.yaml` starts only the game server.

Database settings stay in the repository root `.env` (see `.env.example`). `tools/docker-compose.yaml` loads that file into every service.

## Cam Mover

A PHP daemon that picks up closed recordings (`*.cam.ready`), compresses them with 7-Zip, writes a row to the `cams` table, and deletes the source file.

[cam-mover/README.md](cam-mover/README.md)

## Cam Player

A Rust server that plays a recording back to a Tibia 10.97–10.98 client on port `7173`.

[cam-player/README.md](cam-player/README.md)

## Docker Compose

From the repository root:

```bash
docker compose -f tools/docker-compose.yaml up
```

Ctrl+C stops the containers started by that command. Compose keeps them, and the next `up` starts the same containers again.

To remove the containers Compose created:

```bash
docker compose -f tools/docker-compose.yaml down
```
