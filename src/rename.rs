//! Rename — the single write path, `UPDATE threads.title` (spec §2.6/§7).

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags};

const WRITE_TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameOutcome {
    Renamed,
    Missing,
}

#[derive(Debug)]
pub enum RenameError {
    Busy,
    Other(rusqlite::Error),
}

pub type RenameRunner = Arc<dyn Fn(&str, &str) -> Result<RenameOutcome, RenameError> + Send + Sync>;

impl fmt::Display for RenameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenameError::Busy => formatter.write_str("database is busy"),
            RenameError::Other(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RenameError::Busy => None,
            RenameError::Other(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenameStore {
    db_path: PathBuf,
}

impl RenameStore {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<RenameOutcome, RenameError> {
        self.rename_inner(id, title).map_err(classify_error)
    }

    fn rename_inner(&self, id: &str, title: &str) -> rusqlite::Result<RenameOutcome> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI;
        let conn = Connection::open_with_flags(&self.db_path, flags)?;
        conn.busy_timeout(WRITE_TIMEOUT)?;
        let changed = conn.execute(
            "UPDATE threads SET title = ?1 WHERE id = ?2",
            rusqlite::params![title, id],
        )?;
        Ok(if changed == 0 {
            RenameOutcome::Missing
        } else {
            RenameOutcome::Renamed
        })
    }
}

pub fn runner(db_path: PathBuf) -> RenameRunner {
    let store = RenameStore::new(db_path);
    Arc::new(move |id, title| store.rename(id, title))
}

pub fn normalize_title(input: &str) -> Option<String> {
    let input = input.trim();
    let mut title = String::with_capacity(input.len());
    let mut separator_pending = false;

    for character in input.chars() {
        if matches!(character, '\n' | '\r' | '\t') {
            while title.ends_with(char::is_whitespace) {
                title.pop();
            }
            separator_pending = !title.is_empty();
        } else if separator_pending && character.is_whitespace() {
            continue;
        } else {
            if separator_pending {
                title.push(' ');
            }
            title.push(character);
            separator_pending = false;
        }
    }

    let title = title.trim().to_string();
    (!title.is_empty()).then_some(title)
}

fn classify_error(error: rusqlite::Error) -> RenameError {
    if let rusqlite::Error::SqliteFailure(sqlite, _) = &error
        && matches!(
            sqlite.code,
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
        )
    {
        return RenameError::Busy;
    }
    RenameError::Other(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_db() -> std::path::PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "tsm-rename-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn seed(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO threads (id,title,updated_at,updated_at_ms)
            VALUES ('one','old title',123,123456);",
        )
        .unwrap();
    }

    #[test]
    fn normalize_trims_and_collapses_line_breaks_and_tabs() {
        assert_eq!(
            normalize_title(" \t hello\t\tworld \n next line \r\n "),
            Some("hello world next line".to_string())
        );
        assert_eq!(
            normalize_title("no length limit"),
            Some("no length limit".to_string())
        );
        assert_eq!(normalize_title(" \n\t\r "), None);
    }

    #[test]
    fn rename_updates_only_title_and_reports_missing_rows() {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path);
        let writer = RenameStore::new(path.clone());

        assert_eq!(
            writer.rename("one", "new title").unwrap(),
            RenameOutcome::Renamed
        );
        assert_eq!(
            writer.rename("missing", "unused").unwrap(),
            RenameOutcome::Missing
        );

        let conn = Connection::open(&path).unwrap();
        let row: (String, i64, i64) = conn
            .query_row(
                "SELECT title,updated_at,updated_at_ms FROM threads WHERE id='one'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("new title".to_string(), 123, 123456));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn writer_never_creates_a_missing_database() {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        let error = RenameStore::new(path.clone())
            .rename("one", "title")
            .unwrap_err();
        assert!(matches!(error, RenameError::Other(_)));
        assert!(!path.exists());
    }
}
