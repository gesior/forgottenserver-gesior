# Feature: Cast system

A viewer is not a player on the server. Logging in with a cast account attaches the client to an online character and copies that character's outgoing game packets. The viewer cannot move, use items, or talk in game. The only accepted actions are logout, ping, switching casts, and `/sc`.

A character is public only after `/cast`. Public viewers and admin viewers are kept separate. `players.cast_viewers` stores the public viewer count, or `-1` when the character is not broadcasting. That column is reset to `-1` on startup. Admin viewers are excluded from the count and from the viewer limits.

## Players

**Broadcast.** `/cast` toggles broadcasting, `/cast on` starts it, and `/cast off` stops it. Stopping a cast moves public viewers to another open cast, or disconnects them if none is available. Admin viewers stay on that character.

**Watch.** Log in with the account name `cast` and leave the password empty. The character list shows active casts, up to 255, sorted by viewer count and then by name. Each row is the player name, with a world line such as `3 viewers | Lvl 100`.

**Switch.** Arrow keys and Ctrl+arrows move to the previous or next open cast. The order is alphabetical: north and west go to the previous cast, south and east go to the next. `/sc <name>` jumps to a named broadcaster. Full casts are skipped, and switches are limited to one every 500 ms.

A public viewer sees the map, equipment, open containers, and public channels. Guild, party, and private chat, and private messages, are hidden. The VIP list is not sent.

## Administrators

Log in with the account name `admincast`. The password is `castAdminPassword` from [config.lua.dist](../../config.lua.dist). An empty password disables admin login.

The character list is the 50 highest-level players online, not only characters that used `/cast`. The world line is `Lvl N`. Arrow keys and `/sc <name>` can still reach a player who is not on that list.

**Switch.** Arrow keys and Ctrl+arrows move through every player online, including players who are not broadcasting. The order is level from highest to lowest.Arrow switches are limited to one every 300 ms. `/sc <name>` jumps to any online player and has no cooldown.

Admin viewers ignore `castEnabled`, `castMaxViewers`, and `castMaxViewersPerCast`. They also see guild, party, and private channels, and private messages.

If the watched player logs out, the admin is disconnected. Closing the server disconnects public viewers and leaves admins connected. Shutdown disconnects both.

## Configuration

In `config.lua`:

- `castEnabled` — allow or block new public cast connections. Admin can always view casts.
- `castMaxViewers` — server-wide public viewer cap. `0` means only admins can watch.
- `castMaxViewersPerCast` — public viewer cap for one cast. Full casts are skipped when switching with the arrow keys.
- `castAdminPassword` — password for the `admincast` account. Empty disables admin access.

Lua: `player:setCastBroadcast(enabled)`, `player:isCastBroadcasting()`, and `player:getCastViewerCount()`. The count is public viewers only.
