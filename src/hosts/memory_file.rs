//! Guarded read-modify-write for the instruction files squeez shares with the
//! user (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `SOUL.md`, …).
//!
//! Those files carry the user's own rules; squeez owns only the
//! `<!-- squeez:start --> … <!-- squeez:end -->` block inside them. Every
//! adapter used to do `read_to_string(..).unwrap_or_default()` followed by a
//! plain `fs::write(block + rest)`. `fs::write` truncates before it writes, so
//! a second `squeez init` starting in that window read an empty file, took it
//! for "no rules yet" and wrote the block alone — the user's rules were gone
//! (observed on an OpenCode `AGENTS.md`: 16 KB down to the 874-byte block).
//!
//! [`open`] and [`MemoryFile::commit`] close that hole four ways:
//!   1. a file that exists but cannot be read is an error, never "empty";
//!   2. a result that drops text from outside the squeez block is refused;
//!   3. the write is a temp file in the same directory renamed over the
//!      target, so no reader ever sees a truncated file;
//!   4. one writer at a time, via an OS lock on a sibling lock file. The OS
//!      drops the lock when its holder dies, so there is no stale lock to
//!      break.
//! On top of that `commit` re-reads the file and refuses to write when it
//! changed since `open` (a writer that does not take the lock).
//!
//! Detected errors fail closed. Non-cooperating writers can still race the
//! final comparison and rename: ordinary filesystems provide no atomic
//! compare-and-replace by content. This is not a CAS guarantee for editors.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Promotion marker: present in every error and log line of this module, so
/// a built binary can be checked for the guard.
pub const MARKER: &str = "WRITE_RACE_GUARD_V1";
const REVIEW_MARKER: &str = "WRITE_RACE_REVIEW_V2";

const BLOCK_START: &str = "<!-- squeez:start -->";
const BLOCK_END: &str = "<!-- squeez:end -->";

/// Longest `open` waits for another writer. Writers hold the lock for a few
/// milliseconds; anything longer means something is wrong, and skipping one
/// refresh is cheaper than stalling a session start.
const LOCK_WAIT: Duration = Duration::from_millis(3000);

/// A temp file older than this was left behind by a writer that died between
/// create and rename.
const TMP_STALE: Duration = Duration::from_secs(60);

const RENAME_ATTEMPTS: u32 = 20;
const RENAME_BACKOFF: Duration = Duration::from_millis(25);

/// An instruction file held under the writer lock, with the content it had
/// when the lock was taken. The lock is released on drop.
pub struct MemoryFile {
    path: PathBuf,
    /// `None`: the file did not exist.
    snapshot: Option<String>,
    _lock: File,
}

/// Lock `path` and read it. Fails — without touching anything — when the lock
/// cannot be taken in time or the file exists but cannot be read as UTF-8.
pub fn open(path: &Path) -> io::Result<MemoryFile> {
    open_with_wait(path, LOCK_WAIT)
}

/// As [`open`], with an explicit lock wait.
pub fn open_with_wait(path: &Path, wait: Duration) -> io::Result<MemoryFile> {
    let path = resolve(path)?;
    let lock = acquire(&lock_path(&path), wait).map_err(|e| {
        refuse(&path, e.kind(), &format!("writer lock not acquired ({e})"))
    })?;
    sweep_stale_tmp(&path);
    let snapshot = read_strict(&path)
        .map_err(|e| refuse(&path, e.kind(), &format!("exists but is unreadable ({e})")))?;
    validate_markers(&path, snapshot.as_deref().unwrap_or(""))?;
    Ok(MemoryFile { path, snapshot, _lock: lock })
}

impl MemoryFile {
    /// Content at the time the lock was taken; empty when the file is absent.
    pub fn existing(&self) -> &str {
        self.snapshot.as_deref().unwrap_or("")
    }

