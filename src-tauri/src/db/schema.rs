//! Схема БД и миграции (через PRAGMA user_version).

use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 3;

/// Инициализирует соединение и создаёт схему при первом запуске.
pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 3000;
         PRAGMA foreign_keys = ON;",
    )?;

    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        migrate_to_v1(conn)?;
    }
    if version < 2 {
        migrate_to_v2(conn)?;
    }
    if version < 3 {
        migrate_to_v3(conn)?;
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// v2: приложение-источник записи (для фильтра по источнику и игнор-списка).
fn migrate_to_v2(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("ALTER TABLE clipboard_items ADD COLUMN source_app TEXT;")
}

/// v3: категория/метка и теги (Pro: организация, 4.3).
/// `category` — цветовая метка (имя цвета/пусто); `tags` — CSV тегов в нижнем регистре.
fn migrate_to_v3(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "ALTER TABLE clipboard_items ADD COLUMN category TEXT;
         ALTER TABLE clipboard_items ADD COLUMN tags TEXT;",
    )
}

fn migrate_to_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS clipboard_items (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            type          TEXT NOT NULL,
            content       TEXT,
            image_path    TEXT,
            preview       TEXT,
            mime_type     TEXT,
            size          INTEGER,
            created_at    INTEGER NOT NULL,
            last_used_at  INTEGER,
            use_count     INTEGER NOT NULL DEFAULT 1,
            is_pinned     INTEGER NOT NULL DEFAULT 0,
            content_hash  TEXT UNIQUE
        );

        CREATE INDEX IF NOT EXISTS idx_items_order
            ON clipboard_items(is_pinned DESC, last_used_at DESC);

        -- Полнотекстовый поиск по тексту (external-content FTS5).
        CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_fts USING fts5(
            content,
            content='clipboard_items',
            content_rowid='id',
            tokenize='unicode61 remove_diacritics 2'
        );

        -- Триггеры синхронизации FTS с основной таблицей.
        CREATE TRIGGER IF NOT EXISTS clipboard_ai AFTER INSERT ON clipboard_items BEGIN
            INSERT INTO clipboard_fts(rowid, content) VALUES (new.id, new.content);
        END;
        CREATE TRIGGER IF NOT EXISTS clipboard_ad AFTER DELETE ON clipboard_items BEGIN
            INSERT INTO clipboard_fts(clipboard_fts, rowid, content)
                VALUES ('delete', old.id, old.content);
        END;
        CREATE TRIGGER IF NOT EXISTS clipboard_au AFTER UPDATE ON clipboard_items BEGIN
            INSERT INTO clipboard_fts(clipboard_fts, rowid, content)
                VALUES ('delete', old.id, old.content);
            INSERT INTO clipboard_fts(rowid, content) VALUES (new.id, new.content);
        END;",
    )
}
