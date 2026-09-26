//! Диагностика состояния FTS: `cargo run --example ftsdiag`
use rusqlite::Connection;

fn main() {
    let path = dirs::data_local_dir().unwrap().join("ClipVault").join("clipvault.db");
    let conn = Connection::open(&path).expect("open db");

    let items: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_items WHERE content IS NOT NULL", [], |r| r.get(0)).unwrap();
    println!("text rows in clipboard_items: {items}");

    let fts: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_fts", [], |r| r.get(0)).unwrap_or(-1);
    println!("rows in clipboard_fts:         {fts}");

    let direct: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH '\"hello\"*'", [], |r| r.get(0)).unwrap_or(-999);
    println!("direct MATCH 'hello*':         {direct}");

    // Пересобрать индекс из внешнего контента.
    match conn.execute("INSERT INTO clipboard_fts(clipboard_fts) VALUES('rebuild')", []) {
        Ok(_) => println!("rebuild: OK"),
        Err(e) => println!("rebuild ERR: {e}"),
    }

    let after: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH '\"hello\"*'", [], |r| r.get(0)).unwrap_or(-999);
    println!("MATCH 'hello*' after rebuild:  {after}");

    let fts2: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_fts", [], |r| r.get(0)).unwrap_or(-1);
    println!("rows in clipboard_fts after:   {fts2}");
}
