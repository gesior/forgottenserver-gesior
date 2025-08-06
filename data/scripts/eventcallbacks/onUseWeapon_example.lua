-- Example of using Player:onUseWeapon(item) event
-- This event is called when a player attacks using a weapon

-- Register callback for onUseWeapon event
EventCallback.onUseWeapon = function(player, item)
    -- Check if player is using a specific weapon
    if item:getId() == 2400 then -- Sword ID
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are using a sword!")
    elseif item:getId() == 2456 then -- Axe ID
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are using an axe!")
    elseif item:getId() == 2383 then -- Bow ID
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are shooting with a bow!")
    else
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are using a weapon: " .. item:getName())
    end
    
    -- You can add additional logic here, e.g.:
    -- - Check if player has appropriate skills
    -- - Add special effects
    -- - Log weapon usage
    -- - Check if weapon is not broken
    
    return true -- Return true to continue normal operation
end

-- Register callback
EventCallback:register()