//! Replacing a file whole: a reader, or the next start, sees the old bytes or
//! the new ones, never an empty or half-written file.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io};

/// Replace the contents of `path` with `bytes`. They go to a temp file beside
/// the real file, `.<name>.<pid>-<seq>.tmp`, which is then renamed over it,
/// so a process killed partway leaves the old file whole. The pid keeps a
/// live board and a `--once` writing the same cache entry apart; `seq`
/// counts this process's writes, so two threads saving the same file (the
/// dump tests share one config dir) never share a temp file either.
///
/// A symlinked `path` stays a symlink: the write lands beside the file it
/// points to, so a stow-managed `config.toml` stays in its dotfiles repo. An
/// existing file's permissions carry over to the new one. A failure removes
/// the temp file, leaves the target as it was, and names `path`.
///
/// No fsync: this survives a killed process, not a power loss.
pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let named = |e: io::Error| io::Error::new(e.kind(), format!("write {}: {e}", path.display()));
    let target = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::canonicalize(path).map_err(named)?
    } else {
        path.to_path_buf()
    };
    let tmp = temp_path(&target).map_err(named)?;
    let replaced = fs::write(&tmp, bytes)
        .and_then(|()| match fs::metadata(&target) {
            Ok(m) => fs::set_permissions(&tmp, m.permissions()),
            Err(_) => Ok(()),
        })
        .and_then(|()| fs::rename(&tmp, &target));
    replaced.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        named(e)
    })
}

fn temp_path(target: &Path) -> io::Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut tmp = OsString::from(".");
    tmp.push(name);
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    tmp.push(format!(".{}-{seq}.tmp", std::process::id()));
    Ok(target.with_file_name(tmp))
}

/// The file name a temp file of [`write`] was replacing: `nfl-scoreboard` for
/// `.nfl-scoreboard.4242-0.tmp`. None for any other name.
pub fn leftover_target(name: &str) -> Option<&str> {
    let (target, tag) = name
        .strip_prefix('.')?
        .strip_suffix(".tmp")?
        .rsplit_once('.')?;
    let (pid, seq) = tag.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (digits(pid) && digits(seq) && !target.is_empty()).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gd-atomic-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every name in `dir`, sorted: a temp file left behind shows up here.
    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn replaces_the_file_and_leaves_no_temp_file() {
        let dir = tmp("replace");
        let path = dir.join("config.toml");
        write(&path, "first").unwrap();
        write(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(names(&dir), ["config.toml"]);
        fs::remove_dir_all(&dir).ok();
    }

    /// The prune's tests name leftovers by hand; this ties that form to the
    /// one `write` actually makes.
    #[test]
    fn leftover_target_reads_the_temp_name_write_makes() {
        let tmp = temp_path(Path::new("cache/nfl-scoreboard.etag")).unwrap();
        let name = tmp.file_name().unwrap().to_str().unwrap();
        let pid = std::process::id();
        assert!(
            name.starts_with(&format!(".nfl-scoreboard.etag.{pid}-")),
            "{name}"
        );
        assert_eq!(leftover_target(name), Some("nfl-scoreboard.etag"));
    }

    /// A stow-managed `config.toml` is a symlink into a dotfiles repo; the
    /// save must land in the repo, not replace the link with a plain file.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_target_stays_a_symlink() {
        let dir = tmp("link");
        let real = dir.join("dotfiles");
        let conf = dir.join("conf");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(&conf).unwrap();
        fs::write(real.join("config.toml"), "old").unwrap();
        let link = conf.join("config.toml");
        std::os::unix::fs::symlink(real.join("config.toml"), &link).unwrap();
        write(&link, "new").unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(real.join("config.toml")).unwrap(), "new");
        assert_eq!(names(&real), ["config.toml"]);
        assert_eq!(names(&conf), ["config.toml"]);
        fs::remove_dir_all(&dir).ok();
    }

    /// A user's `chmod 600 config.toml` survives a save.
    #[cfg(unix)]
    #[test]
    fn existing_permissions_survive() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("mode");
        let path = dir.join("config.toml");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write(&path, "new").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode {mode:o}");
        fs::remove_dir_all(&dir).ok();
    }

    /// A directory nobody may write to: the temp file cannot be created, the
    /// target keeps its bytes, and the error names the path.
    #[cfg(unix)]
    #[test]
    fn a_write_that_cannot_complete_leaves_the_target_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("ro");
        let path = dir.join("pins.json");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        // Root (a packager's container) ignores the read-only bit, so the
        // write this test needs to fail would succeed. Probe it first.
        if fs::write(dir.join("probe"), "").is_ok() {
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
            fs::remove_dir_all(&dir).ok();
            eprintln!("skipped: a 0o555 directory is writable for this user (root?), so no write can fail");
            return;
        }
        let got = write(&path, "new");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let err = got.unwrap_err().to_string();
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(names(&dir), ["pins.json"]);
        fs::remove_dir_all(&dir).ok();
    }

    /// The temp file is written but the rename fails (the target is a
    /// directory): the temp file is removed, not left beside the target.
    #[test]
    fn a_failed_rename_removes_the_temp_file() {
        let dir = tmp("rename");
        let path = dir.join("pins.json");
        fs::create_dir_all(path.join("inside")).unwrap();
        let err = write(&path, "new").unwrap_err().to_string();
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert_eq!(names(&dir), ["pins.json"]);
        fs::remove_dir_all(&dir).ok();
    }
}
