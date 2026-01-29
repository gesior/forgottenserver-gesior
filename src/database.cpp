// Copyright 2023 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#include "otpch.h"

#include "configmanager.h"
#include "database.h"
#ifdef STATS_ENABLED
#include "stats.h"
#endif

#include <libpq-fe.h>
#include <mysql/errmsg.h>

extern ConfigManager g_config;

void tfs::detail::PgConnDeleter::operator()(PGconn* handle) const
{
	if (handle) {
		PQfinish(handle);
	}
}

void tfs::detail::PgResultDeleter::operator()(PGresult* handle) const
{
	if (handle) {
		PQclear(handle);
	}
}

static std::string toLowerString(std::string value)
{
	std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
		return static_cast<char>(std::tolower(ch));
	});
	return value;
}

static DatabaseBackend getDatabaseBackendFromConfig()
{
	std::string type = toLowerString(g_config.getString(ConfigManager::DB_TYPE));
	if (type == "postgres" || type == "postgresql") {
		return DatabaseBackend::Postgres;
	}
	if (type != "mysql" && !type.empty()) {
		std::cout << "[Warning - Database] Unknown dbType '" << type << "', falling back to mysql." << std::endl;
	}
	return DatabaseBackend::Mysql;
}

static std::string escapeConninfoValue(std::string_view value)
{
	std::string escaped;
	escaped.reserve(value.size());
	for (char ch : value) {
		if (ch == '\\' || ch == '\'') {
			escaped.push_back('\\');
		}
		escaped.push_back(ch);
	}
	return escaped;
}

static tfs::detail::Mysql_ptr connectToMysql(const bool retryIfError)
{
	bool isFirstAttemptToConnect = true;

	retry:
	if (!isFirstAttemptToConnect) {
		std::this_thread::sleep_for(std::chrono::seconds(1));
	}
	isFirstAttemptToConnect = false;

// MariaDB requires explicit SSL settings to avoid the following error:
// "SSL is required, but the server does not support it"
// For more details see issue #4954 ( https://github.com/otland/forgottenserver/issues/4954 )
#ifdef MARIADB_VERSION_ID
	// this needs to be above "goto" otherwise it won't build
	bool ssl_enforce = false;
	bool ssl_verify = false;
#endif

	tfs::detail::Mysql_ptr handle{mysql_init(nullptr)};
	if (!handle) {
		std::cout << std::endl << "Failed to initialize MySQL connection handle." << std::endl;
		goto error;
	}

// MariaDB explicit SSL settings continued
#ifdef MARIADB_VERSION_ID
	mysql_options(handle.get(), MYSQL_OPT_SSL_ENFORCE, &ssl_enforce);
	mysql_options(handle.get(), MYSQL_OPT_SSL_VERIFY_SERVER_CERT, &ssl_verify);
	mysql_ssl_set(handle.get(), nullptr, nullptr, nullptr, nullptr, nullptr);
#endif

	// connects to database
	if (!mysql_real_connect(handle.get(), g_config.getString(ConfigManager::MYSQL_HOST).c_str(),
							g_config.getString(ConfigManager::MYSQL_USER).c_str(), g_config.getString(ConfigManager::MYSQL_PASS).c_str(),
							g_config.getString(ConfigManager::MYSQL_DB).c_str(), g_config.getNumber(ConfigManager::SQL_PORT),
							g_config.getString(ConfigManager::MYSQL_SOCK).c_str(), 0)) {
		std::cout << std::endl << "MySQL Error Message: " << mysql_error(handle.get()) << std::endl;
		goto error;
	}
	return handle;

	error:
	if (retryIfError) {
		goto retry;
	}
	return nullptr;
}

