//! Store — read-only access to traex's `state_(N).sqlite` `threads` table.
//!
//! This is the single module coupled to traex's DB schema (spec §2). Everything
//! else works against the [`Session`] snapshot it produces.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags};

/// Startup-facing busy text (spec §11).
const BUSY_MESSAGE: &str = "traex database is busy · try again";

/// The `threads` projection shared by every list query (spec §2.3/§2.5); the
/// only per-query difference is the trailing `WHERE` / `ORDER BY`.
const SELECT_SESSION: &str = "SELECT id,title,first_user_message,cwd,updated_at,updated_at_ms,\
     archived,archived_at,git_branch,model,tokens_used \
     FROM threads";

/// Columns tsm relies on in `threads`; presence is validated before we trust a
/// database (spec §2.2 / §2.3).
const REQUIRED_COLUMNS: &[&str] = &[
    "id",
    "title",
    "first_user_message",
    "cwd",
    "source",
    "updated_at",
    "updated_at_ms",
    "archived",
    "archived_at",
    "git_branch",
    "model",
    "tokens_used",
];

/// One row of `threads`, projected to the columns tsm shows (spec §2.3).
///
/// Some fields are consumed by later tickets (scope/lifecycle/mutate/preview);
/// they are part of the store contract even where ticket 01 does not read them.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub first_user_message: String,
    pub cwd: String,
    /// epoch seconds
    pub updated_at: i64,
    /// epoch millis — the authoritative sort key
    pub updated_at_ms: i64,
    pub archived: bool,
    /// epoch seconds; only set when archived
    pub archived_at: Option<i64>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub tokens_used: i64,
}

/// Resolve the traex cli directory from the env chain (spec §2.1).
///
/// Priority: `$TRAECLI_HOME` → `$TRAE_HOME/cli` → `~/.trae/cli`. `CODEX_HOME` is
/// deliberately ignored.
pub fn resolve_cli_dir(
    traecli_home: Option<&str>,
    trae_home: Option<&str>,
    home: &Path,
) -> PathBuf {
    if let Some(d) = traecli_home.filter(|s| !s.is_empty()) {
        return PathBuf::from(d);
    }
    if let Some(d) = trae_home.filter(|s| !s.is_empty()) {
        return PathBuf::from(d).join("cli");
    }
    home.join(".trae").join("cli")
}

/// Resolve the cli directory using the real process environment.
pub fn resolve_cli_dir_from_env() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    Ok(resolve_cli_dir(
        std::env::var("TRAECLI_HOME").ok().as_deref(),
        std::env::var("TRAE_HOME").ok().as_deref(),
        &home,
    ))
}

/// Given file names in a directory, pick the `state_(N).sqlite` with the highest
/// generation number N (spec §2.2). Returns `(generation, filename)`.
pub fn pick_state_db(filenames: &[String]) -> Option<(u64, String)> {
    filenames
        .iter()
        .filter_map(|name| parse_generation(name).map(|n| (n, name.clone())))
        .max_by_key(|(n, _)| *n)
}

