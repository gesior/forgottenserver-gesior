function onUpdateDatabase()
	print("> Updating database to version 33 (Cam System)")
	db.query([[
		CREATE TABLE IF NOT EXISTS `cams` (
			`id` INT(10) UNSIGNED NOT NULL AUTO_INCREMENT,
			`player_id` INT(10) UNSIGNED NOT NULL,
			`account_id` INT(10) UNSIGNED NOT NULL,
			`player_level` INT(10) UNSIGNED NOT NULL,
			`player_ip` INT(10) UNSIGNED NOT NULL,
			`title` VARCHAR(25) NOT NULL DEFAULT '',
			`date_start` INT(10) UNSIGNED NOT NULL,
			`date_end` INT(10) UNSIGNED NOT NULL,
			`offset_start` INT(10) NOT NULL DEFAULT 0,
			`offset_end` INT(10) NOT NULL DEFAULT 0,
			`size` INT(10) UNSIGNED NOT NULL,
			`path` VARCHAR(255) NOT NULL,
			`public` TINYINT(1) NOT NULL DEFAULT 0,
			`access_code` VARCHAR(20) NOT NULL DEFAULT '' COLLATE utf8mb4_bin,
			PRIMARY KEY (`id`),
			UNIQUE KEY `uniq_path` (`path`),
			KEY `idx_account_date` (`account_id`, `date_start` DESC, `id` DESC),
			KEY `idx_player_id` (`player_id`),
			KEY `idx_public_date` (`public`, `date_start` DESC, `id` DESC),
			KEY `idx_date_start` (`date_start` DESC),
			KEY `idx_access_code` (`access_code`, `date_start` DESC, `id` DESC)
		) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
	]])

	return true
end