    /// Replace the file with `contents`, atomically. A no-op when nothing
    /// would change. Refuses — leaving the file as it is — when `contents`
    /// drops text from outside the squeez block, or when the file changed
    /// since [`open`].
    pub fn commit(self, contents: &str) -> io::Result<()> {
        if self.snapshot.as_deref() == Some(contents) {
            return Ok(());
        }
        validate_markers(&self.path, contents)?;
        if dropped_content(self.existing(), contents).is_some() {
            return Err(refuse(
                &self.path,
                io::ErrorKind::InvalidData,
                "new content would drop text outside the squeez block",
            ));
        }
        let now = read_strict(&self.path).map_err(|e| {
            refuse(&self.path, e.kind(), &format!("exists but is unreadable ({e})"))
        })?;
        if now != self.snapshot {
            return Err(refuse(
                &self.path,
                io::ErrorKind::Other,
                "file changed since it was read; not overwriting",
            ));
        }
        write_atomic(&self.path, contents.as_bytes(), &self.snapshot)
            .map_err(|e| refuse(&self.path, e.kind(), &format!("atomic write failed ({e})")))
    }
}

/// Multiple, nested, reversed or unterminated markers are ambiguous ownership.
/// Never guess where the owner's text ends in such a file.
fn validate_markers(path: &Path, contents: &str) -> io::Result<()> {
    let starts = contents.matches(BLOCK_START).count();
    let ends = contents.matches(BLOCK_END).count();
    let valid = (starts == 0 && ends == 0)
        || (starts == 1 && ends == 1 && contents.find(BLOCK_START) < contents.find(BLOCK_END));
    if valid { Ok(()) } else {
        Err(refuse(path, io::ErrorKind::InvalidData, "ambiguous squeez markers; not overwriting"))
    }
}

/// The first line outside `old`'s squeez block that `new` no longer carries
/// outside its own, or `None` when every such line survives in order.
/// Blank lines and surrounding whitespace are not content.
pub fn dropped_content(old: &str, new: &str) -> Option<String> {
    let old_outside = outside_block(old);
    let new_outside = outside_block(new);
    let mut kept = new_outside.lines().map(str::trim).filter(|l| !l.is_empty());
    for line in old_outside.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !kept.any(|k| k == line) {
            return Some(line.to_string());
        }
    }
    None
}

/// `s` without its squeez block. An unterminated block is not a block: the
/// text after a lone start marker stays.
fn outside_block(s: &str) -> String {
    let Some(start) = s.find(BLOCK_START) else {
        return s.to_string();
    };
    let Some(rel) = s[start..].find(BLOCK_END) else {
        return s.to_string();
    };
    let mut end = start + rel + BLOCK_END.len();
    if s[end..].starts_with("\r\n") {
        end += 2;
    } else if s[end..].starts_with('\n') {
        end += 1;
    }
    format!("{}{}", &s[..start], &s[end..])
}

/// Follow a symlink to the file it names, so the rename replaces that file
/// and not the link. A dangling link is an error, not an absent file.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path).map_err(|e| {
            refuse(path, e.kind(), &format!("is a symlink that does not resolve ({e})"))
        }),
        _ => Ok(path.to_path_buf()),
    }
}

/// `Ok(None)` only when the file does not exist. Anything else that keeps the
/// content from being read — a sharing violation, missing permission, bytes
/// that are not UTF-8 — is an error.
fn read_strict(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{suffix}"))
}

/// The lock lives next to the file it guards, so every process agrees on it
/// whatever its environment says about temp or home directories. It is
/// created once, stays empty and is never removed.
fn lock_path(path: &Path) -> PathBuf {
    sibling(path, "squeez-lock")
}

fn acquire(lock_path: &Path, wait: Duration) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "another squeez process holds it",
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8], snapshot: &Option<String>) -> io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = sibling(
        path,
        &format!(
            "squeez-tmp-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ),
    );
    write_atomic_to(path, bytes, snapshot, &tmp)
}

