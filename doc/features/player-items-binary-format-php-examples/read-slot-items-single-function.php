<?php

function parseBinarySlotItems($binaryData, $lastItemSlot = 10)
{
    if (empty($binaryData)) {
        return [];
    }

    if (ord($binaryData[0]) !== 1) {
        throw new Exception('Invalid binary items format. Expected version 1, items version: ' . ord($binaryData[0]));
    }

    $position = 1;
    $slotItems = [];
    $dataLength = strlen($binaryData);

    while ($position < $dataLength) {
        $position += 4; // SID
        $pid = ord($binaryData[$position + 0]) +
            ord($binaryData[$position + 1]) * 256 +
            ord($binaryData[$position + 2]) * 65536 +
            ord($binaryData[$position + 3]) * 16777216;
        $position += 4; // PID length
        $itemId = ord($binaryData[$position + 0]) +
            ord($binaryData[$position + 1]) * 256;
        $position += 2; // item ID length
        $position += 2; // item count
        $attributesLength = ord($binaryData[$position + 0]) +
            ord($binaryData[$position + 1]) * 256 +
            ord($binaryData[$position + 2]) * 65536 +
            ord($binaryData[$position + 3]) * 16777216;
        $position += 4; // attributes size length
        $position += $attributesLength; // attributes bytes

        if ($lastItemSlot < $pid) {
            break;
        }

        $slotItems[$pid] = $itemId;
    }

    return $slotItems;
}

// $SQL = PDO connection to MySQL database
$SQL = new PDO("mysql:host=localhost;dbname=tfs14", 'root', 'root');

$data = $SQL->query('SELECT * FROM player_items_binary WHERE player_id = 2')->fetch();
$equipment = parseBinarySlotItems($data['items']);
for ($slotId = 0; $slotId <= 10; $slotId++) {
    if (isset($equipment[$slotId])) {
        echo $data['player_id'] . ' - ' . $slotId . ' - ' . $equipment[$slotId] . PHP_EOL;
    } else {
        echo $data['player_id'] . ' - ' . $slotId . ' - NO ITEM IN SLOT ' . PHP_EOL;
    }
}
