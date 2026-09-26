//! Операции над историей: вставка с дедупликацией, список, поиск, закрепление,
//! удаление, очистка и автоочистка по лимиту.

use crate::models::{now_ms, ClipItem, NewItem, KIND_IMAGE};
use rusqlite::{params, Connection, Row};

// Колонки с префиксом алиаса `i` — обязательно: поиск ссылается на i.content рядом с
// подзапросом к clipboard_fts, у которой тоже есть колонка `content`.
const SELECT_COLS: &str = "i.id, i.type, i.content, i.image_path, i.preview, i.mime_type, \
    i.size, i.created_at, i.last_used_at, i.use_count, i.is_pinned, \
    i.source_app, i.category, i.tags";

fn row_to_item(row: &Row) -> rusqlite::Result<ClipItem> {
    Ok(ClipItem {
        id: row.get(0)?,
        kind: row.get(1)?,
        content: row.get(2)?,
        image_path: row.get(3)?,
        preview: row.get(4)?,
        mime_type: row.get(5)?,
        size: row.get(6)?,
        created_at: row.get(7)?,
        last_used_at: row.get(8)?,
        use_count: row.get(9)?,
        is_pinned: row.get::<_, i64>(10)? != 0,
        source_app: row.get(11)?,
        category: row.get(12)?,
        tags: row.get(13)?,
    })
}

/// WHERE-часть для вкладок интерфейса.
fn filter_clause(filter: &str) -> &'static str {
    match filter {
        "pinned" => "WHERE is_pinned = 1",
        "text" => "WHERE type = 'text'",
        "image" => "WHERE type = 'image'",
        "files" => "WHERE type = 'files'",
        _ => "",
    }
}

/// Список элементов: закреплённые сверху, затем по свежести использования.
pub fn list(
    conn: &Connection,
    filter: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<ClipItem>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM clipboard_items i {} \
         ORDER BY i.is_pinned DESC, COALESCE(i.last_used_at, i.created_at) DESC \
         LIMIT ?1 OFFSET ?2",
        filter_clause(filter)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit, offset], row_to_item)?;
    rows.collect()
}

