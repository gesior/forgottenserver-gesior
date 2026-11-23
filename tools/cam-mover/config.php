<?php

declare(strict_types=1);

return [
    'camsSourceDir'      => getenv('CAMS_SOURCE_DIR') ?: 'cams/',
    'camsDestinationDir' => getenv('CAMS_DEST_DIR') ?: 'cams-ready/',
    'db' => [
        'host'    => getenv('DB_HOST') ?: '127.0.0.1',
        'port'    => (int)(getenv('DB_PORT') ?: 3306),
        'name'    => getenv('DB_NAME') ?: 'forgottenserver',
        'user'    => getenv('DB_USER') ?: 'forgottenserver',
        'pass'    => getenv('DB_PASS') !== false ? getenv('DB_PASS') : '',
        'charset' => 'utf8mb4',
    ],
    'compression' => [
        'executable' => getenv('SEVENZIP_PATH') ?: '7z',
        'level'      => 1, // -mx=1 (fast LZMA2 compression)
        'threads'    => 1, // -mmt=1 (strictly 1 CPU core)
    ],
    'useDateSubdirectories'  => true,
    'pollIntervalMicrosec'   => (int)(getenv('POLL_INTERVAL_US') ?: 100000), // 0.1s
    'crashFileSleepMicrosec' => (int)(getenv('CRASH_FILE_SLEEP_US') ?: 500000), // 0.5s
    'singleRun'              => filter_var(getenv('SINGLE_RUN') ?: false, FILTER_VALIDATE_BOOLEAN),
];
