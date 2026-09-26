//! Слой базы данных: открытие соединения, схема, операции.

pub mod repo;
pub mod schema;

use rusqlite::Connection;
use std::path::Path;

/// Открывает (создаёт при необходимости) БД и инициализирует схему.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    schema::init(&conn)?;
    register_functions(&conn)?;
    Ok(conn)
}

/// `cv_contains(text, needle)` — поиск подстроки без учёта регистра (в т.ч. кириллица).
/// FTS5 находит только начало слова; эта функция закрывает поиск по середине слова.
/// `needle` приходит уже в нижнем регистре (см. repo::substring_terms).
fn register_functions(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "cv_contains",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let hay: Option<String> = ctx.get(0)?;
            let needle: String = ctx.get(1)?;
            Ok(hay.map_or(false, |h| h.to_lowercase().contains(&needle)))
        },
    )
}
