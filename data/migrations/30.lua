function onUpdateDatabase()
	print("> Updating database to version 30 (store player items in binary format)")
	db.query([[
		CREATE TABLE `player_items_binary` (
			`player_id` int NOT NULL,
			`items` longblob NOT NULL,
			PRIMARY KEY (`player_id`),
			FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
		) ENGINE=InnoDB DEFAULT CHARACTER SET=utf8;
	]])
	db.query([[
		CREATE TABLE `player_depotitems_binary` (
			`player_id` int NOT NULL,
			`items` longblob NOT NULL,
			PRIMARY KEY (`player_id`),
			FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
		) ENGINE=InnoDB DEFAULT CHARACTER SET=utf8;
	]])
	db.query([[
		CREATE TABLE `player_inboxitems_binary` (
			`player_id` int NOT NULL,
			`items` longblob NOT NULL,
			PRIMARY KEY (`player_id`),
			FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
		) ENGINE=InnoDB DEFAULT CHARACTER SET=utf8;
	]])
	db.query([[
		CREATE TABLE `player_storeinboxitems_binary` (
			`player_id` int NOT NULL,
			`items` longblob NOT NULL,
			PRIMARY KEY (`player_id`),
			FOREIGN KEY (`player_id`) REFERENCES `players` (`id`) ON DELETE CASCADE
		) ENGINE=InnoDB DEFAULT CHARACTER SET=utf8;
	]])
	return true
end