static tfs::detail::PgConn_ptr connectToPostgres(const bool retryIfError)
{
	bool isFirstAttemptToConnect = true;

	retry:
	if (!isFirstAttemptToConnect) {
		std::this_thread::sleep_for(std::chrono::seconds(1));
	}
	isFirstAttemptToConnect = false;

	std::string conninfo = fmt::format(
		"host='{0}' port='{1}' user='{2}' password='{3}' dbname='{4}' sslmode='{5}'",
		escapeConninfoValue(g_config.getString(ConfigManager::POSTGRES_HOST)),
		escapeConninfoValue(std::to_string(g_config.getNumber(ConfigManager::POSTGRES_PORT))),
		escapeConninfoValue(g_config.getString(ConfigManager::POSTGRES_USER)),
		escapeConninfoValue(g_config.getString(ConfigManager::POSTGRES_PASS)),
		escapeConninfoValue(g_config.getString(ConfigManager::POSTGRES_DB)),
		escapeConninfoValue(g_config.getString(ConfigManager::POSTGRES_SSLMODE)));

	tfs::detail::PgConn_ptr handle{PQconnectdb(conninfo.c_str())};
	if (!handle || PQstatus(handle.get()) != CONNECTION_OK) {
		std::cout << std::endl << "PostgreSQL Error Message: " << PQerrorMessage(handle.get()) << std::endl;
		goto error;
	}
	return handle;

	error:
	if (retryIfError) {
		goto retry;
	}
	return nullptr;
}

static bool isLostConnectionError(const unsigned error)
{
	return error == CR_SERVER_LOST || error == CR_SERVER_GONE_ERROR || error == CR_CONN_HOST_ERROR ||
		error == 1053 /*ER_SERVER_SHUTDOWN*/ || error == CR_CONNECTION_ERROR;
}

static bool isPostgresConnectionBad(const tfs::detail::PgConn_ptr& handle)
{
	return !handle || PQstatus(handle.get()) != CONNECTION_OK;
}

static bool executeQuery(tfs::detail::Mysql_ptr& handle, std::string_view query, const bool retryIfLostConnection)
{
	while (mysql_real_query(handle.get(), query.data(), query.length()) != 0) {
		std::cout << "[Error - mysql_real_query] Query: " << query.substr(0, 256) << std::endl
				  << "Message: " << mysql_error(handle.get()) << std::endl;
		const unsigned error = mysql_errno(handle.get());
		if (!isLostConnectionError(error) || !retryIfLostConnection) {
			return false;
		}
		handle = connectToMysql(true);
	}
	return true;
}

static bool executeQuery(tfs::detail::PgConn_ptr& handle, std::string_view query, const bool retryIfLostConnection)
{
	const std::string queryString{query};
	while (true) {
		tfs::detail::PgResult_ptr result{PQexec(handle.get(), queryString.c_str())};
		if (result && (PQresultStatus(result.get()) == PGRES_COMMAND_OK || PQresultStatus(result.get()) == PGRES_TUPLES_OK)) {
			return true;
		}

		std::cout << "[Error - PQexec] Query: " << queryString.substr(0, 256) << std::endl
				  << "Message: " << PQerrorMessage(handle.get()) << std::endl;

		if (!retryIfLostConnection || !isPostgresConnectionBad(handle)) {
			return false;
		}
		handle = connectToPostgres(true);
		if (!handle) {
			return false;
		}
	}
}

bool Database::connect()
{
	backend = getDatabaseBackendFromConfig();
	if (backend == DatabaseBackend::Postgres) {
		auto newHandle = connectToPostgres(false);
		if (!newHandle) {
			return false;
		}

		mysqlHandle.reset();
		pgHandle = std::move(newHandle);

		const std::string& schema = g_config.getString(ConfigManager::POSTGRES_SCHEMA);
		if (!schema.empty()) {
			executeQuery(fmt::format("SET search_path TO {:s}", escapeString(schema)));
		}
		return true;
	}

	auto newHandle = connectToMysql(false);
	if (!newHandle) {
		return false;
	}

	pgHandle.reset();
	mysqlHandle = std::move(newHandle);
	DBResult_ptr result = storeQuery("SHOW VARIABLES LIKE 'max_allowed_packet'");
	if (result) {
		maxPacketSize = result->getNumber<uint64_t>("Value");
	}
	return true;
}

