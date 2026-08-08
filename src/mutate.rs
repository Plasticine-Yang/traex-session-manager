//! Mutate — the unified batch engine behind delete / archive / unarchive (spec
//! §3, §6). tsm never writes the store for these; it shells out to `traex <op>
//! <uuid>` (delete adds `--force`, spec §3.2) and branches on the exit code
//! (spec §3.3). One `BatchJob` drives one spawn/progress/failure pipeline; the
//! three ops differ only in the verb, the subcommand, and the `--force` flag
//! (spec §6.2).
//!
//! Fan-out is a fixed `std::thread` pool of 4 (spec §6.4) fed by an `mpsc`
//! channel — **not** tokio (this fans out external processes, not in-process
//! async I/O; spec §6.4 / §9.2). The pool size is the R2 ceiling, hardcoded.

use std::collections::VecDeque;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Fixed concurrency ceiling for the fan-out pool (spec §6.4, R2 ceiling).
pub const POOL_SIZE: usize = 4;

/// The three store-changing operations. All three ride the same pipeline; only
/// the verb, the traex subcommand, and the `--force` flag differ (spec §6.2).
/// `Archive`/`Unarchive` are wired to the `a` key by ticket 06, gated by the
/// lifecycle view (spec §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `traex delete <uuid> --force` — irreversible, also deletes rollout files.
    Delete,
    /// `traex archive <uuid>` — reversible, moves rollout files.
    Archive,
    /// `traex unarchive <uuid>` — the reverse of archive.
    Unarchive,
}

impl Op {
    /// The traex subcommand (`delete` / `archive` / `unarchive`).
    pub fn subcommand(self) -> &'static str {
        match self {
            Op::Delete => "delete",
            Op::Archive => "archive",
            Op::Unarchive => "unarchive",
        }
    }

    /// Present-progressive verb for the progress modal (`Deleting…`, spec §6.5).
    pub fn progress_verb(self) -> &'static str {
        match self {
            Op::Delete => "Deleting",
            Op::Archive => "Archiving",
            Op::Unarchive => "Unarchiving",
        }
    }

    /// Past-tense verb for the success toast (`Deleted N.`, spec §6.6).
    pub fn past_verb(self) -> &'static str {
        match self {
            Op::Delete => "Deleted",
            Op::Archive => "Archived",
            Op::Unarchive => "Unarchived",
        }
    }
}

/// A unit of batch work: one op applied to a set of session ids (spec §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchJob {
    pub op: Op,
    pub ids: Vec<String>,
}

/// The result of one worker running one id: `error = None` on exit 0 (spec
/// §3.3), otherwise the extracted stderr line (or a spawn-failure message).
#[derive(Debug, Clone)]
pub struct Outcome {
    pub id: String,
    /// `None` = success; `Some(msg)` = failure, message shown in the result face.
    pub error: Option<String>,
}

/// Per-id executor. Returns `None` on success, `Some(error line)` on failure.
/// Boxed so tests can inject a deterministic runner instead of spawning traex.
pub type Runner = Arc<dyn Fn(Op, &str) -> Option<String> + Send + Sync>;

