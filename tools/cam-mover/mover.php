<?php

declare(strict_types=1);

$config = require __DIR__ . '/config.php';

echo "[CamMover] Starting Cam Mover Daemon...\n";
echo "[CamMover] Source dir: " . $config['camsSourceDir'] . "\n";
echo "[CamMover] Dest dir:   " . $config['camsDestinationDir'] . "\n";
echo "[CamMover] DB host:    " . $config['db']['host'] . ":" . $config['db']['port'] . " (db: " . $config['db']['name'] . ")\n";

$running = true;
if (extension_loaded('pcntl')) {
    pcntl_async_signals(true);
    $signalHandler = static function (int $signo) use (&$running): void {
        echo "[CamMover] Received shutdown signal ($signo). Exiting...\n";
        $running = false;
    };
    pcntl_signal(SIGINT, $signalHandler);
    pcntl_signal(SIGTERM, $signalHandler);
}

/**
 * Access code for a private recording. Always contains at least one letter
 * so a login password of digits only stays a public-list page number.
 */
function generateCamAccessCode(int $length = 20): string
{
    $alphabet = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
    $letters = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ';
    $alphabetLast = strlen($alphabet) - 1;
    $lettersLast = strlen($letters) - 1;

    $accessCode = '';
    for ($i = 0; $i < $length; $i++) {
        $accessCode .= $alphabet[random_int(0, $alphabetLast)];
    }

    if (ctype_digit($accessCode)) {
        $index = random_int(0, $length - 1);
        $accessCode[$index] = $letters[random_int(0, $lettersLast)];
    }

    return $accessCode;
}

/**
 * Connect or reconnect to MySQL PDO.
 */
function getDbConnection(array $dbConfig, bool &$running): ?PDO
{
    $dsn = sprintf(
        'mysql:host=%s;port=%d;dbname=%s;charset=%s',
        $dbConfig['host'],
        $dbConfig['port'],
        $dbConfig['name'],
        $dbConfig['charset']
    );

    $options = [
        PDO::ATTR_ERRMODE            => PDO::ERRMODE_EXCEPTION,
        PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
        PDO::ATTR_TIMEOUT            => 5,
    ];

    while ($running) {
        try {
            $pdo = new PDO($dsn, $dbConfig['user'], $dbConfig['pass'], $options);
            return $running ? $pdo : null;
        } catch (PDOException $e) {
            if (!$running) {
                return null;
            }
            echo "[CamMover] DB connection failed: " . $e->getMessage() . ". Retrying in 2 seconds...\n";
            sleep(2);
        }
    }

    return null;
}

/**
 * Execute 7-zip compression.
 */
function compressFile(string $sourceFile, string $destFile, array $compressionConfig): bool
{
    $cmd = sprintf(
        '%s a -mx=%d -mmt=%d -bd -y %s %s',
        escapeshellcmd($compressionConfig['executable']),
        (int)$compressionConfig['level'],
        (int)$compressionConfig['threads'],
        escapeshellarg($destFile),
        escapeshellarg($sourceFile)
    );

    $descriptors = [
        0 => ['pipe', 'r'],
        1 => ['pipe', 'w'],
        2 => ['pipe', 'w'],
    ];

    $process = proc_open($cmd, $descriptors, $pipes);
    if (!is_resource($process)) {
        echo "[CamMover] ERROR: Failed to invoke proc_open for 7z!\n";
        return false;
    }

    fclose($pipes[0]);
    $stdout = stream_get_contents($pipes[1]);
    fclose($pipes[1]);
    $stderr = stream_get_contents($pipes[2]);
    fclose($pipes[2]);

    $exitCode = proc_close($process);
    if ($exitCode !== 0) {
        echo "[CamMover] ERROR: 7z returned exit code $exitCode. Output:\n$stdout\n$stderr\n";
        if (file_exists($destFile)) {
            @unlink($destFile);
        }
        return false;
    }

    if (!file_exists($destFile) || filesize($destFile) === 0) {
        echo "[CamMover] ERROR: 7z finished with 0 but destination file missing or empty: $destFile\n";
        if (file_exists($destFile)) {
            @unlink($destFile);
        }
        return false;
    }

    return true;
}