bool Database::beginTransaction()
{
	databaseLock.lock();
	const bool result = executeQuery("START TRANSACTION");
	retryQueries = !result;
	if (!result) {
		databaseLock.unlock();
	}
	return result;
}

bool Database::rollback()
{
	const bool result = executeQuery("ROLLBACK");
	retryQueries = true;
	databaseLock.unlock();
	return result;
}

bool Database::commit()
{
	const bool result = executeQuery("COMMIT");
	retryQueries = true;
	databaseLock.unlock();
	return result;
}

bool Database::executeQuery(const std::string& query)
{
	std::lock_guard<std::recursive_mutex> lockGuard(databaseLock);
#ifdef STATS_ENABLED
	std::chrono::high_resolution_clock::time_point time_point = std::chrono::high_resolution_clock::now();
#endif

	bool success = false;
	if (backend == DatabaseBackend::Postgres) {
		success = ::executeQuery(pgHandle, query, retryQueries);
	} else {
		success = ::executeQuery(mysqlHandle, query, retryQueries);
		// we should call that every time as someone would call executeQuery('SELECT...')
		// as it is described in MySQL manual: "it doesn't hurt" :P
		tfs::detail::MysqlResult_ptr res{mysql_store_result(mysqlHandle.get())};
	}

#ifdef STATS_ENABLED
	uint64_t ns = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::high_resolution_clock::now() - time_point).count();
	g_stats.addSqlStats(new Stat(ns, std::string{query.substr(0, 100)}, std::string{query.substr(0, 256)}));
#endif

	return success;
}

DBResult_ptr Database::storeQuery(std::string_view query)
{
	std::lock_guard<std::recursive_mutex> lockGuard(databaseLock);

#ifdef STATS_ENABLED
	std::chrono::high_resolution_clock::time_point time_point = std::chrono::high_resolution_clock::now();
#endif

	if (backend == DatabaseBackend::Postgres) {
		retry:
		{
			const std::string queryString{query};
			tfs::detail::PgResult_ptr res{PQexec(pgHandle.get(), queryString.c_str())};
			if (!res || PQresultStatus(res.get()) != PGRES_TUPLES_OK) {
				std::cout << "[Error - PQexec] Query: " << queryString << std::endl
						  << "Message: " << PQerrorMessage(pgHandle.get()) << std::endl;
				if (!retryQueries || !isPostgresConnectionBad(pgHandle)) {
					return nullptr;
				}
				pgHandle = connectToPostgres(true);
				if (!pgHandle) {
					return nullptr;
				}
				goto retry;
			}

#ifdef STATS_ENABLED
			uint64_t ns = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::high_resolution_clock::now() - time_point).count();
			g_stats.addSqlStats(new Stat(ns, std::string{query.substr(0, 100)}, std::string{query.substr(0, 256)}));
#endif

			DBResult_ptr result = std::make_shared<DBResult>(std::move(res));
			if (!result->hasNext()) {
				return nullptr;
			}
			return result;
		}
	}

	retry:
	if (!::executeQuery(mysqlHandle, query, retryQueries) && !retryQueries) {
		return nullptr;
	}

	tfs::detail::MysqlResult_ptr res{mysql_store_result(mysqlHandle.get())};
	if (!res) {
		std::cout << "[Error - mysql_store_result] Query: " << query << std::endl
				  << "Message: " << mysql_error(mysqlHandle.get()) << std::endl;
		const unsigned error = mysql_errno(mysqlHandle.get());
		if (!isLostConnectionError(error) || !retryQueries) {
			return nullptr;
		}
		goto retry;
	}

#ifdef STATS_ENABLED
	uint64_t ns = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::high_resolution_clock::now() - time_point).count();
	g_stats.addSqlStats(new Stat(ns, std::string{query.substr(0, 100)}, std::string{query.substr(0, 256)}));
