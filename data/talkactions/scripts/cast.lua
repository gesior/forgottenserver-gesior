local function enableCast(player)
	if player:isCastBroadcasting() then
		player:sendCancelMessage("Your cast broadcast is already enabled.")
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

function onSay(player, words, param)
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
		player:sendCancelMessage("Usage: /cast, /cast on or /cast off.")
		return false
	end
end
