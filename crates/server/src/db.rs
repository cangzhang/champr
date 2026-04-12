use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    auth::hash_password,
    config::Config,
    error::{AppError, AppResult},
    models::{
        ChampionDataRecord, CreateSourceRequest, CreateUserRequest, SourceRecord,
        UpdateSourceRequest, UpdateUserRequest, UpsertChampionDataRequest, UserRecord, UserRole,
        db_bool, normalize_alias, normalize_email, normalize_mode, normalize_source_key,
        now_rfc3339,
    },
};

pub type Database = Arc<Mutex<Connection>>;

#[derive(Debug, Clone)]
pub struct AppState {
    pub db: Database,
    pub jwt_secret: String,
    pub addr: String,
}

pub async fn init_state(config: Config) -> anyhow::Result<AppState> {
    ensure_sqlite_parent_exists(&config.database_url)?;
    let connection = Connection::open(sqlite_path_from_url(&config.database_url))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    connection.execute_batch(include_str!("../migrations/0001_init.sql"))?;

    let db = Arc::new(Mutex::new(connection));

    bootstrap_default_sources(&db).await?;
    bootstrap_admin(&db, &config).await?;

    Ok(AppState {
        db,
        jwt_secret: config.jwt_secret,
        addr: config.addr,
    })
}

pub async fn find_user_by_id(db: &Database, id: i64) -> AppResult<Option<UserRecord>> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT id, email, password_hash, role, is_active, created_at, updated_at FROM users WHERE id = ?1",
            params![id],
            map_user,
        )
        .optional()
    })
}

pub async fn find_user_by_email(db: &Database, email: &str) -> AppResult<Option<UserRecord>> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT id, email, password_hash, role, is_active, created_at, updated_at FROM users WHERE email = ?1",
            params![email],
            map_user,
        )
        .optional()
    })
}

pub async fn list_users(db: &Database) -> AppResult<Vec<UserRecord>> {
    with_conn(db, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, email, password_hash, role, is_active, created_at, updated_at FROM users ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], map_user)?;
        rows.collect()
    })
}

pub async fn create_user(db: &Database, req: CreateUserRequest) -> AppResult<UserRecord> {
    let email = normalize_email(&req.email);
    if email.is_empty() {
        return Err(AppError::BadRequest("email is required".to_string()));
    }

    let password_hash = hash_password(&req.password)?;
    let now = now_rfc3339();
    let id = with_conn(db, |conn| {
        conn.execute(
            "INSERT INTO users (email, password_hash, role, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![email, password_hash, req.role.as_str(), db_bool(req.is_active.unwrap_or(true)), now, now],
        )?;
        Ok(conn.last_insert_rowid())
    })?;

    find_user_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::internal("created user missing"))
}

pub async fn update_user(db: &Database, id: i64, req: UpdateUserRequest) -> AppResult<UserRecord> {
    let Some(current) = find_user_by_id(db, id).await? else {
        return Err(AppError::NotFound("user not found".to_string()));
    };

    let password_hash = match req.password {
        Some(password) => hash_password(&password)?,
        None => current.password_hash,
    };
    let role = req
        .role
        .unwrap_or(
            UserRole::from_db(&current.role)
                .ok_or_else(|| AppError::internal("invalid stored role"))?,
        )
        .as_str()
        .to_string();
    let is_active = req.is_active.unwrap_or(current.is_active != 0);
    let now = now_rfc3339();

    with_conn(db, |conn| {
        conn.execute(
            "UPDATE users SET password_hash = ?1, role = ?2, is_active = ?3, updated_at = ?4 WHERE id = ?5",
            params![password_hash, role, db_bool(is_active), now, id],
        )?;
        Ok(())
    })?;

    find_user_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::internal("updated user missing"))
}