$pdo = getDbConnection($config['db'], $running);
if ($pdo === null || !$running) {
    echo "[CamMover] Cam Mover stopped.\n";
    exit(0);
}
$insertSql = 'INSERT INTO `cams` (
    `player_id`, `account_id`, `player_level`, `player_ip`,
    `title`, `date_start`, `date_end`, `offset_start`,
    `offset_end`, `size`, `path`, `public`, `access_code`
) VALUES (?, ?, ?, ?, \'\', ?, ?, 0, 0, ?, ?, 0, ?)';

$insertStmt = $pdo->prepare($insertSql);

$srcDir = rtrim($config['camsSourceDir'], '/\\') . DIRECTORY_SEPARATOR;
$destDir = rtrim($config['camsDestinationDir'], '/\\') . DIRECTORY_SEPARATOR;

if (!is_dir($destDir) && !mkdir($destDir, 0777, true) && !is_dir($destDir)) {
    echo "[CamMover] ERROR: Cannot create destination directory: $destDir\n";
    exit(1);
}

echo "[CamMover] Watching for *.cam.ready files in $srcDir...\n";

while ($running) {
    $pattern = $srcDir . '*.cam.ready';
    $files = glob($pattern);

    if ($files !== false && count($files) > 0) {
        foreach ($files as $file) {
            if (!$running) {
                break;
            }

            if (!is_file($file)) {
                continue;
            }

            $sourceSize = filesize($file);
            if ($sourceSize === false || $sourceSize === 0) {
                echo "[CamMover] Skipping empty file: $file\n";
                continue;
            }

            $baseName = basename($file);
            // Format: <accountId>_<playerId>_<playerLevel>_<startTimeSec>_<endTimeSec>_<ip>_<camId>_.cam.ready
            // camId is the server session id. cams.id is assigned by AUTO_INCREMENT.
            $parts = explode('_', $baseName);
            if (count($parts) < 8 || !ctype_digit($parts[6])) {
                echo "[CamMover] WARNING: Invalid file name format: $baseName. Skipping.\n";
                continue;
            }

            $accountId   = (int)$parts[0];
            $playerId    = (int)$parts[1];
            $playerLevel = (int)$parts[2];
            $dateStart   = (int)$parts[3];
            $dateEnd     = (int)$parts[4];
            $playerIp    = (int)$parts[5];

            if ($config['useDateSubdirectories']) {
                $subDir = date('Y-m-d', $dateStart > 0 ? $dateStart : time());
                $targetDir = $destDir . $subDir . DIRECTORY_SEPARATOR;
                if (!is_dir($targetDir) && !mkdir($targetDir, 0777, true) && !is_dir($targetDir)) {
                    echo "[CamMover] ERROR: Failed to create target subdirectory: $targetDir\n";
                    continue;
                }
                $relPath = $subDir . '/' . $baseName . '.7z';
                $destFile = $targetDir . $baseName . '.7z';
            } else {
                $relPath = $baseName . '.7z';
                $destFile = $destDir . $baseName . '.7z';
            }

            echo "[CamMover] Compressing: $baseName -> $relPath\n";
            $compressedOk = compressFile($file, $destFile, $config['compression']);
            if (!$compressedOk) {
                echo "[CamMover] Compression failed for $baseName, keeping source file on disk.\n";
                continue;
            }

            $compressedSize = filesize($destFile);
            if ($compressedSize === false || $compressedSize === 0) {
                echo "[CamMover] Invalid compressed file size for $destFile, keeping source file.\n";
                continue;
            }

            // Insert into MySQL
            $inserted = false;
            $duplicatePath = false;
            $camId = '0';
            $retries = 3;
            while ($retries > 0) {
                try {
                    $insertStmt->execute([
                        $playerId,
                        $accountId,
                        $playerLevel,
                        $playerIp,
                        $dateStart,
                        $dateEnd,
                        $compressedSize,
                        $relPath,
                        generateCamAccessCode(),
                    ]);
                    $camId = $pdo->lastInsertId();
                    $inserted = true;
                    break;
                } catch (PDOException $e) {
                    $driverCode = isset($e->errorInfo[1]) ? (int) $e->errorInfo[1] : 0;
                    if ($driverCode === 1062) {
                        echo "[CamMover] WARNING: Path already exists in cams ($relPath). "
                            . "Keeping the archive and the source file for an administrator.\n";
                        $duplicatePath = true;
                        break;
                    }
                    echo "[CamMover] DB Insert error: " . $e->getMessage() . ". Reconnecting PDO...\n";
                    $reconnectedPdo = getDbConnection($config['db'], $running);
                    if ($reconnectedPdo === null || !$running) {
                        break;
                    }
                    $pdo = $reconnectedPdo;
                    $insertStmt = $pdo->prepare($insertSql);
                    $retries--;
                    sleep(1);
                }
            }

            if ($inserted) {
                if (@unlink($file)) {
                    echo "[CamMover] Processed successfully: $baseName -> Cam ID #$camId, Size: $compressedSize bytes\n";
                } else {
                    echo "[CamMover] WARNING: Inserted into DB (ID #$camId), but failed to unlink source file: $file\n";
                }
            } elseif ($duplicatePath) {
                echo "[CamMover] Source file left in place: $file\n";
            } else {
                if ($running) {
                    echo "[CamMover] ERROR: DB Insert failed after retries for $baseName, keeping source file and removing archive.\n";
                } else {
                    echo "[CamMover] Shutdown requested during DB reconnect, keeping source file and removing archive.\n";
                }
                if (file_exists($destFile)) {
                    @unlink($destFile);
                }
            }
        }
    }

    if ($config['singleRun']) {
        echo "[CamMover] Single run mode complete. Exiting.\n";
        break;
    }

    usleep($config['pollIntervalMicrosec']);
}

echo "[CamMover] Cam Mover stopped.\n";
