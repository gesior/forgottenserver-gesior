local function trim(value)
	return (value:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function enableCast(player)
	if player:isCastBroadcasting() then
		player:sendCancelMessage("Your cast broadcast is already enabled. Use /cast off before starting it again.")
		return false
	end

	if not configManager.getBoolean(configKeys.CAST_ENABLED) then
		player:sendCancelMessage("Cast system is currently disabled.")
		return false
	end

	if not player:setCastBroadcast(true) then
		player:sendCancelMessage("Cast system is currently unavailable.")
		return false
	end

	player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "You have started broadcasting your gameplay.")
	player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "Use /castpriv <password> to start a private cast with a password.")
	return false
end

local function disableCast(player)
	if not player:isCastBroadcasting() then
		player:sendCancelMessage("Your cast broadcast is already disabled.")
		return false
	end

	player:setCastBroadcast(false)
	player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "You have stopped broadcasting your gameplay.")
	return false
end

local function enablePrivateCast(player, param)
	if player:isCastBroadcasting() then
		player:sendCancelMessage("Your cast broadcast is already enabled. Use /cast off before starting it again.")
		return false
	end

	if not configManager.getBoolean(configKeys.CAST_ENABLED) then
		player:sendCancelMessage("Cast system is currently disabled.")
		return false
	end

	local password = trim(param)
	if not Game.isValidCastPassword(password) then
		player:sendCancelMessage("Cast password must be 1-" .. CAST_PASSWORD_MAX_LENGTH .. " characters.")
		return false
	end

	if not player:setPrivateCastBroadcast(password) then
		player:sendCancelMessage("Cast system is currently unavailable.")
		return false
	end

	player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "You have started a private broadcast. Viewers must log in to the cast account with your password.")
	return false
end

function onSay(player, words, param)
	if words == "/castpriv" or words == "!castpriv" then
		return enablePrivateCast(player, param)
	end

	if param == "" then
		if player:isCastBroadcasting() then
			return disableCast(player)
		else
			return enableCast(player)
		end
	elseif param == "on" then
		return enableCast(player)
	elseif param == "off" then
		return disableCast(player)
	else
		player:sendCancelMessage("Usage: /cast, /cast on or /cast off. Use /castpriv <password> to start a private cast with a password.")
		return false
	end
end
