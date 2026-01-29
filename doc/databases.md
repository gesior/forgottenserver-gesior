# Database Setup (MySQL and PostgreSQL)

This project supports both MySQL/MariaDB and PostgreSQL. The active backend is selected in `config.lua`.

## MySQL / MariaDB

1. Create a database in MySQL/MariaDB.
2. Import the schema:
   - `schema.sql`
3. In `config.lua` set:
   - `dbType = "mysql"`
   - `mysqlHost`, `mysqlUser`, `mysqlPass`, `mysqlDatabase`, `mysqlPort`, `mysqlSock`

## PostgreSQL

1. Create a database in PostgreSQL.
2. Import the schema:
   - `schema.postgres.sql`
3. In `config.lua` set:
   - `dbType = "postgres"`
   - `postgresHost`, `postgresUser`, `postgresPass`, `postgresDatabase`, `postgresPort`
   - `postgresSchema` (default: `public`)
   - `postgresSslMode` (default: `prefer`)

## Migrating from MySQL to PostgreSQL

The script below uses `pgloader` via Docker to migrate data from MySQL to PostgreSQL.

1. Ensure `schema.postgres.sql` is already applied to the PostgreSQL database.
2. Make sure Docker is installed and running.
3. Run the script:

```
pwsh tools/migrate-mysql-to-postgres.ps1 `
  -MySqlHost "127.0.0.1" -MySqlPort 3306 -MySqlDatabase "forgottenserver" `
  -MySqlUser "forgottenserver" -MySqlPassword "your_mysql_password" `
  -PgHost "127.0.0.1" -PgPort 5432 -PgDatabase "forgottenserver" `
  -PgUser "forgottenserver" -PgPassword "your_pg_password" `
  -PgSchema "public"
```

Notes:
- The script loads data only and disables triggers during import.
- If you use a custom schema, set `postgresSchema` in `config.lua` and pass the same value via `-PgSchema`.
- You can run the script from Windows PowerShell, PowerShell Core (`pwsh`), or WSL.