pub async fn list_active_sources(db: &Database) -> AppResult<Vec<SourceRecord>> {
    with_conn(db, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, key, label, is_active, created_at, updated_at FROM sources WHERE is_active = 1 ORDER BY label ASC",
        )?;
        let rows = stmt.query_map([], map_source)?;
        rows.collect()
    })
}

pub async fn create_source(db: &Database, req: CreateSourceRequest) -> AppResult<SourceRecord> {
    let key = normalize_source_key(&req.key);
    let label = req.label.trim().to_string();
    if key.is_empty() || label.is_empty() {
        return Err(AppError::BadRequest(
            "source key and label are required".to_string(),
        ));
    }

    let now = now_rfc3339();
    let id = with_conn(db, |conn| {
        conn.execute(
            "INSERT INTO sources (key, label, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, label, db_bool(req.is_active.unwrap_or(true)), now, now],
        )?;
        Ok(conn.last_insert_rowid())
    })?;

    find_source_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::internal("created source missing"))
}

pub async fn update_source(
    db: &Database,
    id: i64,
    req: UpdateSourceRequest,
) -> AppResult<SourceRecord> {
    let Some(current) = find_source_by_id(db, id).await? else {
        return Err(AppError::NotFound("source not found".to_string()));
    };

    let key = req
        .key
        .map(|value| normalize_source_key(&value))
        .unwrap_or(current.key);
    let label = req.label.unwrap_or(current.label).trim().to_string();
    if key.is_empty() || label.is_empty() {
        return Err(AppError::BadRequest(
            "source key and label are required".to_string(),
        ));
    }

    let now = now_rfc3339();
    with_conn(db, |conn| {
        conn.execute(
            "UPDATE sources SET key = ?1, label = ?2, is_active = ?3, updated_at = ?4 WHERE id = ?5",
            params![key, label, db_bool(req.is_active.unwrap_or(current.is_active != 0)), now, id],
        )?;
        Ok(())
    })?;

    find_source_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::internal("updated source missing"))
}

pub async fn upsert_champion_data(
    db: &Database,
    req: UpsertChampionDataRequest,
) -> AppResult<ChampionDataRecord> {
    validate_champion_request(&req)?;
    let source = resolve_source(db, req.source_id, req.source_key.as_deref()).await?;
    let alias = normalize_alias(&req.champion_alias);
    let mode = normalize_mode(req.mode.as_deref());
    let payload = serde_json::to_string(&req.content).map_err(AppError::internal)?;
    let now = now_rfc3339();

    let id = with_conn(db, |conn| {
        let existing_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM champion_data WHERE source_id = ?1 AND mode = ?2 AND (champion_id = ?3 OR champion_alias = ?4) LIMIT 1",
                params![source.id, mode, req.champion_id, alias],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(existing_id) = existing_id {
            conn.execute(
                "UPDATE champion_data SET source_id = ?1, champion_id = ?2, champion_alias = ?3, mode = ?4, version = ?5, payload = ?6, updated_at = ?7 WHERE id = ?8",
                params![source.id, req.champion_id, alias, mode, req.version, payload, now, existing_id],
            )?;
            Ok(existing_id)
        } else {
            conn.execute(
                "INSERT INTO champion_data (source_id, champion_id, champion_alias, mode, version, payload, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![source.id, req.champion_id, alias, mode, req.version, payload, now, now],
            )?;
            Ok(conn.last_insert_rowid())
        }
    })?;

    find_champion_data_by_record_id(db, id)
        .await?
        .ok_or_else(|| AppError::internal("upserted champion data missing"))
}

