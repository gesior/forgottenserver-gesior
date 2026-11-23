# Feature: Cam system

A recording is a copy of one character's session, written while that character is online. The game server stores the packets. [Cam Mover](../../tools/cam-mover/README.md) compresses a finished recording and inserts a private row in `cams` table. [Cam Player](../../tools/cam-player/README.md) plays that file back to a normal client. The viewer cannot move the recorded character. Playback is a stream of the stored game packets.

Cam Mover and Cam Player are optional. The root `docker-compose.yaml` starts only the game server. How to start both tools is in [tools/README.md](../../tools/README.md).

## Recording

**Start and stop.** When `camsEnabled` is on, each successful character login starts a recording. Logout or disconnect finishes it. When `camsMaxDuration` is positive, an active recording that reaches the limit is finalized and seamlessly rotated into a new recording on disk, starting with a full client-state snapshot stored as outgoing (`0x01`) packets. A session shorter than 3 seconds is deleted and never becomes a `.cam.ready` file. A login that cannot place the character cancels the recording and leaves no file.

**Packets.** The game thread only queues packets. A background thread writes them to `camsDir`. Outgoing game packets are always stored. Client-to-server packets are stored only when `camsIncomingPackets` is true, and only the part of each packet that the server parses. `camsDir` must exist and be writable at startup. A directory that fails that check keeps recording off even after a config reload.

**Limits.** The total buffer over `camsMaxTotalRam`, a stuck queue over `camsMaxQueueRam`, or a failed disk write emergency-disables recording until the server restarts. A config reload cannot turn it back on, and the recordings still held in RAM are dropped, not written. Turning recording off in a reload finishes the recordings that are still open.

**Shutdown.** The server saves the open recordings before it exits. It waits for the recorder thread only while that thread makes progress. After 30 seconds of a disk write that does not return, the process exits without it and the unsaved recordings are lost.

## Sync packets

`camsSyncInterval` is the number of seconds between client-state snapshots written into the recording. A snapshot holds the map around the player, items, fight modes, open containers, open channels, and known creatures. Those packets stay in the file. They are not sent to the live player.

Cam Player sends the latest snapshot at or before a seek target, then the outgoing packets between that snapshot and the target. `/goto`, `/back`, `/forward`, and the skip keys can then jump without replaying the file from the start. With no snapshot, the jump replays outgoing packets from the beginning of the file up to the target.

**Cost.** Generating one snapshot takes about 0.1–1 ms per player in tests. At 500 players, writing them every 30 seconds can add up to about 2% CPU on the Dispatcher thread. `camsSyncInterval` defaults to `0`, which disables snapshots.

## Cam Mover and Cam Player

**Cam Mover.** It watches `cams/` for closed `*.cam.ready` files, compresses each with 7-Zip LZMA2 on one core into `cams-ready/YYYY-MM-DD/`, and inserts a `cams` row. The table is created by [data/migrations/33.lua](../../data/migrations/33.lua). A new row is private (`public = 0`), has an empty `title`, and gets a 20-character `access_code` that always contains a letter. The letter keeps the code from being read as a public-list page number. The source file is removed only after the archive and the insert succeed.

**Own recordings.** A normal account login appends that account's newest recordings under the character list, on a world named `Cam`. That world points at `camPlayerIp`:`camPlayerPort`. An empty `camPlayerIp` uses the same IP as the game world. `camPlayerPort = 0` or `camAccountListSize = 0` adds none. Each row looks like `2026-11-12 21:00:00 Knight, 100 EK;1A3`: local start time, character name, level, vocation abbreviation, and the recording id in hex. A name longer than 25 characters is cut to 22 characters plus `...`. This list uses the character name. The account password is sent with the selection, so the owner can play a private recording from here.

**Public list.** Log in with the account name `cam` or `cams`. The list is public recordings, 50 per page, newest first, up to 255 rows. An empty password is page 1. A password of up to 9 digits is the page number. Page `0` is rejected. Any other password is an `access_code` and lists the recordings with that code. A public row uses `title` when it is not empty, in place of the character name. The same `Cam` world and Cam Player address are used. `camPlayerPort = 0` answers `Cam player is disabled.` An empty public list says there are no public recordings. An unknown access code says the recording was not found or the code is invalid.