/// Превращает пользовательский ввод в безопасный FTS5-запрос с префиксным поиском.
fn to_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|term| {
            let escaped = term.replace('"', "\"\"");
            format!("\"{escaped}\"*")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Условие текстового поиска: слово ищется с начала (FTS5, без диакритики)
/// или как подстрока в любом месте текста (`cv_contains`, см. db::open).
/// Каждое слово запроса должно найтись. `None` — пустой запрос.
fn text_condition(input: &str) -> Option<(String, Vec<rusqlite::types::Value>)> {
    use rusqlite::types::Value;
    let fts = to_fts_query(input);
    if fts.is_empty() {
        return None;
    }
    let terms: Vec<String> = input.split_whitespace().map(str::to_lowercase).collect();
    let substr = vec!["cv_contains(i.content, ?)"; terms.len()].join(" AND ");
    let sql = format!(
        "(i.id IN (SELECT rowid FROM clipboard_fts WHERE clipboard_fts MATCH ?) OR ({substr}))"
    );
    let mut binds = vec![Value::Text(fts)];
    binds.extend(terms.into_iter().map(Value::Text));
    Some((sql, binds))
}

/// Мгновенный поиск по тексту: начало слова (FTS5) или фрагмент внутри слова.
pub fn search(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<ClipItem>> {
    let Some((cond, mut binds)) = text_condition(query) else {
        return list(conn, "all", limit, 0);
    };
    binds.push(rusqlite::types::Value::Integer(limit));
    let sql = format!(
        "SELECT {SELECT_COLS} FROM clipboard_items i          WHERE {cond}          ORDER BY i.is_pinned DESC, COALESCE(i.last_used_at, i.created_at) DESC          LIMIT ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), row_to_item)?;
    rows.collect()
}

/// Ищет запись по хэшу содержимого (для дедупликации).
fn find_by_hash(conn: &Connection, hash: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM clipboard_items WHERE content_hash = ?1",
        params![hash],
        |r| r.get::<_, i64>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

/// Вставляет новую запись либо, если такой хэш уже есть, поднимает её наверх
/// (use_count += 1, last_used_at = now). Возвращает (id, было_вставлено).
pub fn insert_or_bump(conn: &Connection, item: &NewItem) -> rusqlite::Result<(i64, bool)> {
    let now = now_ms();
    if let Some(id) = find_by_hash(conn, &item.content_hash)? {
        conn.execute(
            "UPDATE clipboard_items SET use_count = use_count + 1, last_used_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        return Ok((id, false));
    }
    conn.execute(
        "INSERT INTO clipboard_items
            (type, content, image_path, preview, mime_type, size,
             created_at, last_used_at, use_count, is_pinned, content_hash, source_app)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, 0, ?8, ?9)",
        params![
            item.kind,
            item.content,
            item.image_path,
            item.preview,
            item.mime_type,
            item.size,
            now,
            item.content_hash,
            item.source_app,
        ],
    )?;
    Ok((conn.last_insert_rowid(), true))
}

/// Обновляет `last_used_at` (когда элемент повторно скопирован обратно в буфер).
pub fn touch(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE clipboard_items SET last_used_at = ?1, use_count = use_count + 1 WHERE id = ?2",
        params![now_ms(), id],
    )?;
    Ok(())
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE clipboard_items SET is_pinned = ?1 WHERE id = ?2",
        params![pinned as i64, id],
    )?;
    Ok(())
}

pub fn get_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<ClipItem>> {
    let sql = format!("SELECT {SELECT_COLS} FROM clipboard_items i WHERE i.id = ?1");
    conn.query_row(&sql, params![id], row_to_item)
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
}

/// Удаляет запись. Возвращает имя файла картинки, если он был (чтобы вызвавший стёр файл).
pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    let image: Option<String> = conn
        .query_row(
            "SELECT image_path FROM clipboard_items WHERE id = ?1 AND type = ?2",
            params![id, KIND_IMAGE],
            |r| r.get(0),
        )
        .optional()?;
    conn.execute("DELETE FROM clipboard_items WHERE id = ?1", params![id])?;
    Ok(image)
}

/// Очистка истории. Если `keep_pinned` — закреплённые сохраняются (ТЗ §18, §24).
/// Возвращает список файлов картинок, которые нужно удалить с диска.
pub fn clear(conn: &Connection, keep_pinned: bool) -> rusqlite::Result<Vec<String>> {
    let where_clause = if keep_pinned {
        "WHERE is_pinned = 0"
    } else {
        ""
    };
    let images = collect_image_files(conn, where_clause)?;
    conn.execute(
        &format!("DELETE FROM clipboard_items {where_clause}"),
        [],
    )?;
    Ok(images)
}

/// Автоочистка: если записей больше лимита — удаляет самые старые НЕзакреплённые
/// (закреплённые не удаляются автоматически — ТЗ §18).
pub fn cleanup(conn: &Connection, max_items: i64) -> rusqlite::Result<Vec<String>> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM clipboard_items WHERE is_pinned = 0",
        [],
        |r| r.get(0),
    )?;
    if count <= max_items {
        return Ok(Vec::new());
    }
    let excess = count - max_items;
    // id-набор самых старых незакреплённых сверх лимита.
    let sub = "SELECT id FROM clipboard_items WHERE is_pinned = 0 \
               ORDER BY COALESCE(last_used_at, created_at) ASC LIMIT ?1";
    let images: Vec<String> = {
        let sql = format!(
            "SELECT image_path FROM clipboard_items \
             WHERE type = 'image' AND image_path IS NOT NULL AND id IN ({sub})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![excess], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok).collect()
    };
    conn.execute(
        &format!("DELETE FROM clipboard_items WHERE id IN ({sub})"),
        params![excess],
    )?;
    Ok(images)
}

/// Автоочистка по возрасту (Pro, 4.1): удаляет незакреплённые записи старше cutoff.
/// Возвращает файлы картинок для удаления с диска.
pub fn cleanup_by_age(conn: &Connection, cutoff_ms: i64) -> rusqlite::Result<Vec<String>> {
    let sub = "SELECT id FROM clipboard_items \
               WHERE is_pinned = 0 AND COALESCE(last_used_at, created_at) < ?1";
    let images: Vec<String> = {
        let sql = format!(
            "SELECT image_path FROM clipboard_items \
             WHERE type = 'image' AND image_path IS NOT NULL AND id IN ({sub})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![cutoff_ms], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok).collect()
    };
    conn.execute(
        &format!("DELETE FROM clipboard_items WHERE id IN ({sub})"),
        params![cutoff_ms],
    )?;
    Ok(images)
}

/// Устанавливает цветовую метку/категорию (Pro, 4.3). None очищает.
pub fn set_category(conn: &Connection, id: i64, category: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE clipboard_items SET category = ?1 WHERE id = ?2",
        params![category, id],
    )?;
    Ok(())
}

/// Устанавливает теги (CSV, нижний регистр) (Pro, 4.3). None очищает.
pub fn set_tags(conn: &Connection, id: i64, tags: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE clipboard_items SET tags = ?1 WHERE id = ?2",
        params![tags, id],
    )?;
    Ok(())
}