#endif

	// retrieving results of query
	DBResult_ptr result = std::make_shared<DBResult>(std::move(res));
	if (!result->hasNext()) {
		return nullptr;
	}
	return result;
}

std::string Database::escapeString(std::string_view s) const
{
	if (backend == DatabaseBackend::Postgres) {
		const size_t maxLength = (s.length() * 2) + 1;
		std::string escaped;
		escaped.resize(maxLength);
		int error = 0;
		size_t escapedLength = PQescapeStringConn(pgHandle.get(), escaped.data(), s.data(), s.length(), &error);
		if (error != 0) {
			return "''";
		}
		escaped.resize(escapedLength);
		return fmt::format("'{:s}'", escaped);
	}

	return escapeBlob(s.data(), s.length());
}

std::string Database::quoteIdentifier(std::string_view name) const
{
	std::string escaped{name};
	if (backend == DatabaseBackend::Postgres) {
		size_t pos = 0;
		while ((pos = escaped.find('"', pos)) != std::string::npos) {
			escaped.insert(pos, 1, '"');
			pos += 2;
		}
		return fmt::format("\"{:s}\"", escaped);
	}

	size_t pos = 0;
	while ((pos = escaped.find('`', pos)) != std::string::npos) {
		escaped.insert(pos, 1, '`');
		pos += 2;
	}
	return fmt::format("`{:s}`", escaped);
}

std::string Database::escapeBlob(const char* s, uint32_t length) const
{
	if (backend == DatabaseBackend::Postgres) {
		size_t escapedLength = 0;
		unsigned char* output = PQescapeByteaConn(pgHandle.get(), reinterpret_cast<const unsigned char*>(s), length, &escapedLength);
		std::string escaped;
		escaped.reserve(escapedLength + 3);
		escaped.append("E'");
		escaped.append(reinterpret_cast<char*>(output));
		escaped.push_back('\'');
		PQfreemem(output);
		return escaped;
	}

	// the worst case is 2n + 1
	size_t maxLength = (length * 2) + 1;

	std::string escaped;
	escaped.reserve(maxLength + 2);
	escaped.push_back('\'');

	if (length != 0) {
		char* output = new char[maxLength];
		mysql_real_escape_string(mysqlHandle.get(), output, s, length);
		escaped.append(output);
		delete[] output;
	}

	escaped.push_back('\'');
	return escaped;
}

uint64_t Database::getLastInsertId()
{
	std::lock_guard<std::recursive_mutex> lockGuard(databaseLock);
	if (backend == DatabaseBackend::Postgres) {
		tfs::detail::PgResult_ptr result{PQexec(pgHandle.get(), "SELECT LASTVAL()")};
		if (!result || PQresultStatus(result.get()) != PGRES_TUPLES_OK || PQntuples(result.get()) < 1) {
			return 0;
		}
		const char* value = PQgetvalue(result.get(), 0, 0);
		if (!value) {
			return 0;
		}
		return pugi::cast<uint64_t>(value);
	}

	return static_cast<uint64_t>(mysql_insert_id(mysqlHandle.get()));
}

std::string Database::getClientVersion() const
{
	if (backend == DatabaseBackend::Postgres) {
		const int version = PQlibVersion();
		const int major = version / 10000;
		const int minor = (version / 100) % 100;
		const int patch = version % 100;
		return fmt::format("{:d}.{:d}.{:d}", major, minor, patch);
	}
	return mysql_get_client_info();
}

const char* Database::getBackendName() const
{
	return backend == DatabaseBackend::Postgres ? "PostgreSQL" : "MySQL";
}

DBResult::DBResult(tfs::detail::MysqlResult_ptr&& res) : backend{DatabaseBackend::Mysql}, mysqlHandle{std::move(res)}
{
	size_t i = 0;

	MYSQL_FIELD* field = mysql_fetch_field(mysqlHandle.get());
	while (field) {
		listNames[field->name] = i++;
		field = mysql_fetch_field(mysqlHandle.get());
	}

	mysqlRow = mysql_fetch_row(mysqlHandle.get());
}