**Playback.** Cam Player speaks the 10.97–10.98 game protocol and plays the compressed file. A public recording (`public != 0`) plays with no password. A private recording plays with that recording's `access_code`, or with the owning account's name and password. An `access_code` also opens a public recording. Commands are `/goto <seconds>`, `/forward <seconds>`, `/back <seconds>`, and `/speed <multiplier>`. `/help` prints the control list. Ctrl+Up and Ctrl+Down change speed. Ctrl+Left and Ctrl+Right skip 10 seconds. The recording stops at its end with `------- CAM FINISHED -------`.

## Website

No website is included, and no in-game command changes a recording. Without that site there is no way to mark a recording `public`, set `title`, or give an `access_code` to friends. An administrator with database access can read `access_code` from `cams` table and watch that player's recording by logging in using account name `cam` with access code as `password`.

## Configuration

In `config.lua` (defaults are in [config.lua.dist](../../config.lua.dist)):

- `camsEnabled` — record new logins. Default `false`.
- `camsDir` — directory for open and finished recordings. It must exist and be writable at startup. Default `cams/`.
- `camsIncomingPackets` — also store client-to-server packets. Default `true`.
- `camRemoveOnCrash` — delete unfinished `*.cam` files left in `camsDir` after a crash or emergency-disable on the next startup. When `false`, startup renames them to `*.cam.crash` instead. Default `true`.
- `camsBufferSize` — RAM buffer per recording, in KB, before a disk write. Default `100`.
- `camsSaveInterval` — seconds between disk writes for one recording. Default `30`.
- `camsSyncInterval` — seconds between sync snapshots. `0` disables them. Default `0`, because of the CPU cost above.
- `camsMaxDuration` — maximum duration of a single recorded cam in seconds before it closes and rotates into a new cam with a fresh start snapshot. `0` disables rotation (unlimited). Positive values are clamped to at least `10` seconds. Default `3600` (`60 * 60`).
- `camsMaxTotalRam` — total cam RAM in MB. Exceeding it emergency-disables recording until restart. Default `512`.
- `camsMaxQueueRam` — queued packet RAM in MB. Exceeding it emergency-disables recording until restart, including when storage stops answering. Default `128`.
- `camPlayerPort` — Cam Player port used on the character list and the `cam` login. `0` adds no recordings and disables the `cam` login. Default `7173`.
- `camPlayerIp` — Cam Player address. Empty uses the game world IP. Default empty.
- `camAccountListSize` — how many of this account's newest recordings to append. `0` adds none. Default `5`.

## Production configuration

`cams/` folder may be huge (X-XX GB with 60-minute limit per cam and 1000 online), so it's recommended to put it in a separate partition with a size limit/quota. Otherwise .cam files may fill up the disk and crash the server (block access to SSH, stop MySQL, etc.).

Most of big OTSes host `cam-player` app on separate VPS and configure `cam-mover` to move .cam recordings to a separate storage using `sshfs` or other methods that transfer recordings to a separate storage, when they are moved to `camsDestinationDir` folder.

## Crash recovery

Open recordings flushed to `camsDir` as `*.cam` are valid recordings up to the last disk write; after a server crash, `kill -9`, or emergency-disable, they are only missing the packets still in RAM and the final `endTimeSec` in the file name.

By default (`camRemoveOnCrash = true`), the next server startup deletes leftover `*.cam` files in `camsDir`. When `camRemoveOnCrash = false`, startup renames them to `*.cam.crash` instead. To recover those recordings, run [tools/cam-mover/crash-mover.php](../../tools/cam-mover/crash-mover.php) alongside `mover.php`: it scans `camsDir` for `*.cam.crash` files (sleeping 0.5 seconds between files by default), finds the timestamp of the last complete packet in each file to compute `endTimeSec`, and renames the file to `*.cam.ready` so `mover.php` can compress and import it into `cams` table.

## Lua and C++ API to log extra data in Cams (bot detection)

