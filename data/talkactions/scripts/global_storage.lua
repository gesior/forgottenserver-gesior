--[[
How to use:
- get storage value: "/globalstorage key" ex. "/globalstorage 123"
- set storage value: "/globalstorage key,value" ex. "/globalstorage 123,234"
]]--
function onSay(player, words, param)
	if not player:getGroup():getAccess() then
		return false
	end

	if player:getAccountType() < ACCOUNT_TYPE_GOD then
		return false
	end

	local split = param:splitTrimmed(",")
	local key = tonumber(split[1])
	local value = split[2]
	local valueNumber = tonumber(value)

	if not key or key < 0 then
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "First parameter 'key' is required and must be >= 0")
		return false
	end

	if valueNumber then
		Game.setStorageValue(key, valueNumber)
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, string.format("Value set to %d", valueNumber))
	elseif value then
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "Value must be a number")
	end

	player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, string.format("Key %d value is %d", key, Game.getStorageValue(key)))

	return false
end
