use sha1::{Digest, Sha1};
use sqlx::mysql::MySqlRow;
use sqlx::{MySql, Pool, Row};

/// Read a text column that may arrive as `VARCHAR` or `VARBINARY`.
///
/// `VARCHAR ... COLLATE utf8mb4_bin` is sent by MySQL/MariaDB with the binary
/// charset, and sqlx exposes that as `VARBINARY`. `String` only decodes `VARCHAR`.
fn text_column(row: &MySqlRow, column: &str) -> Result<String, sqlx::Error> {
    match row.try_get::<String, _>(column) {
        Ok(value) => Ok(value),
        Err(string_err) => {
            let bytes: Vec<u8> = match row.try_get(column) {
                Ok(bytes) => bytes,
                Err(_) => return Err(string_err),
            };
            String::from_utf8(bytes).map_err(|err| sqlx::Error::ColumnDecode {
                index: format!("\"{column}\""),
                source: Box::new(err),
            })
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cam {
    pub id: u32,
    #[allow(dead_code)]
    pub player_id: u32,
    pub account_id: u32,
    pub player_level: u32,
    #[allow(dead_code)]
    pub player_ip: u32,
    #[allow(dead_code)]
    pub title: String,
    #[allow(dead_code)]
    pub date_start: u32,
    #[allow(dead_code)]
    pub date_end: u32,
    pub offset_start: i32,
    pub offset_end: i32,
    #[allow(dead_code)]
    pub size: u32,
    pub path: String,
    pub public: i8,
    pub access_code: String,
}

#[derive(Clone)]
pub struct Repository {
    pool: Pool<MySql>,
}

impl Repository {
    pub fn new(pool: Pool<MySql>) -> Self {
        Self { pool }
    }

    pub async fn get_cam_by_id(&self, cam_id: u32) -> Result<Option<Cam>, sqlx::Error> {
        let row_opt = sqlx::query(
            "SELECT id, player_id, account_id, player_level, player_ip, title, date_start, date_end, offset_start, offset_end, size, path, public, access_code FROM `cams` WHERE `id` = ? LIMIT 1"
        )
        .bind(cam_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row_opt {
            Ok(Some(Cam {
                id: row.get("id"),
                player_id: row.get("player_id"),
                account_id: row.get("account_id"),
                player_level: row.get("player_level"),
                player_ip: row.get("player_ip"),
                title: row.get("title"),
                date_start: row.get("date_start"),
                date_end: row.get("date_end"),
                offset_start: row.get("offset_start"),
                offset_end: row.get("offset_end"),
                size: row.get("size"),
                path: row.get("path"),
                public: row.get("public"),
                access_code: text_column(&row, "access_code")?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn verify_account_access(
        &self,
        account_name: &str,
        password_plain: &str,
        cam_account_id: u32,
    ) -> Result<bool, sqlx::Error> {
        let row_opt = sqlx::query("SELECT id, password FROM `accounts` WHERE `name` = ? LIMIT 1")
            .bind(account_name)
            .fetch_optional(&self.pool)
            .await?;

        let row = match row_opt {
            Some(r) => r,
            None => return Ok(false),
        };

        let account_id: i32 = row.get("id");
        let db_pass: String = row.get("password");

        let mut hasher = Sha1::new();
        hasher.update(password_plain.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        if !hash.eq_ignore_ascii_case(&db_pass) {
            return Ok(false);
        }

        Ok((account_id as u32) == cam_account_id)
    }
}
