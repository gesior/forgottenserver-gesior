param(
    [string]$MySqlHost = "127.0.0.1",
    [int]$MySqlPort = 3306,
    [string]$MySqlDatabase = "forgottenserver",
    [string]$MySqlUser = "forgottenserver",
    [string]$MySqlPassword = "",
    [string]$PgHost = "127.0.0.1",
    [int]$PgPort = 5432,
    [string]$PgDatabase = "forgottenserver",
    [string]$PgUser = "forgottenserver",
    [string]$PgPassword = "",
    [string]$PgSchema = "public",
    [int]$Workers = 4,
    [int]$BatchRows = 5000
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function EncodeUriComponent {
    param([string]$Value)
    return [System.Uri]::EscapeDataString($Value)
}

$mysqlUser = EncodeUriComponent $MySqlUser
$mysqlPass = EncodeUriComponent $MySqlPassword
$pgUser = EncodeUriComponent $PgUser
$pgPass = EncodeUriComponent $PgPassword

$mysqlUri = "mysql://$mysqlUser`:$mysqlPass@$MySqlHost`:$MySqlPort/$MySqlDatabase"
$pgUri = "postgresql://$pgUser`:$pgPass@$PgHost`:$PgPort/$PgDatabase"

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "tfs-pgloader"
$loadPath = Join-Path $tempRoot "mysql-to-postgres.load"
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null

$loadContent = @"
LOAD DATABASE
     FROM $mysqlUri
     INTO $pgUri

 WITH data only,
      disable triggers,
      workers = $Workers,
      batch rows = $BatchRows;

 BEFORE LOAD DO
$$
  CREATE SCHEMA IF NOT EXISTS $PgSchema;
  SET search_path TO $PgSchema;
$$;
"@

Set-Content -Path $loadPath -Value $loadContent -Encoding ASCII

Write-Host "Running pgloader via Docker..."
Write-Host "Load file: $loadPath"
docker run --rm -v "$tempRoot:/work" -w /work dimitri/pgloader:latest pgloader /work/mysql-to-postgres.load
