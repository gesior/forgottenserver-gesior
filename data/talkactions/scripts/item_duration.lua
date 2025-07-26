function onSay(player, words, param)
	if not player:getGroup():getAccess() then
		return true
	end

	local position = player:getPosition()
	position:getNextPosition(player:getDirection())

	local tile = Tile(position)
	if not tile then
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "There is no tile in front of you.")
		return false
	end

	local thing = tile:getTopVisibleThing(player)
	if not thing then
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "There is an empty tile in front of you.")
		return false
	end

    if not thing:isItem() then
        player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "Thing in front of you is not an item.")
        return false
    end

	local newDuration = tonumber(param)
	if not newDuration or newDuration < 0 then
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "Invalid duration value.")
		return false
	end

	if not thing:setAttribute(ITEM_ATTRIBUTE_DURATION, newDuration) then
		player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "Cannot set duration attribute of that item.")
		return false
	end

	player:sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, string.format("Attribute duration set to: %s", newDuration))
	position:sendMagicEffect(CONST_ME_MAGIC_GREEN)

	return false
end