DBResult::DBResult(tfs::detail::PgResult_ptr&& res) : backend{DatabaseBackend::Postgres}, pgHandle{std::move(res)}
{
	rowCount = PQntuples(pgHandle.get());
	const int fieldCount = PQnfields(pgHandle.get());
	for (int i = 0; i < fieldCount; ++i) {
		listNames[PQfname(pgHandle.get(), i)] = static_cast<size_t>(i);
	}
	currentRow = 0;
}

std::string DBResult::getString(std::string_view column) const
{
	auto it = listNames.find(column);
	if (it == listNames.end()) {
		std::cout << "[Error - DBResult::getStream] Column '" << column << "' doesn't exist in the result set" << std::endl;
		return {};
	}

	if (backend == DatabaseBackend::Postgres) {
		if (PQgetisnull(pgHandle.get(), currentRow, static_cast<int>(it->second)) != 0) {
			return {};
		}
		return PQgetvalue(pgHandle.get(), currentRow, static_cast<int>(it->second));
	}

	if (!mysqlRow[it->second]) {
		return {};
	}

	return mysqlRow[it->second];
}

const char* DBResult::getStream(const std::string& s, unsigned long& size) const
{
	auto it = listNames.find(s);
	if (it == listNames.end()) {
		std::cout << "[Error - DBResult::getStream] Column '" << s << "' doesn't exist in the result set" << std::endl;
		size = 0;
		return nullptr;
	}

	if (backend == DatabaseBackend::Postgres) {
		if (PQgetisnull(pgHandle.get(), currentRow, static_cast<int>(it->second)) != 0) {
			size = 0;
			return nullptr;
		}

		size_t unescapedSize = 0;
		unsigned char* unescaped = PQunescapeBytea(
			reinterpret_cast<unsigned char*>(PQgetvalue(pgHandle.get(), currentRow, static_cast<int>(it->second))),
			&unescapedSize);
		if (!unescaped) {
			size = 0;
			return nullptr;
		}
		postgresBlobBuffer.assign(unescaped, unescaped + unescapedSize);
		PQfreemem(unescaped);
		size = static_cast<unsigned long>(postgresBlobBuffer.size());
		return reinterpret_cast<const char*>(postgresBlobBuffer.data());
	}

	if (mysqlRow[it->second] == nullptr) {
		size = 0;
		return nullptr;
	}

	size = mysql_fetch_lengths(mysqlHandle.get())[it->second];
	return mysqlRow[it->second];
}

bool DBResult::hasNext() const
{
	if (backend == DatabaseBackend::Postgres) {
		return currentRow < rowCount;
	}
	return mysqlRow;
}

bool DBResult::next()
{
	if (backend == DatabaseBackend::Postgres) {
		++currentRow;
		return currentRow < rowCount;
	}

	mysqlRow = mysql_fetch_row(mysqlHandle.get());
	return mysqlRow;
}

DBInsert::DBInsert(std::string query) : query(std::move(query)) { this->length = this->query.length(); }

bool DBInsert::addRow(const std::string& row)
{
	// adds new row to buffer
	const size_t rowLength = row.length();
	length += rowLength;
	if (length > Database::getInstance().getMaxPacketSize() && !execute()) {
		return false;
	}

	if (values.empty()) {
		values.reserve(rowLength + 2);
		values.push_back('(');
		values.append(row);
		values.push_back(')');
	} else {
		values.reserve(values.length() + rowLength + 3);
		values.push_back(',');
		values.push_back('(');
		values.append(row);
		values.push_back(')');
	}
	return true;
}

bool DBInsert::addRow(std::ostringstream& row)
{
	bool ret = addRow(row.str());
	row.str(std::string());
	return ret;
}

bool DBInsert::execute()
{
	if (values.empty()) {
		return true;
	}

	// executes buffer
	bool res = Database::getInstance().executeQuery(query + values);
	values.clear();
	length = query.length();
	return res;
}
