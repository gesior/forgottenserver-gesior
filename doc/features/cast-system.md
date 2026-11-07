# Feature: Cast system

A viewer is not a player on the server. Logging in with a cast account attaches the client to an online character and copies that character's outgoing game packets. The viewer cannot move, use items, or talk in game. The only accepted actions are logout, ping, switching casts, and `/sc`.

A character is listed after `/cast` or `/castpriv`. A `/castpriv` broadcast accepts a normal viewer only when that viewer logged into `cast` with the same password. Public viewers and admin viewers are kept separate. `players.cast_viewers` stores the viewer count for either kind of broadcast, or `-1` when the character is not broadcasting. That column is reset to `-1` on startup. Admin viewers are excluded from the count and from the viewer limits.

## Players

**Broadcast.** `/cast` toggles a public broadcast, `/cast on` starts one, and `/cast off` stops the current broadcast. `/castpriv <password>` starts a password-protected broadcast. The password is 1–32 characters. `/cast` and `/cast on` start a broadcast with no password, and only while the cast is off. While a broadcast is already on, `/cast on` and `/castpriv` report that it is already enabled, and it must be stopped with `/cast` or `/cast off` first. Stopping a cast clears the password and moves viewers to another open cast that has no password, or disconnects them if none is available. Admin viewers stay on that character.

**Watch.** Log in with the account name `cast`. An empty password can enter public casts. A password can also enter casts started with that exact password. The character list shows active casts, up to 255. Casts whose password matches the login password are listed first. Inside that group, and for every other row, the order is viewer count and then name. Each row is the player name, with a world line such as `3 viewers | Lvl 100`. A password-protected cast appends ` PASS`, for example `3 viewers | Lvl 100 PASS`. Selecting a protected cast succeeds only when the login password matches. A different password is rejected. A public cast can be selected with or without a password.

**Switch.** Arrow keys and Ctrl+arrows move to the previous or next cast the viewer may enter: every cast with no password, plus casts whose password matches the password used at login. The order is alphabetical: north and west go to the previous cast, south and east go to the next. `/sc <name>` jumps to a named broadcaster under the same rule. A different password is rejected. Full casts are skipped, and switches are limited to one every 500 ms. When a cast ends, viewers are moved only to casts with no password, even if the viewer logged in with a matching password.

A public viewer sees the map, equipment, open containers, and public channels. Guild, party, and private chat, and private messages, are hidden. The VIP list is not sent.

## Administrators

**Watch.** Log in with the account name `admincast`. The password is `castAdminPassword` from [config.lua](../../config.lua). An empty password disables admin login.

The character list is the 50 highest-level players online, not only characters that used `/cast`. The world line is `Lvl N`. Arrow keys and `/sc <name>` can still reach a player who is not on that list.

**Switch.** Arrow keys and Ctrl+arrows move through every player online, including players who are not broadcasting. The order is level from highest to lowest.Arrow switches are limited to one every 300 ms. `/sc <name>` jumps to any online player and has no cooldown.

Admin viewers ignore `castEnabled`, `castMaxViewers`, `castMaxViewersPerCast`, and cast passwords. They also see guild, party, and private channels, and private messages.

If the watched player logs out, the admin is disconnected. Closing the server disconnects public viewers and leaves admins connected. Shutdown disconnects both.

## Configuration

In `config.lua`:

- `castEnabled` — allow or block new public cast connections. Admin can always view casts.
- `castMaxViewers` — server-wide public viewer cap. `0` means only admins can watch.
- `castMaxViewersPerCast` — public viewer cap for one cast. Full casts are skipped when switching with the arrow keys.
- `castAdminPassword` — password for the `admincast` account. Empty disables admin access.

Lua: `player:setCastBroadcast(enabled)`, `player:setPrivateCastBroadcast(password)`, `player:isCastBroadcasting()`, `player:isCastPublic()`, `player:getCastViewerCount()`, and `Game.isValidCastPassword(password)`. `player:isCastPublic()` is true only for an active broadcast with no password. `CAST_PASSWORD_MAX_LENGTH` is `Casts::MAX_CAST_PASSWORD_LENGTH`. The count includes viewers of password-protected casts and excludes admins.
