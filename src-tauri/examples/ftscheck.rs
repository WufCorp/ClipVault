//! Проверка FTS5-поиска: `cargo run --example ftscheck -- hello`
use rusqlite::{params, Connection};

fn main() {
    let term = std::env::args().nth(1).unwrap_or_else(|| "hello".into());
    let q = format!("\"{}\"*", term.replace('"', "\"\""));
    let path = dirs::data_local_dir()
        .unwrap()
        .join("ClipVault")
        .join("clipvault.db");
    let conn = Connection::open(&path).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT i.id, substr(i.content,1,60) FROM clipboard_items i \
             JOIN clipboard_fts f ON f.rowid = i.id \
             WHERE clipboard_fts MATCH ?1 ORDER BY i.id",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![q], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .unwrap();
    println!("FTS query = {q}");
    let mut n = 0;
    for row in rows {
        let (id, c) = row.unwrap();
        println!("  match #{id}: {}", c.replace('\n', " "));
        n += 1;
    }
    println!("--- {n} match(es) ---");
}