A custom data packet (`0x04`) stores the content of a `NetworkMessage` in the player's current recording. It is not sent to the player, so the client does not need to support it. When the player has no active recording, nothing is stored.

Custom packets are not used by Cam Player or any other tool in this repository, and Cam Player skips them during playback. They only add extra data to `.cam` files, so you can analyse them later in your own software, e.g. to detect cheaters. The packet format (opcodes, fields) and the analysing software are server-specific, and you have to prepare them yourself.

The feature is available from Lua (`networkMessage:addCustomDataToCam(player)`) and from C++ (`Player::addCustomDataToCam(const NetworkMessage&)`). Examples of both are below.

### Lua example - save storage to cams every 5 seconds

In Lua, `networkMessage:addCustomDataToCam(player)` stores the content of the `NetworkMessage` in the player's current recording as a custom data packet. It returns `true` when the player exists, and does nothing when that player has no active recording.

Example `data/scripts/globalevents/cam_custom_data.lua`. Every 5 seconds it adds the value of storage `123` of each online player to that player's recording (another script can keep e.g. the number of monsters killed so far in that storage):

```lua
local STORAGE_KILLS = 123
local CUSTOM_PACKET_STORAGE = 1 -- your own packet id, used later by your analysing software

local camCustomData = GlobalEvent("CamCustomData")

function camCustomData.onThink(interval)
	for _, player in ipairs(Game.getPlayers()) do
		local networkMessage = NetworkMessage()
		networkMessage:addByte(CUSTOM_PACKET_STORAGE)
		networkMessage:addU32(STORAGE_KILLS)
		networkMessage:addU32(math.max(0, player:getStorageValue(STORAGE_KILLS)))
		networkMessage:addCustomDataToCam(player)
		networkMessage:delete()
	end
	return true
end

camCustomData:interval(5000)
camCustomData:register()
```

### C++ example - track successful rune uses

The same feature is available from C++ through `Player::addCustomDataToCam(const NetworkMessage&)`. It does nothing when the player has no active recording, so no extra checks are needed. The message is only copied into the recording, so a `NetworkMessage` created on the stack is enough.

The example below is for `bool RuneSpell::executeUse(Player* player, Item* item, ...)` in `src/spells.cpp`. After the rune script succeeds, it stores the rune item ID and the content of the `var` field (`type`, and `number` or `position` depending on the type) in the recording:

```cpp
	if (!internalCastSpell(player, var, isHotkey)) {
		return false;
	}

	postCastSpell(player);

	// cam custom data: successful rune use (bot detection)
	{
		static constexpr uint8_t CUSTOM_PACKET_RUNE_USE = 2; // your own packet id, used later by your analysing software

		NetworkMessage msg;
		msg.addByte(CUSTOM_PACKET_RUNE_USE);
		msg.add<uint16_t>(item ? item->getID() : getRuneItemId()); // item can be nullptr
		msg.addByte(static_cast<uint8_t>(var.type));
		switch (var.type) {
			case VARIANT_NUMBER: // target creature ID
				msg.add<uint32_t>(var.number);
				break;

			case VARIANT_POSITION: // target position
				msg.addPosition(var.pos);
				break;

			default:
				break;
		}
		player->addCustomDataToCam(msg);
	}

	target = g_game.getCreatureByID(var.number);
	// ... rest of the function unchanged
```

Packet layout (little-endian, as everywhere in `NetworkMessage`):

| Field | Size | Description |
|-------|------|-------------|
| packet id | 1 byte | `2` - your own id of 'rune used' packet |
| item id | 2 bytes | ID of the used rune item |
| var type | 1 byte | `LuaVariantType_t`: `1` = number (creature ID), `2` = position |
| var number | 4 bytes | only when type is `1` - ID of the target creature |
| var position | 5 bytes | only when type is `2` - `x` (2 bytes), `y` (2 bytes), `z` (1 byte) |

The code is placed after `internalCastSpell` returned `true`, so only successful uses are saved. Failed checks (`playerRuneSpellCheck`) and scripts that returned `false` are not logged. Move the block before `internalCastSpell` if you want to log all attempts that passed the spell check.