/// Parse the generation number out of a `state_(\d+).sqlite` file name.
fn parse_generation(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("state_")?;
    let digits = rest.strip_suffix(".sqlite")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Locate the state database to use, resolving `--db` or the env chain (spec §2.1/§2.2).
///
/// `--db` points directly at a file (glob skipped); otherwise the resolved cli
/// directory is scanned for the highest-generation `state_(N).sqlite`.
pub fn locate_db(db_flag: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = db_flag {
        if !path.is_file() {
            bail!("no traex state database found at {}", path.display());
        }
        return Ok(path.to_path_buf());
    }

    let dir = resolve_cli_dir_from_env()?;
    let filenames: Vec<String> = std::fs::read_dir(&dir)
        .with_context(|| no_db_message(&dir))?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();

    match pick_state_db(&filenames) {
        Some((_, name)) => Ok(dir.join(name)),
        None => bail!(no_db_message(&dir)),
    }
}

/// The startup "no state database" message, including the env-chain hint (spec §11).
fn no_db_message(dir: &Path) -> String {
    format!(
        "no traex state database found at {} · set --db or $TRAE_HOME",
        dir.display()
    )
}

/// A short-lived read-only handle to the state database (spec §2.4).
#[derive(Debug)]
pub struct Store {
    db_path: PathBuf,
}

impl Store {
    /// Open the database read-only, validating that `threads` has the columns
    /// tsm needs (spec §2.2/§2.4/§11).
    pub fn open(db_path: PathBuf) -> Result<Self> {
        let conn = open_readonly(&db_path)?;
        validate_schema(&conn, &db_path)?;
        Ok(Self { db_path })
    }

    /// Path used by the independent rename write connection (spec §2.6).
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Run the current-project active-sessions query (spec §2.5, the default landing).
    ///
    /// Each refresh is its own short read (spec §2.4): opens a fresh connection,
    /// runs one query, drops it. A `SQLITE_BUSY` timeout is mapped to the
    /// spec §11 busy message; other failures propagate as-is.
    pub fn query_project_active(&self, cwd: &str) -> Result<Vec<Session>> {
        self.query(Some(cwd), false)
    }

    /// Run one scope×lifecycle query (spec §2.5). `scope_cwd = Some(cwd)` filters
    /// to the current project (Scope=Project); `None` spans all projects
    /// (Scope=All). `archived` selects the Lifecycle band (`false`=Active,
    /// `true`=Archived). Rows come back sorted `updated_at_ms DESC`.
    ///
    /// Each call is its own short read (spec §2.4): fresh connection, one query,
    /// drop. A `SQLITE_BUSY` timeout maps to the spec §11 busy message.
    pub fn query(&self, scope_cwd: Option<&str>, archived: bool) -> Result<Vec<Session>> {
        self.query_inner(scope_cwd, archived).map_err(map_busy)
    }

    fn query_inner(
        &self,
        scope_cwd: Option<&str>,
        archived: bool,
    ) -> rusqlite::Result<Vec<Session>> {
        let conn = open_readonly_raw(&self.db_path)?;
        let archived_flag = i64::from(archived);
        // The `cwd` predicate is the only shape difference across the four
        // combos (spec §2.5); the archived flag is always bound.
        let (sql, params) = match scope_cwd {
            Some(cwd) => (
                format!(
                    "{SELECT_SESSION} WHERE cwd = ?1 AND archived = ?2 \
                     AND source IN ('cli', 'vscode') ORDER BY updated_at_ms DESC"
                ),
                vec![
                    rusqlite::types::Value::Text(cwd.to_string()),
                    rusqlite::types::Value::Integer(archived_flag),
                ],
            ),
            None => (
                format!(
                    "{SELECT_SESSION} WHERE archived = ?1 \
                     AND source IN ('cli', 'vscode') ORDER BY updated_at_ms DESC"
                ),
                vec![rusqlite::types::Value::Integer(archived_flag)],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        stmt.query_map(rusqlite::params_from_iter(params), row_to_session)?
            .collect::<rusqlite::Result<Vec<_>>>()
    }
}

/// Map a `SQLITE_BUSY` timeout to the spec §11 busy message; pass anything else
/// through with context.
fn map_busy(err: rusqlite::Error) -> anyhow::Error {
    if let rusqlite::Error::SqliteFailure(e, _) = &err
        && e.code == rusqlite::ErrorCode::DatabaseBusy
    {
        return anyhow!(BUSY_MESSAGE);
    }
    anyhow::Error::new(err).context("querying threads")
}

/// Whether a query error is the normalized `SQLITE_BUSY` timeout.
pub fn is_busy_error(err: &anyhow::Error) -> bool {
    err.to_string() == BUSY_MESSAGE
}

/// Open a read-only connection with the exact flags/pragmas from spec §2.4,
/// adding path context on failure.
fn open_readonly(db_path: &Path) -> Result<Connection> {
    open_readonly_raw(db_path).with_context(|| format!("failed to open {}", db_path.display()))
}

/// [`open_readonly`] without the anyhow context wrapping, so callers can inspect
/// the raw `rusqlite::Error` (e.g. to detect `SQLITE_BUSY`).
fn open_readonly_raw(db_path: &Path) -> rusqlite::Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)?;
    conn.busy_timeout(Duration::from_millis(3000))?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

/// Verify `threads` exposes every [`REQUIRED_COLUMNS`] entry (spec §2.2).
fn validate_schema(conn: &Connection, db_path: &Path) -> Result<()> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(threads)")
        .context("querying threads schema")?;
    let present: std::collections::HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;

    if present.is_empty() {
        return Err(anyhow!(
            "unrecognized traex database schema at {}; tsm may be outdated",
            db_path.display()
        ));
    }
    for col in REQUIRED_COLUMNS {
        if !present.contains(*col) {
            return Err(anyhow!(
                "unrecognized traex database schema (missing threads.{col}); tsm may be outdated"
            ));
        }
    }
    Ok(())
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        first_user_message: row.get(2)?,
        cwd: row.get(3)?,
        updated_at: row.get(4)?,
        updated_at_ms: row.get(5)?,
        archived: row.get::<_, i64>(6)? != 0,
        archived_at: row.get(7)?,
        git_branch: row.get(8)?,
        model: row.get(9)?,
        tokens_used: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_dir_prefers_traecli_home() {
        let dir = resolve_cli_dir(Some("/a/cli"), Some("/b"), Path::new("/home/u"));
        assert_eq!(dir, PathBuf::from("/a/cli"));
    }

    #[test]
    fn cli_dir_falls_back_to_trae_home_cli() {
        let dir = resolve_cli_dir(None, Some("/b"), Path::new("/home/u"));
        assert_eq!(dir, PathBuf::from("/b/cli"));
    }

    #[test]
    fn cli_dir_defaults_to_dot_trae_cli() {
        let dir = resolve_cli_dir(None, None, Path::new("/home/u"));
        assert_eq!(dir, PathBuf::from("/home/u/.trae/cli"));
    }

    #[test]
    fn empty_env_values_are_ignored() {
        let dir = resolve_cli_dir(Some(""), Some(""), Path::new("/home/u"));
        assert_eq!(dir, PathBuf::from("/home/u/.trae/cli"));
    }

    #[test]
    fn picks_highest_generation() {
        let names = vec![
            "state_5.sqlite".to_string(),
            "state_6.sqlite".to_string(),
            "state_10.sqlite".to_string(),
            "goals_1.sqlite".to_string(),
            "state_.sqlite".to_string(),
            "state_5.sqlite-wal".to_string(),
            "notes.txt".to_string(),
        ];
        assert_eq!(
            pick_state_db(&names),
            Some((10, "state_10.sqlite".to_string()))
        );
    }

    #[test]
    fn no_state_db_returns_none() {
        let names = vec!["goals_1.sqlite".to_string(), "auth.json".to_string()];
        assert_eq!(pick_state_db(&names), None);
    }

    #[test]
    fn parse_generation_rejects_non_digits() {
        assert_eq!(parse_generation("state_5.sqlite"), Some(5));
        assert_eq!(parse_generation("state_05.sqlite"), Some(5));
        assert_eq!(parse_generation("state_.sqlite"), None);
        assert_eq!(parse_generation("state_5x.sqlite"), None);
        assert_eq!(parse_generation("state_5.sqlite-shm"), None);
        assert_eq!(parse_generation("goals_1.sqlite"), None);
    }

    /// Build a minimal `threads` table matching the real traex schema subset,
    /// then exercise validation + query through a read-only handle.
    fn seed_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                first_user_message TEXT NOT NULL DEFAULT '',
                cwd TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'cli',
                updated_at INTEGER NOT NULL,
                updated_at_ms INTEGER,
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER,
                git_branch TEXT,
                model TEXT,
                tokens_used INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO threads (id,title,first_user_message,cwd,updated_at,updated_at_ms,archived,archived_at,tokens_used)
             VALUES ('a','Older','fu','/proj',100,100000,0,NULL,10),
                    ('b','Newer','fu','/proj',200,200000,0,NULL,20),
                    ('c','Archived','fu','/proj',300,300000,1,3050,30),
                    ('d','Other','fu','/elsewhere',400,400000,0,NULL,40),
                    ('e','OtherArch','fu','/elsewhere',500,500000,1,5050,50)",
        )
        .unwrap();
    }

    #[test]
    fn query_excludes_subagent_sessions_like_traex_resume() {
        let tmp = std::env::temp_dir().join(format!("tsm-subagents-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        seed_db(&tmp);
        let conn = Connection::open(&tmp).unwrap();
        conn.execute(
            "INSERT INTO threads (
                id,title,first_user_message,cwd,source,updated_at,updated_at_ms,archived,tokens_used
             ) VALUES (
                'subagent','Review the diff','fu','/proj',
                '{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"parent\",\"depth\":1}}}',
                300,300000,0,30
             )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (
                id,title,first_user_message,cwd,source,updated_at,updated_at_ms,archived,tokens_used
             ) VALUES (
                'vscode','Interactive IDE session','fu','/proj','vscode',250,250000,0,25
             )",
            [],
        )
        .unwrap();
        drop(conn);

        let store = Store::open(tmp.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();

        assert_eq!(ids_of(&rows), vec!["vscode", "b", "a"]);

        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn query_project_active_filters_and_sorts() {
        let tmp = std::env::temp_dir().join(format!("tsm-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        seed_db(&tmp);

        let store = Store::open(tmp.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();

        // Only active rows in /proj, newest first.
        let ids: Vec<_> = rows.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);
        assert_eq!(rows[0].tokens_used, 20);

        std::fs::remove_file(&tmp).unwrap();
    }

    fn ids_of(rows: &[Session]) -> Vec<&str> {
        rows.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn query_covers_all_four_scope_lifecycle_combos() {
        let tmp = std::env::temp_dir().join(format!("tsm-combos-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        seed_db(&tmp);
        let store = Store::open(tmp.clone()).unwrap();

        // Project · Active
        assert_eq!(
            ids_of(&store.query(Some("/proj"), false).unwrap()),
            vec!["b", "a"]
        );
        // Project · Archived
        let arch = store.query(Some("/proj"), true).unwrap();
        assert_eq!(ids_of(&arch), vec!["c"]);
        assert_eq!(arch[0].archived_at, Some(3050));
        // All · Active (newest first, both projects)
        assert_eq!(
            ids_of(&store.query(None, false).unwrap()),
            vec!["d", "b", "a"]
        );
        // All · Archived
        assert_eq!(ids_of(&store.query(None, true).unwrap()), vec!["e", "c"]);

        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn open_rejects_missing_columns() {
        let tmp = std::env::temp_dir().join(format!("tsm-badschema-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let conn = Connection::open(&tmp).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY);")
            .unwrap();
        drop(conn);

        let err = Store::open(tmp.clone()).unwrap_err();
        assert!(
            err.to_string()
                .contains("unrecognized traex database schema")
        );
        std::fs::remove_file(&tmp).unwrap();
    }
}
