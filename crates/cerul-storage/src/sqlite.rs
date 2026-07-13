use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use rusqlite::Connection;

use crate::paths::AppPaths;

mod embedded {
    // Latest embedded schema change: V0015__item_discovered_at.sql.
    use refinery::embed_migrations;

    embed_migrations!("migrations");
}

static INITIALIZED_DATABASES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

pub fn open(paths: &AppPaths) -> anyhow::Result<Connection> {
    let mut conn = Connection::open(&paths.db)?;
    configure_connection(&conn)?;
    initialize_database_once(&mut conn, &paths.db)?;
    Ok(conn)
}

fn configure_connection(conn: &Connection) -> anyhow::Result<()> {
    conn.busy_timeout(Duration::from_secs(30))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn initialize_database_once(conn: &mut Connection, db_path: &Path) -> anyhow::Result<()> {
    let initialized = INITIALIZED_DATABASES.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = initialized
        .lock()
        .expect("SQLite initialization mutex poisoned");
    if guard.contains(db_path) {
        return Ok(());
    }

    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    embedded::migrations::runner().run(conn)?;
    guard.insert(db_path.to_path_buf());
    Ok(())
}