/// A running batch: the outcome stream plus the cancel switch and worker
/// handles. Dropping the handle after joining is fine; the app keeps it in
/// [`crate::app::App`] for the duration of the progress modal.
pub struct BatchHandle {
    rx: Receiver<Outcome>,
    cancel: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl BatchHandle {
    /// Drain all outcomes available right now without blocking (spec §6.1: the
    /// app polls this each frame to advance the progress modal).
    pub fn drain_ready(&self) -> Vec<Outcome> {
        self.rx.try_iter().collect()
    }

    /// Request cancellation: stop dispatching new ids, but do **not** kill the
    /// ≤4 in-flight workers (spec §6.8 — a half-deleted traex is riskier than
    /// letting a ~2s op finish). Already-queued-but-unstarted ids simply never
    /// run and surface as cancelled.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// True once every worker thread has exited (all ids either ran or were
    /// skipped by cancellation).
    pub fn is_finished(&self) -> bool {
        self.handles.iter().all(JoinHandle::is_finished)
    }
}

/// Spawn the fan-out pool for `job`, executing each id through `runner`. Returns
/// immediately with a [`BatchHandle`]; outcomes stream over the channel as
/// workers finish (spec §6.4).
pub fn spawn(job: BatchJob, runner: Runner) -> BatchHandle {
    let op = job.op;
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let queue = Arc::new(Mutex::new(VecDeque::from(job.ids)));

    let n = POOL_SIZE.min(queue.lock().unwrap().len().max(1));
    let mut handles = Vec::with_capacity(n);
    for _ in 0..n {
        let tx = tx.clone();
        let cancel = Arc::clone(&cancel);
        let queue = Arc::clone(&queue);
        let runner = Arc::clone(&runner);
        handles.push(std::thread::spawn(move || {
            loop {
                // Check cancellation before claiming the next id, so a cancel
                // stops dispatch without disturbing the in-flight op (spec §6.8).
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let id = {
                    let mut q = queue.lock().unwrap();
                    q.pop_front()
                };
                let Some(id) = id else { break };
                let error = runner(op, &id);
                // The receiver may already be gone if the app tore down; ignore.
                if tx.send(Outcome { id, error }).is_err() {
                    break;
                }
            }
        }));
    }

    BatchHandle {
        rx,
        cancel,
        handles,
    }
}

/// The production runner: spawn `traex <op> <uuid>` (delete adds `--force`,
/// spec §3.2) and branch on the exit code (spec §3.3). A spawn failure (e.g.
/// `traex` not in PATH) is itself a failure item, not a crash (spec §6.6/§11).
pub fn traex_runner() -> Runner {
    Arc::new(run_traex)
}

/// Probe whether an executable named `traex` is reachable through `PATH`
/// without invoking it (spec §11).
pub fn traex_available() -> bool {
    command_available("traex", std::env::var_os("PATH").as_deref())
}

fn command_available(command: &str, path: Option<&std::ffi::OsStr>) -> bool {
    let Some(path) = path else { return false };
    std::env::split_paths(path).any(|dir| is_executable(&dir.join(command)))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Run one traex op against one id, returning `None` on success or the error
/// line on failure.
fn run_traex(op: Op, id: &str) -> Option<String> {
    let mut cmd = Command::new("traex");
    cmd.arg(op.subcommand()).arg(id);
    // delete has no TTY under tsm, so it must carry --force or it aborts (spec §3.2).
    if op == Op::Delete {
        cmd.arg("--force");
    }
    match cmd.output() {
        Ok(out) if out.status.success() => None,
        Ok(out) => Some(extract_error(&out.stderr, out.status.code())),
        // spawn failure = failure item, surfaced in the result face (spec §6.6/§11).
        Err(e) => Some(format!("Error: could not run traex: {e}")),
    }
}

/// Pull the human-readable failure line out of traex's stderr. traex emits a
/// single `Error: <msg>` line on failure (spec §3.3); fall back to the exit code
/// if stderr was empty.
fn extract_error(stderr: &[u8], code: Option<i32>) -> String {
    let text = String::from_utf8_lossy(stderr);
    for line in text.lines() {
        let line = line.trim();
        if !line.is_empty() {
            return line.to_string();
        }
    }
    match code {
        Some(c) => format!("Error: traex exited with status {c}"),
        None => "Error: traex terminated by signal".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// Drain a handle to completion, joining workers, and return
    /// `(successes, failures)` by id.
    fn run_to_completion(handle: BatchHandle) -> (Vec<String>, Vec<(String, String)>) {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        // Block on the channel until every worker has dropped its sender.
        for out in handle.rx.iter() {
            match out.error {
                None => succeeded.push(out.id),
                Some(e) => failed.push((out.id, e)),
            }
        }
        for h in handle.handles {
            h.join().unwrap();
        }
        (succeeded, failed)
    }

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("id-{i}")).collect()
    }

    #[test]
    fn processes_every_id_and_partitions_success_failure() {
        // Odd ids fail with a canned error; even ids succeed.
        let runner: Runner = Arc::new(|_op, id| {
            let n: usize = id.rsplit('-').next().unwrap().parse().unwrap();
            (n % 2 == 1).then(|| format!("Error: boom {id}"))
        });
        let job = BatchJob {
            op: Op::Delete,
            ids: ids(6),
        };
        let (mut ok, mut bad) = run_to_completion(spawn(job, runner));
        ok.sort();
        bad.sort();
        assert_eq!(ok, vec!["id-0", "id-2", "id-4"]);
        assert_eq!(
            bad,
            vec![
                ("id-1".to_string(), "Error: boom id-1".to_string()),
                ("id-3".to_string(), "Error: boom id-3".to_string()),
                ("id-5".to_string(), "Error: boom id-5".to_string()),
            ]
        );
    }

    #[test]
    fn concurrency_never_exceeds_pool_size() {
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let runner: Runner = {
            let live = Arc::clone(&live);
            let peak = Arc::clone(&peak);
            Arc::new(move |_op, _id| {
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                live.fetch_sub(1, Ordering::SeqCst);
                None
            })
        };
        let job = BatchJob {
            op: Op::Archive,
            ids: ids(16),
        };
        let (ok, bad) = run_to_completion(spawn(job, runner));
        assert_eq!(ok.len(), 16);
        assert!(bad.is_empty());
        assert!(
            peak.load(Ordering::SeqCst) <= POOL_SIZE,
            "peak concurrency {} exceeded pool size {POOL_SIZE}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn cancel_stops_dispatching_new_work() {
        // Each worker blocks on a gate the first time so we can cancel before the
        // rest of the queue is claimed. With a 4-worker pool, at most 4 ids get
        // claimed before cancel takes effect; the remaining must never run.
        let started = Arc::new(AtomicUsize::new(0));
        let runner: Runner = {
            let started = Arc::clone(&started);
            Arc::new(move |_op, _id| {
                started.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(30));
                None
            })
        };
        let job = BatchJob {
            op: Op::Delete,
            ids: ids(100),
        };
        let handle = spawn(job, runner);
        // Let the first wave claim their ids, then cancel.
        std::thread::sleep(Duration::from_millis(10));
        handle.cancel();
        let (ok, _bad) = run_to_completion(handle);
        // Far fewer than 100 ran — cancellation stopped dispatch (spec §6.8). The
        // in-flight ≤4 finished, so a small count is expected.
        assert!(
            ok.len() <= POOL_SIZE,
            "cancel let {} ids through, expected <= {POOL_SIZE}",
            ok.len()
        );
        assert!(started.load(Ordering::SeqCst) <= POOL_SIZE);
    }

    #[test]
    fn empty_job_finishes_cleanly() {
        let runner: Runner = Arc::new(|_op, _id| None);
        let job = BatchJob {
            op: Op::Unarchive,
            ids: vec![],
        };
        let (ok, bad) = run_to_completion(spawn(job, runner));
        assert!(ok.is_empty());
        assert!(bad.is_empty());
    }

    #[test]
    fn extract_error_prefers_stderr_line() {
        assert_eq!(
            extract_error(b"\nError: no rollout found\n", Some(1)),
            "Error: no rollout found"
        );
        assert_eq!(
            extract_error(b"", Some(2)),
            "Error: traex exited with status 2"
        );
        assert_eq!(
            extract_error(b"   ", None),
            "Error: traex terminated by signal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_probe_requires_an_executable_file() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("tsm-traex-probe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("traex");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let path = OsString::from(&root);

        assert!(command_available("traex", Some(path.as_os_str())));
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!command_available("traex", Some(path.as_os_str())));
        assert!(!command_available("traex", None));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn op_verbs_and_subcommands() {
        assert_eq!(Op::Delete.subcommand(), "delete");
        assert_eq!(Op::Delete.progress_verb(), "Deleting");
        assert_eq!(Op::Delete.past_verb(), "Deleted");
        assert_eq!(Op::Archive.subcommand(), "archive");
        assert_eq!(Op::Unarchive.subcommand(), "unarchive");
    }
}
