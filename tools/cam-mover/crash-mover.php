<?php

declare(strict_types=1);

/**
 * Recovers leftover *.cam.crash files into *.cam.ready for mover.php.
 * Full description: doc/features/cam-system.md#crash-recovery
 */

$config = require __DIR__ . '/config.php';
$sleepMicrosec = max(0, (int)($config['crashFileSleepMicrosec'] ?? 500000));

echo "[CrashMover] Starting Cam Crash Mover...\n";
echo "[CrashMover] Source dir: " . $config['camsSourceDir'] . "\n";

$running = true;
if (extension_loaded('pcntl')) {
    pcntl_async_signals(true);
    $signalHandler = static function (int $signo) use (&$running): void {
        echo "[CrashMover] Received shutdown signal ($signo). Exiting...\n";
        $running = false;
    };
    pcntl_signal(SIGINT, $signalHandler);
    pcntl_signal(SIGTERM, $signalHandler);
}

/**
 * Scans a .cam.crash file, finds the timestamp of the last complete packet,
 * and returns endTimeSec (or null if the file header is invalid or has no complete packets).
 */
function findLastPacketTimeSec(string $filePath, int $fallbackStartTimeSec): ?int
{
    $fp = @fopen($filePath, 'rb');
    if ($fp === false) {
        return null;
    }

    $stat = fstat($fp);
    $fileSize = $stat !== false ? (int)$stat['size'] : 0;
    if ($fileSize < 25) {
        fclose($fp);
        return null;
    }

    $header = fread($fp, 15);
    if ($header === false || strlen($header) < 15 || substr($header, 0, 8) !== 'TIBIACAM' || ord($header[8]) !== 1) {
        fclose($fp);
        return null;
    }

    $nameLength = (int)unpack('vlen', substr($header, 13, 2))['len'];
    $packetsOffset = 25 + $nameLength;
    if ($fileSize < $packetsOffset) {
        fclose($fp);
        return null;
    }

    $headerRest = fread($fp, $nameLength + 10);
    if ($headerRest === false || strlen($headerRest) < $nameLength + 10) {
        fclose($fp);
        return null;
    }

    $startTimeMs = (int)unpack('Pms', substr($headerRest, $nameLength + 2, 8))['ms'];
    $lastPacketMs = $startTimeMs;
    $packetCount = 0;
    $offset = $packetsOffset;

    while ($offset + 10 <= $fileSize) {
        $pktHeader = fread($fp, 10);
        if ($pktHeader === false || strlen($pktHeader) < 10) {
            break;
        }

        $unpacked = unpack('PtimestampMs/vpacketSize', $pktHeader);
        $timestampMs = (int)$unpacked['timestampMs'];
        $packetSize = (int)$unpacked['packetSize'];

        if ($packetSize <= 0 || $offset + 10 + $packetSize > $fileSize) {
            // Incomplete or truncated packet at EOF
            break;
        }

        $offset += 10 + $packetSize;
        if (fseek($fp, $offset, SEEK_SET) !== 0) {
            break;
        }

        if ($timestampMs > $lastPacketMs) {
            $lastPacketMs = $timestampMs;
        }
        $packetCount++;
    }

    fclose($fp);

    if ($packetCount === 0) {
        return null;
    }

    $lastPacketSec = intdiv($lastPacketMs, 1000);
    return max($fallbackStartTimeSec, $lastPacketSec);
}

$srcDir = rtrim($config['camsSourceDir'], '/\\') . DIRECTORY_SEPARATOR;

echo "[CrashMover] Watching for *.cam.crash files in $srcDir...\n";

while ($running) {
    $pattern = $srcDir . '*.cam.crash';
    $files = glob($pattern);

    if ($files !== false && count($files) > 0) {
        foreach ($files as $file) {
            if (!$running) {
                break;
            }

            if (!is_file($file)) {
                continue;
            }

            $baseName = basename($file);
            // Format: <accountId>_<playerId>_<playerLevel>_<startTimeSec>_<ip>_<camId>_.cam.crash
            $parts = explode('_', $baseName);
            if (count($parts) < 7 || !ctype_digit($parts[5])) {
                echo "[CrashMover] WARNING: Invalid file name format: $baseName. Skipping.\n";
                if ($running && !$config['singleRun'] && $sleepMicrosec > 0) {
                    usleep($sleepMicrosec);
                }
                continue;
            }

            $accountId   = (int)$parts[0];
            $playerId    = (int)$parts[1];
            $playerLevel = (int)$parts[2];
            $dateStart   = (int)$parts[3];
            $playerIp    = (int)$parts[4];
            $camId       = $parts[5];

            $dateEnd = findLastPacketTimeSec($file, $dateStart);
            if ($dateEnd === null) {
                echo "[CrashMover] WARNING: Corrupted or empty crash file: $baseName. Removing.\n";
                @unlink($file);
                if ($running && !$config['singleRun'] && $sleepMicrosec > 0) {
                    usleep($sleepMicrosec);
                }
                continue;
            }

            $readyName = sprintf(
                '%d_%d_%d_%d_%d_%d_%s_.cam.ready',
                $accountId,
                $playerId,
                $playerLevel,
                $dateStart,
                $dateEnd,
                $playerIp,
                $camId
            );
            $readyFile = $srcDir . $readyName;

            if (@rename($file, $readyFile)) {
                echo "[CrashMover] Recovered: $baseName -> $readyName\n";
            } else {
                echo "[CrashMover] ERROR: Failed to rename $baseName to $readyName\n";
            }

            if ($running && !$config['singleRun'] && $sleepMicrosec > 0) {
                usleep($sleepMicrosec);
            }
        }
    } else {
        if ($config['singleRun']) {
            echo "[CrashMover] Single run mode complete. Exiting.\n";
            break;
        }

        if ($sleepMicrosec > 0) {
            usleep($sleepMicrosec);
        }
    }

    if ($config['singleRun']) {
        echo "[CrashMover] Single run mode complete. Exiting.\n";
        break;
    }
}

echo "[CrashMover] Cam Crash Mover stopped.\n";
