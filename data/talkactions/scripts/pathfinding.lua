function teleportRight(playerId, stepsLeft)
    local player = Player(playerId)
    if not player then
        return
    end
	local playerPosition = player:getPosition()
	playerPosition.x =playerPosition.x + 1
	player:teleportTo(playerPosition, true)
	stepsLeft = stepsLeft - 1
	if stepsLeft > 0 then
		addEvent(teleportRight, 100, playerId, stepsLeft)
	end
end

function onSay(player, words, param)
    local action = tonumber(param)

    if action == 0 then
        player:teleportTo(Position(1000,1000, 7), true)
    elseif action == 1 then
		player:teleportTo(Position(1000,1135, 7), true)
		teleportRight(player:getId(), 550)
    elseif action == 2 then
		player:teleportTo(Position(1000,1170, 7), true)
		teleportRight(player:getId(), 550)
    elseif action == 3 then
		player:teleportTo(Position(1000,1205, 7), true)
		teleportRight(player:getId(), 550)
    elseif action == 4 then
		player:teleportTo(Position(1000,1240, 7), true)
		teleportRight(player:getId(), 550)
    elseif action == 5 then
        for x = 1000, 1500, 2 do
            for y = 1100, 1300, 2 do
                Game.createMonster("Orshabaal", Position(x, y, 7), false, true)
            end
        end
    end
    return false
end
