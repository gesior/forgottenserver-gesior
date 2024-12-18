<?php

class PlayerBinaryItems
{
    private $binaryData;
    private $position = 0;
    /**
     * @var array items are indexed by SID, not PID, you cant read slot items by using 1-10 IDs
     */
    private $items = [];

    public function __construct($binaryData)
    {
        $this->binaryData = $binaryData;
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

    private function getBytes(int $length): string
    {
        $ret = substr($this->binaryData, 0, $length);
        $this->position += $length;

        return $ret;
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
            $sid = $this->getU32();
            $pid = $this->getU32();
            $itemId = $this->getU16();
            $itemCount = $this->getU16();
            $attributesSize = $this->getU32();
            $attributes = $this->getBytes($attributesSize);

            $this->items[$sid] = [
                'sid' => $sid,
                'pid' => $pid,
                'itemId' => $itemId,
                'itemCount' => $itemCount,
                'attributes' => $attributes,
            ];
        }
    }

    public function getItems(): array
    {
        return $this->items;
    }

    public function getItem(int $sid): ?array
    {
        return $this->items[$sid] ?? null;
    }
}

// $SQL = PDO connection to MySQL database
$SQL = new PDO("mysql:host=localhost;dbname=tfs14", 'root', 'root');

$data = $SQL->query('SELECT * FROM player_items_binary WHERE player_id = 2')->fetch();
$allItems = new PlayerBinaryItems($data['items']);
$equipment = [];
foreach ($allItems->getItems() as $item) {
    if ($item['pid'] <= 10) {
        $equipment[$item['pid']] = $item['itemId'];
    }
}

for ($slotId = 0; $slotId <= 10; $slotId++) {
    if (isset($equipment[$slotId])) {
        echo $data['player_id'] . ' - ' . $slotId . ' - ' . $equipment[$slotId] . PHP_EOL;
    } else {
        echo $data['player_id'] . ' - ' . $slotId . ' - NO ITEM IN SLOT ' . PHP_EOL;
    }
}