/// Список приложений-источников (для фасета «источник», 4.2).
pub fn distinct_sources(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT source_app FROM clipboard_items \
         WHERE source_app IS NOT NULL AND source_app <> '' ORDER BY source_app",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Фильтры фасетного поиска (Pro, 4.2).
#[derive(Default)]
pub struct Filters {
    pub text: Option<String>,
    pub source: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub since_ms: Option<i64>,
    pub kind: Option<String>,
    pub pinned_only: bool,
}

/// Фасетный поиск: комбинирует FTS-запрос и фильтры источник/категория/тег/дата/тип.
/// Regex применяется вызывающим кодом поверх результата (в командах).
pub fn query_items(conn: &Connection, f: &Filters, limit: i64) -> rusqlite::Result<Vec<ClipItem>> {
    use rusqlite::types::Value;

    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<Value> = Vec::new();

    if let Some((cond, text_binds)) = f.text.as_deref().and_then(text_condition) {
        conditions.push(cond);
        binds.extend(text_binds);
    }
    if let Some(s) = &f.source {
        conditions.push("i.source_app = ?".into());
        binds.push(Value::Text(s.clone()));
    }
    if let Some(c) = &f.category {
        conditions.push("i.category = ?".into());
        binds.push(Value::Text(c.clone()));
    }
    if let Some(t) = &f.tag {
        conditions.push("(',' || lower(i.tags) || ',') LIKE ?".into());
        binds.push(Value::Text(format!("%,{},%", t.to_lowercase())));
    }
    if let Some(since) = f.since_ms {
        conditions.push("COALESCE(i.last_used_at, i.created_at) >= ?".into());
        binds.push(Value::Integer(since));
    }
    if let Some(k) = &f.kind {
        conditions.push("i.type = ?".into());
        binds.push(Value::Text(k.clone()));
    }
    if f.pinned_only {
        conditions.push("i.is_pinned = 1".into());
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    binds.push(Value::Integer(limit));

    let sql = format!(
        "SELECT {SELECT_COLS} FROM clipboard_items i {where_sql} \
         ORDER BY i.is_pinned DESC, COALESCE(i.last_used_at, i.created_at) DESC \
         LIMIT ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), row_to_item)?;
    rows.collect()
}

fn collect_image_files(conn: &Connection, where_clause: &str) -> rusqlite::Result<Vec<String>> {
    let sql = format!(
        "SELECT image_path FROM clipboard_items \
         {} {} type = 'image' AND image_path IS NOT NULL",
        where_clause,
        if where_clause.is_empty() { "WHERE" } else { "AND" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(Result::ok).collect())
}

// Небольшой помощник: rusqlite::OptionalExtension в области видимости.
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn db_with(texts: &[&str]) -> Connection {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        for (n, t) in texts.iter().enumerate() {
            let item = NewItem {
                kind: "text".into(),
                content: Some((*t).into()),
                image_path: None,
                preview: Some((*t).into()),
                mime_type: None,
                size: None,
                content_hash: format!("h{n}"),
                source_app: None,
            };
            insert_or_bump(&conn, &item).unwrap();
        }
        conn
    }

    fn found(conn: &Connection, q: &str) -> Vec<String> {
        let mut v: Vec<String> =
            search(conn, q, 50).unwrap().into_iter().filter_map(|i| i.content).collect();
        v.sort();
        v
    }

    #[test]
    fn search_finds_fragment_inside_word() {
        let conn = db_with(&["Кроссворд на выходные", "invoice_2026.pdf", "Café au lait"]);
        assert_eq!(found(&conn, "ворд"), vec!["Кроссворд на выходные"]);
        assert_eq!(found(&conn, "КРОСС"), vec!["Кроссворд на выходные"]);
        assert_eq!(found(&conn, "2026"), vec!["invoice_2026.pdf"]);
        assert_eq!(found(&conn, "cafe"), vec!["Café au lait"]); // FTS без диакритики
        assert_eq!(found(&conn, "ворд выход"), vec!["Кроссворд на выходные"]);
        assert!(found(&conn, "ворд лайт").is_empty());
    }

    #[test]
    fn facet_search_uses_fragment_too() {
        let conn = db_with(&["Кроссворд", "другое"]);
        let f = Filters { text: Some("оссв".into()), ..Default::default() };
        let v: Vec<_> = query_items(&conn, &f, 50)
            .unwrap()
            .into_iter()
            .filter_map(|i| i.content)
            .collect();
        assert_eq!(v, vec!["Кроссворд"]);
    }
}
