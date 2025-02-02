function onUpdateDatabase()
	print("> Updating database to version 31 (global storage in DB)")
	db.query([[
		CREATE TABLE `global_storage` (
			`key` int(11) UNSIGNED NOT NULL,
			`value` bigint(20) NOT NULL,
			PRIMARY KEY(`key`)
		) ENGINE=InnoDB;
	]])
	return true
end
