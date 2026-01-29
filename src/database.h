// Copyright 2023 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#ifndef FS_DATABASE_H
#define FS_DATABASE_H

#include "pugicast.h"

#include <libpq-fe.h>

class DBResult;
using DBResult_ptr = std::shared_ptr<DBResult>;

namespace tfs::detail {

struct MysqlDeleter
{
	void operator()(MYSQL* handle) const { mysql_close(handle); }
	void operator()(MYSQL_RES* handle) const { mysql_free_result(handle); }
};

using Mysql_ptr = std::unique_ptr<MYSQL, MysqlDeleter>;
using MysqlResult_ptr = std::unique_ptr<MYSQL_RES, MysqlDeleter>;

struct PgConnDeleter
{
	void operator()(PGconn* handle) const;
};

struct PgResultDeleter
{
	void operator()(PGresult* handle) const;
};

using PgConn_ptr = std::unique_ptr<PGconn, PgConnDeleter>;
using PgResult_ptr = std::unique_ptr<PGresult, PgResultDeleter>;

} // namespace tfs::detail

enum class DatabaseBackend
{
	Mysql,
	Postgres,
};

class Database
{
	public:
		/**
		 * Singleton implementation.
		 *
		 * @return database connection handler singleton
		 */
		static Database& getInstance()
		{
			static Database instance;
			return instance;
		}

		/**
		 * Connects to the database
		 *
		 * @return true on successful connection, false on error
		 */
		bool connect();

		/**
		 * Executes command.
		 *
		 * Executes query which doesn't generates results (eg. INSERT, UPDATE, DELETE...).
		 *
		 * @param query command
		 * @return true on success, false on error
		 */
		bool executeQuery(const std::string& query);

		/**
		 * Queries database.
		 *
		 * Executes query which generates results (mostly SELECT).
		 *
		 * @return results object (nullptr on error)
		 */
		DBResult_ptr storeQuery(std::string_view query);

		/**
		 * Escapes string for query.
		 *
		 * Prepares string to fit SQL queries including quoting it.
		 *
		 * @param s string to be escaped
		 * @return quoted string
		 */
		[[nodiscard]] std::string escapeString(std::string_view s) const;

		/**
		 * Escapes binary stream for query.
		 *
		 * Prepares binary stream to fit SQL queries.
		 *
		 * @param s binary stream
		 * @param length stream length
		 * @return quoted string
		 */
		std::string escapeBlob(const char* s, uint32_t length) const;

		/**
		 * Retrieve id of last inserted row
		 *
		 * @return id on success, 0 if last query did not result on any rows with auto_increment keys
		 */
		[[nodiscard]] uint64_t getLastInsertId();

		/**
		 * Get database engine version
		 *
		 * @return the database engine version
		 */
		[[nodiscard]] std::string getClientVersion() const;

		[[nodiscard]] DatabaseBackend getBackend() const { return backend; }
		[[nodiscard]] const char* getBackendName() const;

		[[nodiscard]] uint64_t getMaxPacketSize() const { return maxPacketSize; }

	private:
		/**
		 * Transaction related methods.
		 *
		 * Methods for starting, committing and rolling back transaction. Each of the returns boolean value.
		 *
		 * @return true on success, false on error
		 */
		bool beginTransaction();
		bool rollback();
		bool commit();

		DatabaseBackend backend = DatabaseBackend::Mysql;
		tfs::detail::Mysql_ptr mysqlHandle = nullptr;
		tfs::detail::PgConn_ptr pgHandle = nullptr;
		mutable std::recursive_mutex databaseLock;
		uint64_t maxPacketSize = 1048576;
		// Do not retry queries if we are in the middle of a transaction
		bool retryQueries = true;

	friend class DBTransaction;
};

class DBResult
{
	public:
		explicit DBResult(tfs::detail::MysqlResult_ptr&& res);
		explicit DBResult(tfs::detail::PgResult_ptr&& res);

		// non-copyable
		DBResult(const DBResult&) = delete;
		DBResult& operator=(const DBResult&) = delete;

		template <typename T>
		T getNumber(std::string_view column) const
		{
			auto it = listNames.find(column);
			if (it == listNames.end()) {
				std::cout << "[Error - DBResult::getNumber] Column '" << column << "' doesn't exist in the result set"
						  << std::endl;
				return {};
			}

			if (backend == DatabaseBackend::Postgres) {
				if (PQgetisnull(pgHandle.get(), currentRow, static_cast<int>(it->second)) != 0) {
					return {};
				}
				return pugi::cast<T>(PQgetvalue(pgHandle.get(), currentRow, static_cast<int>(it->second)));
			}

			if (!mysqlRow[it->second]) {
				return {};
			}

			return pugi::cast<T>(mysqlRow[it->second]);
		}

		[[nodiscard]] std::string getString(std::string_view column) const;
		const char* getStream(const std::string& s, unsigned long& size) const;

		[[nodiscard]] bool hasNext() const;
		bool next();

	private:
		DatabaseBackend backend = DatabaseBackend::Mysql;
		tfs::detail::MysqlResult_ptr mysqlHandle;
		MYSQL_ROW mysqlRow = nullptr;
		tfs::detail::PgResult_ptr pgHandle;
		int currentRow = 0;
		int rowCount = 0;
		mutable std::vector<unsigned char> postgresBlobBuffer;

		std::map<std::string_view, size_t> listNames;

		friend class Database;
};

/**
 * INSERT statement.
 */
class DBInsert
{
	public:
		explicit DBInsert(std::string query);
		bool addRow(const std::string& row);
		bool addRow(std::ostringstream& row);
		bool execute();

	private:
		std::string query;
		std::string values;
		size_t length;
};

class DBTransaction
{
	public:
		constexpr DBTransaction() = default;

		~DBTransaction()
		{
			if (state == STATE_START) {
				Database::getInstance().rollback();
			}
		}

		// non-copyable
		DBTransaction(const DBTransaction&) = delete;
		DBTransaction& operator=(const DBTransaction&) = delete;

		bool begin()
		{
			state = STATE_START;
			return Database::getInstance().beginTransaction();
		}

		bool commit()
		{
			if (state != STATE_START) {
				return false;
			}

			state = STATE_COMMIT;
			return Database::getInstance().commit();
		}

	private:
		enum TransactionStates_t
		{
			STATE_NO_START,
			STATE_START,
			STATE_COMMIT,
		};

		TransactionStates_t state = STATE_NO_START;
};

#endif // FS_DATABASE_H
