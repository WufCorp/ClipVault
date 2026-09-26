//! Диагностика: печатает содержимое истории ClipVault. Запуск: `cargo run --example dump`.
use rusqlite::Connection;

fn main() {
    let path = dirs::data_local_dir()
        .unwrap()
        .join("ClipVault")
        .join("clipvault.db");
    let conn = Connection::open(&path).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT id, type, substr(COALESCE(content, image_path, ''), 1, 50), \
             use_count, is_pinned, size FROM clipboard_items \
             ORDER BY is_pinned DESC, COALESCE(last_used_at, created_at) DESC",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })
        .unwrap();
    let mut n = 0;
    for row in rows {
        let (id, t, c, uc, pin, size) = row.unwrap();
        println!(
            "#{id} [{t}] use={uc} pin={pin} size={} :: {}",
            size.unwrap_or(0),
            c.replace('\n', " ")
        );
        n += 1;
    }
    println!("--- total rows: {n} ---");
}
