# Player:onUseWeapon Event

## Description
The `Player:onUseWeapon(item)` event is triggered when a player attacks using a weapon. The event is called in the `Player::doAttacking()` function in the `player.cpp` file.

## Parameters
- `player` - The player object who is using the weapon
- `item` - The weapon object (Item) that the player is using

## Usage

### Basic usage
```lua
EventCallback.onUseWeapon = function(player, item)
    player:sendTextMessage(MESSAGE_INFO_DESCR, "You are using weapon: " .. item:getName())
    return true
end

EventCallback:register()
```

### Checking for specific weapons
```lua
EventCallback.onUseWeapon = function(player, item)
    if item:getId() == 2400 then -- Sword
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are using a sword!")
    elseif item:getId() == 2456 then -- Axe
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are using an axe!")
    elseif item:getId() == 2383 then -- Bow
        player:sendTextMessage(MESSAGE_INFO_DESCR, "You are shooting with a bow!")
    end
    return true
end

EventCallback:register()
```

### Adding special effects
```lua
EventCallback.onUseWeapon = function(player, item)
    -- Check if the weapon has special properties
    if item:getAttribute(ITEM_ATTRIBUTE_CHARGES) > 0 then
        player:sendTextMessage(MESSAGE_INFO_DESCR, "Weapon has " .. item:getAttribute(ITEM_ATTRIBUTE_CHARGES) .. " charges!")
    end
    
    -- Add sound effect
    player:getPosition():sendMagicEffect(CONST_ME_MAGIC_BLUE)
    
    return true
end

EventCallback:register()
```

### Logging weapon usage
```lua
EventCallback.onUseWeapon = function(player, item)
    -- Save weapon usage log
    local logMessage = string.format(
        "Player %s used weapon %s (ID: %d) at position %s",
        player:getName(),
        item:getName(),
        item:getId(),
        player:getPosition():toString()
    )
    
    -- Here you can add code to save the log to a file or database
    print(logMessage)
    
    return true
end

EventCallback:register()
```

## Important notes

1. **Event is only triggered for weapons** - The event is not triggered when a player attacks with fists (without a weapon).

2. **Call position** - The event is triggered before the actual weapon usage, so you can add logic here that may affect the attack.

3. **Return value** - The function should return `true` to continue normal operation, or `false` to interrupt the attack (although in the current implementation the return value is not checked).

4. **Performance** - The event is triggered on every attack, so avoid heavy operations in this callback.

## Technical implementation

The event is implemented in the following files:
- `src/events.h` - Event declaration
- `src/events.cpp` - Event implementation
- `src/player.cpp` - Event call in the `doAttacking()` function
- `data/events/events.xml` - Event configuration
- `data/events/scripts/player.lua` - Lua implementation
- `data/scripts/lib/event_callbacks.lua` - Callback constant definition

## Usage examples

- **Weapon durability system** - Checking and reducing weapon durability
- **Special effects** - Adding visual or sound effects
- **Logging** - Saving information about weapon usage
- **Skill system** - Checking if the player has appropriate skills to use the weapon
- **Quest system** - Checking if using a specific weapon is part of a quest 