fn write_atomic_to(path: &Path, bytes: &[u8], snapshot: &Option<String>, tmp: &Path) -> io::Result<()> {
    let permissions = match std::fs::metadata(path) {
        Ok(meta) => {
            let permissions = meta.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "target is read-only"));
            }
            Some(permissions)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // Do not clean up a path we failed to create: it belongs to someone else.
    let mut file = options.open(tmp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            std::fs::set_permissions(tmp, permissions)?;
        }
        #[cfg(not(unix))]
        let _ = permissions;
        rename_over(tmp, path, snapshot)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Rename with a short retry: on Windows the target can be briefly held by a
/// reader that did not open it for shared delete.
fn rename_over(tmp: &Path, path: &Path, snapshot: &Option<String>) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        // Compare AFTER staging/fsync and before EVERY retry. A sharing
        // violation can otherwise extend a stale overwrite window by 500 ms.
        if &read_strict(path)? != snapshot {
            return Err(io::Error::other("file changed since it was read; not overwriting"));
        }
        match std::fs::rename(tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                attempt += 1;
                if attempt >= RENAME_ATTEMPTS {
                    return Err(e);
                }
                std::thread::sleep(RENAME_BACKOFF);
            }
        }
    }
}

/// Remove temp files a dead writer left next to `path`. Called under the
/// lock, so a live writer's temp file is never older than [`TMP_STALE`].
fn sweep_stale_tmp(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let prefix = match sibling(path, "squeez-tmp-").file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => return,
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .map(|age| age >= TMP_STALE)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Where refusals are recorded. Outside every host's config tree: OpenCode
/// watches its own and reloads on writes there.
pub fn log_path() -> PathBuf {
    std::env::temp_dir().join("squeez-write-race-guard.log")
}

/// Build the error for a refused write and record it: stderr for whoever is
/// watching, the log file for everyone else (`squeez init` discards the
/// adapter's result).
fn refuse(path: &Path, kind: io::ErrorKind, why: &str) -> io::Error {
    let msg = format!("squeez {MARKER} {REVIEW_MARKER}: {} left untouched: {why}", path.display());
    eprintln!("{msg}");
    if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(log, "{ts} pid={} {msg}", std::process::id());
    }
    io::Error::new(kind, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    const RULES: &str = "# Owner rules\n\n## Shell policy\nuse bash\n\n## MCP servers\nuse them\n";

    fn tmp_dir(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "squeez-memfile-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn block(body: &str) -> String {
        format!("{BLOCK_START}\n## squeez — session context\n{body}\n{BLOCK_END}\n")
    }

    /// What every adapter does with the content it read.
    fn render(existing: &str, body: &str) -> String {
        format!("{}\n{}", block(body), outside_block(existing).trim_start())
    }

    fn inject(path: &Path, body: &str) -> io::Result<()> {
        let file = open(path)?;
        let contents = render(file.existing(), body);
        file.commit(&contents)
    }

    /// The pre-fix adapter body, verbatim in what matters: a failed read is
    /// an empty file, and the write is a plain truncating `fs::write`.
    fn legacy_inject(path: &Path, body: &str) -> io::Result<()> {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let contents = render(&existing, body);
        std::fs::write(path, contents)
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("squeez-tmp-"))
            .collect()
    }

    // ── 1. an existing file that cannot be read is never "empty" ───────────

    #[test]
    fn absent_file_reads_as_empty_and_is_created() {
        let dir = tmp_dir("absent");
        let path = dir.join("AGENTS.md");
        let file = open(&path).unwrap();
        assert_eq!(file.existing(), "");
        file.commit(&block("a")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), block("a"));
    }

    #[test]
    fn non_utf8_file_is_an_error_and_stays_byte_identical() {
        let dir = tmp_dir("nonutf8");
        let path = dir.join("AGENTS.md");
        let bytes = b"# rules\n\xff\xfe not utf-8\n".to_vec();
        std::fs::write(&path, &bytes).unwrap();
        // The legacy read turned exactly this into an empty string.
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), "");
        let err = inject(&path, "a").unwrap_err();
        assert!(err.to_string().contains(MARKER), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(leftovers(&dir).is_empty());
    }

    #[test]
    fn unreadable_path_is_an_error_and_nothing_is_written() {
        // A directory where the file should be: it exists, and reading it fails.
        let dir = tmp_dir("isdir");
        let path = dir.join("AGENTS.md");
        std::fs::create_dir_all(path.join("keep")).unwrap();
        assert!(inject(&path, "a").is_err());
        assert!(path.join("keep").is_dir());
        assert!(leftovers(&dir).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn file_held_exclusively_is_an_error_and_keeps_its_content() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tmp_dir("sharing");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        let held = OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        // The legacy read turned this sharing violation into an empty string.
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), "");
        let err = open(&path).err().expect("open must fail");
        assert!(err.to_string().contains("unreadable"), "{err}");
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), RULES);
        assert!(leftovers(&dir).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_an_error_and_is_not_replaced() {
        let dir = tmp_dir("dangling");
        let path = dir.join("AGENTS.md");
        std::os::unix::fs::symlink(dir.join("gone.md"), &path).unwrap();
        assert!(inject(&path, "a").is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_written_through_not_replaced() {
        let dir = tmp_dir("symlink");
        let real = dir.join("real.md");
        let path = dir.join("AGENTS.md");
        std::fs::write(&real, RULES).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        inject(&path, "a").unwrap();
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("## Shell policy"));
    }

    // ── 2. a result that drops outside content is refused ──────────────────

    #[test]
    fn dropped_content_names_the_first_missing_line() {
        let old = format!("{}\n{RULES}", block("old"));
        assert_eq!(dropped_content(&old, &render(&old, "new")), None);
        assert_eq!(dropped_content(&old, &block("new")).as_deref(), Some("# Owner rules"));
        let partial = format!("{}\n# Owner rules\n\n## Shell policy\nuse bash\n", block("new"));
        assert_eq!(dropped_content(&old, &partial).as_deref(), Some("## MCP servers"));
    }

    #[test]
    fn dropped_content_ignores_block_changes_and_blank_lines() {
        let old = format!("intro\n\n\n{}tail\n", block("one\ntwo\nthree"));
        let new = format!("{}\nintro\ntail\n", block("x"));
        assert_eq!(dropped_content(&old, &new), None);
        assert_eq!(dropped_content("", &block("x")), None);
        // Adding text outside the block is not a drop.
        assert_eq!(dropped_content("a\n", "front\na\nback\n"), None);
    }

    #[test]
    fn dropped_content_rejects_rules_moved_inside_the_block() {
        let old = format!("{}\n{RULES}", block("old"));
        let swallowed = block(&format!("new\n{RULES}"));
        assert!(dropped_content(&old, &swallowed).is_some());
    }

    #[test]
    fn dropped_content_keeps_text_after_an_unterminated_block() {
        let old = format!("{BLOCK_START}\nhalf a block\n{RULES}");
        assert_eq!(dropped_content(&old, &block("new")).as_deref(), Some(BLOCK_START));
        assert_eq!(dropped_content(&old, &format!("{}\n{old}", block("new"))), None);
    }

    #[test]
    fn outside_block_survives_multibyte_text_right_after_the_end_marker() {
        let s = format!("{BLOCK_START}\nx\n{BLOCK_END}é rules\n");
        assert_eq!(outside_block(&s), "é rules\n");
    }

    #[test]
    fn commit_refuses_a_block_only_result_and_keeps_the_file() {
        let dir = tmp_dir("refuse");
        let path = dir.join("AGENTS.md");
        let before = format!("{}\n{RULES}", block("old"));
        std::fs::write(&path, &before).unwrap();
        let file = open(&path).unwrap();
        // Exactly what the race produced: the block and nothing else.
        let err = file.commit(&block("new")).unwrap_err();
        assert!(err.to_string().contains(MARKER), "{err}");
        assert!(err.to_string().contains("would drop"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(leftovers(&dir).is_empty());
    }

    #[test]
    fn refusal_is_logged() {
        let dir = tmp_dir("logged");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        assert!(open(&path).unwrap().commit(&block("new")).is_err());
        let log = std::fs::read_to_string(log_path()).unwrap();
        let needle = path.display().to_string();
        assert!(
            log.lines().any(|l| l.contains(MARKER) && l.contains(&needle) && l.contains("would drop")),
            "no log line for {needle}"
        );
    }

    #[test]
    fn commit_refuses_when_the_file_changed_since_open() {
        let dir = tmp_dir("cas");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        let file = open(&path).unwrap();
        let contents = render(file.existing(), "a");
        // A writer that ignores the lock adds a rule in between.
        let edited = format!("{RULES}## Added meanwhile\nkeep me\n");
        std::fs::write(&path, &edited).unwrap();
        let err = file.commit(&contents).unwrap_err();
        assert!(err.to_string().contains("changed since"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    }

    #[test]
    fn commit_refuses_when_the_file_appeared_since_open() {
        let dir = tmp_dir("appeared");
        let path = dir.join("AGENTS.md");
        let file = open(&path).unwrap();
        std::fs::write(&path, RULES).unwrap();
        assert!(file.commit(&block("a")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), RULES);
    }

    // ── 3. the write is atomic ─────────────────────────────────────────────

    #[test]
    fn commit_replaces_content_and_leaves_no_temp_file() {
        let dir = tmp_dir("atomic");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        inject(&path, "a").unwrap();
        inject(&path, "b").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, format!("{}\n{RULES}", block("b")));
        assert_eq!(body.matches(BLOCK_START).count(), 1);
        assert!(leftovers(&dir).is_empty());
    }

    #[test]
    fn unchanged_content_is_not_rewritten() {
        let dir = tmp_dir("unchanged");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        inject(&path, "a").unwrap();
        // Read-only: any write, including a rename over it, would now fail.
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        inject(&path, "a").expect("identical content must not be written");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn failed_rename_keeps_the_original_and_removes_the_temp_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tmp_dir("renamefail");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        // Shared for reading only: the guard can read it, nothing can replace it.
        let held = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let err = inject(&path, "a").unwrap_err();
        assert!(err.to_string().contains("atomic write failed"), "{err}");
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), RULES);
        assert!(leftovers(&dir).is_empty());
    }

    #[test]
    fn readers_never_see_a_truncated_file() {
        let dir = tmp_dir("readers");
        let path = dir.join("AGENTS.md");
        let rules = format!("{RULES}{}\n## END SENTINEL\n", "filler line of owner rules\n".repeat(4000));
        std::fs::write(&path, &rules).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (path, stop, rules) = (path.clone(), stop.clone(), rules.clone());
            std::thread::spawn(move || {
                let mut reads = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    // A reader may lose the open to a rename in flight; what it
                    // must never get is content without the rules.
                    if let Ok(seen) = std::fs::read_to_string(&path) {
                        assert!(seen.ends_with(&rules), "torn read: {} bytes", seen.len());
                        reads += 1;
                    }
                }
                reads
            })
        };
        for i in 0..300 {
            inject(&path, &format!("round {i}")).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0);
        assert!(leftovers(&dir).is_empty());
    }

    #[test]
    fn stale_temp_files_are_swept_and_fresh_ones_are_left() {
        let dir = tmp_dir("sweep");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        let stale = dir.join(".AGENTS.md.squeez-tmp-1-0");
        let fresh = dir.join(".AGENTS.md.squeez-tmp-2-0");
        let other = dir.join(".OTHER.md.squeez-tmp-1-0");
        for f in [&stale, &fresh, &other] {
            std::fs::write(f, "x").unwrap();
        }
        let old = SystemTime::now() - Duration::from_secs(3600);
        for f in [&stale, &other] {
            File::options().write(true).open(f).unwrap().set_modified(old).unwrap();
        }
        drop(open(&path).unwrap());
        assert!(!stale.exists(), "stale temp file must be removed");
        assert!(fresh.exists(), "a live writer's temp file must stay");
        assert!(other.exists(), "another file's temp file is not ours");
    }

    // ── 4. one writer at a time ────────────────────────────────────────────

    #[test]
    fn second_writer_waits_then_times_out_and_gets_in_after_release() {
        let dir = tmp_dir("lock");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        let first = open(&path).unwrap();
        let started = Instant::now();
        let err = open_with_wait(&path, Duration::from_millis(150)).err().expect("must time out");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), RULES);
        drop(first);
        open_with_wait(&path, Duration::from_millis(150)).expect("lock is free after drop");
    }

    #[test]
    fn waiting_writer_sees_the_first_writers_result() {
        let dir = tmp_dir("handoff");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        let first = open(&path).unwrap();
        let waiter = {
            let path = path.clone();
            std::thread::spawn(move || open(&path).unwrap().existing().to_string())
        };
        std::thread::sleep(Duration::from_millis(100));
        let contents = render(first.existing(), "first");
        first.commit(&contents).unwrap();
        assert_eq!(waiter.join().unwrap(), contents);
    }

    #[test]
    fn lock_file_sits_beside_the_target_and_stays_empty() {
        let dir = tmp_dir("lockfile");
        let path = dir.join("AGENTS.md");
        inject(&path, "a").unwrap();
        let lock = dir.join(".AGENTS.md.squeez-lock");
        assert_eq!(lock_path(&path), lock);
        assert_eq!(std::fs::metadata(&lock).unwrap().len(), 0);
    }

    // ── the original failure, reproduced ───────────────────────────────────

    const WRITERS: usize = 8;
    const BIG_RULES_LINES: usize = 6000;

    fn big_rules() -> String {
        format!(
            "{RULES}{}## END SENTINEL — owner rules end here\n",
            "- a rule the owner wrote and squeez must never lose\n".repeat(BIG_RULES_LINES)
        )
    }

    /// What `WRITERS` threads injecting into one file did to the rules.
    struct Outcome {
        /// A writer read the file and the rules were not in it.
        seen_missing: bool,
        /// The rules are not in the file once every writer has finished.
        lost: bool,
        /// Injects that returned `Ok`.
        written: usize,
    }

    /// Run `WRITERS` threads, `rounds` injects each, against one file.
    fn hammer(
        path: &Path,
        rules: &str,
        rounds: usize,
        inject_fn: fn(&Path, &str) -> io::Result<()>,
    ) -> Outcome {
        let seen_missing = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicU64::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(WRITERS));
        let handles: Vec<_> = (0..WRITERS)
            .map(|w| {
                let (path, rules, seen_missing, barrier) =
                    (path.to_path_buf(), rules.to_string(), seen_missing.clone(), barrier.clone());
                let written = written.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for round in 0..rounds {
                        if seen_missing.load(Ordering::Relaxed) {
                            return;
                        }
                        // Every writer renders a different block, as two
                        // sessions with different summaries do.
                        if inject_fn(&path, &format!("writer {w} round {round}")).is_ok() {
                            written.fetch_add(1, Ordering::Relaxed);
                        }
                        if let Ok(seen) = std::fs::read_to_string(&path) {
                            if !seen.ends_with(&rules) {
                                seen_missing.store(true, Ordering::Relaxed);
                                return;
                            }
                        }
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        // Every writer is done, so this read is the settled file.
        let lost = !std::fs::read_to_string(path).unwrap_or_default().ends_with(rules);
        Outcome {
            seen_missing: seen_missing.load(Ordering::Relaxed),
            lost,
            written: written.load(Ordering::Relaxed) as usize,
        }
    }

    #[test]
    fn concurrent_injects_never_lose_content_outside_the_block() {
        let dir = tmp_dir("race-fixed");
        let path = dir.join("AGENTS.md");
        let rules = big_rules();
        std::fs::write(&path, format!("{}\n{rules}", block("seed"))).unwrap();
        let outcome = hammer(&path, &rules, 250, inject);
        assert!(!outcome.lost, "owner rules lost under concurrent inject");
        assert!(!outcome.seen_missing, "a writer saw the file without the owner rules");
        // The guard may turn a writer away, but it must not pass by never writing.
        eprintln!("guarded injects written: {} of {}", outcome.written, WRITERS * 250);
        assert!(outcome.written * 10 >= WRITERS * 250 * 9, "only {} injects landed", outcome.written);
        let end = std::fs::read_to_string(&path).unwrap();
        assert_eq!(end.matches(BLOCK_START).count(), 1);
        assert_eq!(outside_block(&end).trim_start(), rules, "owner rules not byte-identical");
        assert!(leftovers(&dir).is_empty());
    }

    /// The same scenario against the pre-fix code path must lose the rules —
    /// otherwise the test above proves nothing. Unguarded writers damage the
    /// file two ways: the wipe (one reads the file while another has it
    /// truncated, and writes the block alone) and a torn file (two truncating
    /// writes interleave). The wipe is the reported failure, so that is the
    /// one this test insists on seeing.
    #[test]
    fn legacy_inject_loses_content_under_the_same_load() {
        let rules = big_rules();
        let (mut wiped, mut torn, mut intact) = (0, 0, 0);
        // The race is probabilistic; give it several files' worth of chances.
        for _ in 0..60 {
            let dir = tmp_dir("race-legacy");
            let path = dir.join("AGENTS.md");
            std::fs::write(&path, format!("{}\n{rules}", block("seed"))).unwrap();
            if !hammer(&path, &rules, 250, legacy_inject).lost {
                intact += 1;
                continue;
            }
            let after = std::fs::read_to_string(&path).unwrap_or_default();
            if after.contains("## END SENTINEL") {
                torn += 1;
                continue;
            }
            wiped += 1;
            eprintln!(
                "legacy code wiped the owner rules: file settled at {} bytes, rules alone are {} \
                 (before that: {torn} torn, {intact} intact)",
                after.len(),
                rules.len()
            );
            assert!(after.len() < 1024, "a wipe leaves the squeez block and nothing else");
            assert!(after.starts_with(BLOCK_START));
            break;
        }
        assert!(
            wiped > 0,
            "the pre-fix code path did not reproduce the wipe ({torn} torn, {intact} intact)"
        );
    }

    #[test]
    fn review_refusal_never_discloses_owner_text() {
        let dir = tmp_dir("review-secret");
        let path = dir.join("AGENTS.md");
        let secret = "PRIVATE_TOKEN_REVIEW_9fa187";
        std::fs::write(&path, secret).unwrap();
        let err = open(&path).unwrap().commit(&block("new")).unwrap_err();
        assert!(!err.to_string().contains(secret));
        let log = std::fs::read_to_string(log_path()).unwrap();
        assert!(!log.lines().filter(|l| l.contains(&path.display().to_string()))
            .any(|l| l.contains(secret)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), secret);
    }

    #[test]
    fn review_temp_collision_never_deletes_an_unowned_file() {
        let dir = tmp_dir("review-collision");
        let path = dir.join("AGENTS.md");
        let tmp = dir.join(".AGENTS.md.squeez-tmp-collision");
        std::fs::write(&path, RULES).unwrap();
        std::fs::write(&tmp, "unowned recovery data").unwrap();
        let err = write_atomic_to(&path, block("new").as_bytes(), &Some(RULES.to_string()), &tmp)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "unowned recovery data");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), RULES);
    }

    #[cfg(unix)]
    #[test]
    fn review_unix_permissions_are_preserved() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp_dir("review-mode");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        inject(&path, "new").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn review_ambiguous_markers_are_refused_without_modification() {
        let dir = tmp_dir("review-markers");
        let path = dir.join("AGENTS.md");
        for before in [
            format!("{BLOCK_START}\nowner\n{BLOCK_START}\ninner\n{BLOCK_END}\n"),
            format!("{}{}owner\n", block("one"), block("two")),
            format!("{BLOCK_END}\nowner\n{BLOCK_START}\n"),
            format!("{BLOCK_START}\nowner\n"),
        ] {
            std::fs::write(&path, &before).unwrap();
            assert!(inject(&path, "replacement").is_err(), "ambiguous file accepted");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        }
    }

    #[test]
    fn review_readonly_file_is_not_replaced() {
        let dir = tmp_dir("review-readonly");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
        let result = inject(&path, "new");
        let after = std::fs::read_to_string(&path).unwrap();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
        assert!(result.is_err());
        assert_eq!(after, RULES);
        assert!(leftovers(&dir).is_empty());
    }

    #[test]
    fn review_ten_megabytes_crlf_unicode_and_bom_survive() {
        let dir = tmp_dir("review-large");
        let path = dir.join("AGENTS.md");
        let rules = format!("\u{feff}# 私有 правила 🦀\r\n{}END\r\n",
            "owner content 0123456789\r\n".repeat(440_000));
        assert!(rules.len() > 10_000_000);
        for before in [rules.clone(), format!("{rules}{}", block("end")),
            format!("{}{rules}", block("start"))] {
            std::fs::write(&path, &before).unwrap();
            inject(&path, "new").unwrap();
            assert_eq!(outside_block(&std::fs::read_to_string(&path).unwrap()).trim_start(), rules);
        }
        assert!(leftovers(&dir).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn review_external_writer_during_rename_retry_is_not_overwritten() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tmp_dir("review-retry-race");
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, RULES).unwrap();
        // Allow external writes and reads, but block replacement until released.
        let held = OpenOptions::new().read(true).share_mode(3).open(&path).unwrap();
        let worker = {
            let path = path.clone();
            std::thread::spawn(move || inject(&path, "replacement"))
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while leftovers(&dir).is_empty() {
            assert!(Instant::now() < deadline, "writer did not stage temp");
            std::thread::yield_now();
        }
        // Wait for staging to finish and the first rename to hit sharing violation.
        std::thread::sleep(Duration::from_millis(75));
        let external = format!("{RULES}NEW EXTERNAL RULE\n");
        std::fs::write(&path, &external).unwrap();
        drop(held);
        let result = worker.join().unwrap();
        assert!(result.is_err(), "guard overwrote external writer in retry window");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
        assert!(leftovers(&dir).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn review_kill_child() {
        if let Some(path) = std::env::var_os("SQUEEZ_REVIEW_KILL_PATH") {
            inject(Path::new(&path), "child replacement").unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn review_killed_writer_keeps_target_and_releases_lock() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tmp_dir("review-kill");
        let path = dir.join("AGENTS.md");
        let rules = format!("{RULES}{}", "owner line\n".repeat(1_000_000));
        std::fs::write(&path, &rules).unwrap();
        let held = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "hosts::memory_file::tests::review_kill_child", "--nocapture"])
            .env("SQUEEZ_REVIEW_KILL_PATH", &path)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while leftovers(&dir).is_empty() {
            if Instant::now() >= deadline {
                let _ = child.kill(); let _ = child.wait();
                panic!("child did not enter atomic write");
            }
            std::thread::yield_now();
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), rules);
        let abandoned = leftovers(&dir);
        assert_eq!(abandoned.len(), 1);
        File::options().write(true).open(dir.join(&abandoned[0])).unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        inject(&path, "after crash").unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().ends_with(&rules));
        assert!(leftovers(&dir).is_empty());
    }
}