pub async fn replace_champion_data(
    db: &Database,
    id: i64,
    req: UpsertChampionDataRequest,
) -> AppResult<ChampionDataRecord> {
    validate_champion_request(&req)?;
    let Some(current) = find_champion_data_by_record_id(db, id).await? else {
        return Err(AppError::NotFound("champion data not found".to_string()));
    };

    let source = resolve_source(
        db,
        req.source_id.or(Some(current.source_id)),
        req.source_key.as_deref(),
    )
    .await?;
    let alias = normalize_alias(&req.champion_alias);
    let mode = normalize_mode(req.mode.as_deref());
    let payload = serde_json::to_string(&req.content).map_err(AppError::internal)?;
    let now = now_rfc3339();

    with_conn(db, |conn| {
        conn.execute(
            "UPDATE champion_data SET source_id = ?1, champion_id = ?2, champion_alias = ?3, mode = ?4, version = ?5, payload = ?6, updated_at = ?7 WHERE id = ?8",
            params![source.id, req.champion_id, alias, mode, req.version, payload, now, id],
        )?;
        Ok(())
    })?;

    find_champion_data_by_record_id(db, id)
        .await?
        .ok_or_else(|| AppError::internal("updated champion data missing"))
}

pub async fn get_champion_data_by_source_and_id(
    db: &Database,
    source_key: &str,
    champion_id: i64,
    mode: &str,
) -> AppResult<ChampionDataRecord> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT cd.id, cd.source_id, s.key, cd.mode, cd.version, cd.champion_id, cd.champion_alias, cd.payload, cd.created_at, cd.updated_at FROM champion_data cd INNER JOIN sources s ON s.id = cd.source_id WHERE s.key = ?1 AND s.is_active = 1 AND cd.champion_id = ?2 AND cd.mode = ?3 LIMIT 1",
            params![normalize_source_key(source_key), champion_id, mode],
            map_champion_data,
        )
        .optional()
    })?
    .ok_or_else(|| AppError::NotFound("champion data not found".to_string()))
}

pub async fn get_champion_data_by_source_and_alias(
    db: &Database,
    source_key: &str,
    champion_alias: &str,
    mode: &str,
) -> AppResult<ChampionDataRecord> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT cd.id, cd.source_id, s.key, cd.mode, cd.version, cd.champion_id, cd.champion_alias, cd.payload, cd.created_at, cd.updated_at FROM champion_data cd INNER JOIN sources s ON s.id = cd.source_id WHERE s.key = ?1 AND s.is_active = 1 AND cd.champion_alias = ?2 AND cd.mode = ?3 LIMIT 1",
            params![normalize_source_key(source_key), normalize_alias(champion_alias), mode],
            map_champion_data,
        )
        .optional()
    })?
    .ok_or_else(|| AppError::NotFound("champion data not found".to_string()))
}

fn with_conn<T, F>(db: &Database, f: F) -> AppResult<T>
where
    F: FnOnce(&Connection) -> rusqlite::Result<T>,
{
    let conn = db
        .lock()
        .map_err(|_| AppError::internal("database lock poisoned"))?;
    f(&conn).map_err(db_err)
}

fn sqlite_path_from_url(database_url: &str) -> String {
    if let Some(path) = database_url.strip_prefix("sqlite://") {
        path.to_string()
    } else {
        database_url.to_string()
    }
}

fn ensure_sqlite_parent_exists(database_url: &str) -> anyhow::Result<()> {
    let path = sqlite_path_from_url(database_url);
    if path == ":memory:" || path.starts_with("file:") {
        return Ok(());
    }

    let db_path = Path::new(&path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(())
}

async fn bootstrap_default_sources(db: &Database) -> AppResult<()> {
    let now = now_rfc3339();
    with_conn(db, |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO sources (key, label, is_active, created_at, updated_at) VALUES (?1, ?2, 1, ?3, ?4)",
            params!["op.gg", "OP.GG", now, now],
        )?;
        Ok(())
    })
}

