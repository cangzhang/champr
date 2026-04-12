CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS champion_data (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL,
    champion_id INTEGER NOT NULL,
    champion_alias TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'ranked',
    version TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (source_id) REFERENCES sources(id) ON DELETE CASCADE,
    UNIQUE (source_id, champion_id, mode),
    UNIQUE (source_id, champion_alias, mode)
);

CREATE INDEX IF NOT EXISTS idx_champion_data_lookup_id
    ON champion_data (source_id, champion_id, mode);

CREATE INDEX IF NOT EXISTS idx_champion_data_lookup_alias
    ON champion_data (source_id, champion_alias, mode);
