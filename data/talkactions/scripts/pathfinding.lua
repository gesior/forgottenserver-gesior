local positions = {}

local teleportInterval = 10000
local teleportDistance = 30
local walkRange = 7
local monsterSpawnDistance = 6
local playerName = 'Druid Sample'
local monsterName = 'Troll'
local wallItemId = 389

local minX, maxX, minY, maxY = 1000, 5500, 1000, 4300
for x = minX, maxX, teleportDistance do
    for y = minY, maxY, teleportDistance do
        table.insert(positions, Position(x, y, 7))
    end
end

function delayedGoDown(playerId)
    local player = Player(playerId)
    if not player then
        return
    end
    local playerPosition = player:getPosition()
    playerPosition.y = playerPosition.y + 1
    player:teleportTo(playerPosition, true)
end

function delayedGoUp(playerId)
    local player = Player(playerId)
    if not player then
        return
    end
    local playerPosition = player:getPosition()
    playerPosition.y = playerPosition.y - 1
    player:teleportTo(playerPosition, true)
end

function delayedTp(playerId, positionId)
    local player = Player(playerId)
    if not player then
        return
    end
    --print('position ID', player:getId(), positionId, #positions)
    local playerPosition = positions[positionId]
    player:teleportTo(playerPosition)
    for i = 1, walkRange * 2 do
        if i < walkRange then
            --print('delayedGoUp', (teleportInterval / (walkRange * 2 + 1)) * i, playerId)
            addEvent(delayedGoUp, (teleportInterval / (walkRange * 2 + 1)) * i, playerId)
        else
            --print('delayedGoDown', (teleportInterval / (walkRange * 2 + 1)) * i, playerId)
            addEvent(delayedGoDown, (teleportInterval / (walkRange * 2 + 1)) * i, playerId)
        end
    end

end

local teleportEvent = 0
function startLoop(steps)
    print('startLoop', os.time())
    local players = Game.getPlayers()
    for i, player in pairs(players) do
        if player:getName() == playerName then
            local positionId = ((player:getId() + steps) % #positions) + 1
            delayedTp(player:getId(), positionId)
        end
    end

    stopEvent(teleportEvent)
    teleportEvent = addEvent(startLoop, teleportInterval, steps + #players)
end

function onSay(player, words, param)
    if param == 'run' then
        startLoop(2000)
    elseif param == 'wall7' then
        -- 7 sqm walls
        for x = minX - monsterSpawnDistance * 2, maxX + monsterSpawnDistance * 2 do
            for y = minY - monsterSpawnDistance * 2, maxY + monsterSpawnDistance * 2 do
                if x % 3 == 0 and y % 7 ~= 0 then
                    Game.createItem(wallItemId, 1, Position(x,y, 7))
                end
            end
        end
    elseif param == 'wall15' then
        -- 15 sqm walls
        for x = minX - monsterSpawnDistance * 2, maxX + monsterSpawnDistance * 2 do
            for y = minY - monsterSpawnDistance * 2, maxY + monsterSpawnDistance * 2 do
                if x % 3 == 0 and y % 15 ~= 0 then
                    Game.createItem(wallItemId, 1, Position(x,y, 7))
                end
            end
        end
    elseif param == 'checker' then
        -- checker pattern
        for x = minX - monsterSpawnDistance * 2, maxX + monsterSpawnDistance * 2 do
            for y = minY - monsterSpawnDistance * 2, maxY + monsterSpawnDistance * 2 do
                if y % 3 ~= 0 and x % 2 ~= y % 2 then
                    Game.createItem(wallItemId, 1, Position(x,y, 7))
                end
            end
        end
    elseif param == 'troll' then
        for x = minX - monsterSpawnDistance * 2, maxX + monsterSpawnDistance * 2, monsterSpawnDistance do
            for y = minY - monsterSpawnDistance * 2, maxY + monsterSpawnDistance * 2, monsterSpawnDistance do
                Game.createMonster('Troll', Position(x, y, 7), false, true)
            end
        end
    elseif param == 'demon' then
        for x = minX - monsterSpawnDistance * 2, maxX + monsterSpawnDistance * 2, monsterSpawnDistance do
            for y = minY - monsterSpawnDistance * 2, maxY + monsterSpawnDistance * 2, monsterSpawnDistance do
                Game.createMonster('Demon', Position(x, y, 7), false, true)
            end
        end
    elseif param == 'orsh' then
        for x = minX - monsterSpawnDistance * 2, maxX + monsterSpawnDistance * 2, monsterSpawnDistance do
            for y = minY - monsterSpawnDistance * 2, maxY + monsterSpawnDistance * 2, monsterSpawnDistance do
                Game.createMonster('Orshabaal', Position(x, y, 7), false, true)
            end
        end
    end
    return false
end
