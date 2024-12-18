<?php

class PlayerBinarySlotItems
{
    private $binaryData;
    private $position = 0;
    private $slotItems = [];
    /**
     * @var int number of player item slots
     */
    private $lastItemSlot;

    public function __construct($binaryData, $lastItemSlot = 10)
    {
        $this->binaryData = $binaryData;
        $this->lastItemSlot = $lastItemSlot;
        $this->processData();
    }

    private function getU8(): int
    {
        $ret = ord($this->binaryData[$this->position + 0]);
        $this->position += 1;

        return $ret;
    }

    private function getU16(): int
    {
        $ret = ord($this->binaryData[$this->position + 0]) +
            ord($this->binaryData[$this->position + 1]) * 256;
        $this->position += 2;

        return $ret;
    }

    private function getU32(): int
    {
        $ret = ord($this->binaryData[$this->position + 0]) +
            ord($this->binaryData[$this->position + 1]) * 256 +
            ord($this->binaryData[$this->position + 2]) * 65536 +
            ord($this->binaryData[$this->position + 3]) * 16777216;
        $this->position += 4;

        return $ret;
    }

    private function skipBytes($length): void
    {
        $this->position += $length;
    }

    public function processData(): void
    {
        $dataLength = strlen($this->binaryData);

        if ($dataLength === 0) {
            return;
        }

        $itemsFormatVersion = $this->getU8();
        if ($itemsFormatVersion !== 1) {
            throw new Exception(
                'Invalid binary items format. Expected version 1, items version: ' . $itemsFormatVersion
            );
        }

        while ($this->position < $dataLength) {
            $this->skipBytes(4); // SID
            $pid = $this->getU32();
            $itemId = $this->getU16();
            $this->skipBytes(2); // item count
            $this->skipBytes($this->getU32());

            if ($this->lastItemSlot < $pid) {
                break;
            }

            $this->slotItems[$pid] = $itemId;
        }
    }

    public function getSlotItemId(int $pid): ?int
    {
        return $this->slotItems[$pid] ?? null;
    }
}

// $SQL = PDO connection to MySQL database
$SQL = new PDO("mysql:host=localhost;dbname=tfs14", 'root', 'root');

$data = $SQL->query('SELECT * FROM player_items_binary WHERE player_id = 2')->fetch();
$equipment = new PlayerBinarySlotItems($data['items']);
for ($slotId = 0; $slotId <= 10; $slotId++) {
    if ($equipment->getSlotItemId($slotId)) {
        echo $data['player_id'] . ' - ' . $slotId . ' - ' . $equipment->getSlotItemId($slotId) . PHP_EOL;
    } else {
        echo $data['player_id'] . ' - ' . $slotId . ' - NO ITEM IN SLOT ' . PHP_EOL;
    }
}