async fn bootstrap_admin(db: &Database, config: &Config) -> AppResult<()> {
    let Some(email) = config.bootstrap_admin_email.as_deref() else {
        return Ok(());
    };
    let Some(password) = config.bootstrap_admin_password.as_deref() else {
        return Ok(());
    };

    let email = normalize_email(email);
    let password_hash = hash_password(password)?;
    let now = now_rfc3339();

    with_conn(db, |conn| {
        conn.execute(
            "INSERT INTO users (email, password_hash, role, is_active, created_at, updated_at) VALUES (?1, ?2, 'admin', 1, ?3, ?4) ON CONFLICT(email) DO UPDATE SET password_hash = excluded.password_hash, role = 'admin', is_active = 1, updated_at = excluded.updated_at",
            params![email, password_hash, now, now],
        )?;
        Ok(())
    })
}

async fn resolve_source(
    db: &Database,
    source_id: Option<i64>,
    source_key: Option<&str>,
) -> AppResult<SourceRecord> {
    match (source_id, source_key) {
        (Some(id), Some(key)) => {
            let source = find_source_by_id(db, id)
                .await?
                .ok_or_else(|| AppError::NotFound("source not found".to_string()))?;
            if source.key != normalize_source_key(key) {
                return Err(AppError::BadRequest(
                    "source_id and source_key refer to different sources".to_string(),
                ));
            }
            Ok(source)
        }
        (Some(id), None) => find_source_by_id(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("source not found".to_string())),
        (None, Some(key)) => find_source_by_key(db, &normalize_source_key(key))
            .await?
            .ok_or_else(|| AppError::NotFound("source not found".to_string())),
        (None, None) => Err(AppError::BadRequest(
            "source_id or source_key is required".to_string(),
        )),
    }
}

async fn find_source_by_id(db: &Database, id: i64) -> AppResult<Option<SourceRecord>> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT id, key, label, is_active, created_at, updated_at FROM sources WHERE id = ?1",
            params![id],
            map_source,
        )
        .optional()
    })
}

async fn find_source_by_key(db: &Database, key: &str) -> AppResult<Option<SourceRecord>> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT id, key, label, is_active, created_at, updated_at FROM sources WHERE key = ?1",
            params![key],
            map_source,
        )
        .optional()
    })
}

async fn find_champion_data_by_record_id(
    db: &Database,
    id: i64,
) -> AppResult<Option<ChampionDataRecord>> {
    with_conn(db, |conn| {
        conn.query_row(
            "SELECT cd.id, cd.source_id, s.key, cd.mode, cd.version, cd.champion_id, cd.champion_alias, cd.payload, cd.created_at, cd.updated_at FROM champion_data cd INNER JOIN sources s ON s.id = cd.source_id WHERE cd.id = ?1",
            params![id],
            map_champion_data,
        )
        .optional()
    })
}

fn validate_champion_request(req: &UpsertChampionDataRequest) -> AppResult<()> {
    if req.version.trim().is_empty() {
        return Err(AppError::BadRequest("version is required".to_string()));
    }
    if req.champion_id <= 0 {
        return Err(AppError::BadRequest(
            "champion_id must be positive".to_string(),
        ));
    }
    if normalize_alias(&req.champion_alias).is_empty() {
        return Err(AppError::BadRequest(
            "champion_alias is required".to_string(),
        ));
    }

    Ok(())
}

fn db_err(err: rusqlite::Error) -> AppError {
    let message = err.to_string();
    if message.contains("UNIQUE constraint failed") {
        AppError::Conflict(message)
    } else {
        AppError::Internal(message)
    }
}

fn map_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRecord> {
    Ok(UserRecord {
        id: row.get(0)?,
        email: row.get(1)?,
        password_hash: row.get(2)?,
        role: row.get(3)?,
        is_active: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn map_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRecord> {
    Ok(SourceRecord {
        id: row.get(0)?,
        key: row.get(1)?,
        label: row.get(2)?,
        is_active: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_champion_data(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChampionDataRecord> {
    Ok(ChampionDataRecord {
        id: row.get(0)?,
        source_id: row.get(1)?,
        source_key: row.get(2)?,
        mode: row.get(3)?,
        version: row.get(4)?,
        champion_id: row.get(5)?,
        champion_alias: row.get(6)?,
        payload: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}
