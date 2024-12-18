# Feature: Player items binary format

This feature stores player items in database in binary format instead of relational.
It speeds up players load and save time 3-40 times.

### Binary format description

First byte (`uint8_t`) is `0x01` - it's format version for future compatibility.

Next bytes are items stored in format similar to columns in relational tables:
- `uint32_t` - serialize ID (`sid`) - unique ID of serialized item
- `uint32_t` - parent ID (`pid`) - points to parent element `sid`, use it to restore child-parent tree of containers
- `uint16_t` - item ID (`itemtype`)
- `uint16_t` - item count (`count`)
- `uint32_t` - attributes data length
- bytes[] - attributes data (`attributes`)

### PHP examples

In folder [player-items-binary-format-php-examples](player-items-binary-format-php-examples) are example codes that read items from binary format:
- read player EQ slots items using single PHP function: [read-slot-items-single-function.php](player-items-binary-format-php-examples/read-slot-items-single-function.php)
- read player EQ slots items using simple PHP class: [read-slot-items-class.php](player-items-binary-format-php-examples/read-slot-items-class.php)
- read all player items using PHP class and filter EQ slots: [read-all-items-class.php](player-items-binary-format-php-examples/read-all-items-class.php)
