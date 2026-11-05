function onUpdateDatabase()
	print("> Updating database to version 32 (Cast System cast_viewers)")
	db.query("ALTER TABLE `players` ADD `cast_viewers` int(10) NOT NULL DEFAULT '-1'");
	return true
end
