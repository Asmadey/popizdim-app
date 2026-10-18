//! Portable mode and app data directories.
//!
//! When a file named `portable` exists next to the executable, all user data
//! (settings, models, recordings, database, logs) is stored in a `Data/`
//! directory alongside the executable instead of `%APPDATA%`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::Manager;

static PORTABLE_DATA_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static PREVIOUS_HF_HOME: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Detect portable mode by looking for a `portable` marker file next to the exe.
/// Must be called once at startup before Tauri initializes.
pub fn init() {
    let previous_hf_home = std::env::var_os("HF_HOME").map(PathBuf::from);
    let _ = PREVIOUS_HF_HOME.set(previous_hf_home);

    PORTABLE_DATA_DIR.get_or_init(|| {
        let exe_path = std::env::current_exe().ok()?;
        let exe_dir = exe_path.parent()?;

        let marker_path = exe_dir.join("portable");
        let data_dir = exe_dir.join("Data");

        let is_portable = if is_valid_portable_marker(&marker_path) {
            true
        } else if marker_path.exists() && data_dir.exists() {
            // Migration: v0.8.0 created an empty marker file. If we find an
            // empty/invalid marker alongside an existing Data/ dir, this is a
            // real portable install — upgrade the marker in place.
            eprintln!("[portable] upgrading legacy empty marker to magic string");
            let _ = std::fs::write(&marker_path, "Handy Portable Mode");
            true
        } else {
            false
        };

        if is_portable {
            if !data_dir.exists() {
                std::fs::create_dir_all(&data_dir).ok()?;
            }
            let hf_home = hugging_face_home(&data_dir);
            std::env::set_var("HF_HOME", &hf_home);
            eprintln!("[portable] data dir: {}", data_dir.display());
            eprintln!("[portable] Hugging Face home: {}", hf_home.display());
            Some(data_dir)
        } else {
            None
        }
    });
}

/// Return the Hugging Face home configured before portable mode redirected it.
///
/// Portable releases before v0.9.6 downloaded models to this location (or to
/// hf-hub's default cache when it is `None`). Keeping it lets the model manager
/// recognize those downloads after an upgrade without copying multi-gigabyte
/// files or modifying a cache shared with other applications.
pub fn previous_hf_home() -> Option<&'static PathBuf> {
    PREVIOUS_HF_HOME.get().and_then(|path| path.as_ref())
}

/// Keep hf-hub downloads inside the portable data directory. hf-hub appends
/// its own `hub` component to `HF_HOME` for model snapshots and blobs.
fn hugging_face_home(data_dir: &Path) -> PathBuf {
    data_dir.join("huggingface")
}

/// Returns `true` if running in portable mode.
pub fn is_portable() -> bool {
    PORTABLE_DATA_DIR.get().and_then(|v| v.as_ref()).is_some()
}

/// Get the portable data dir (if active). Does not require an AppHandle.
/// Returns `None` when not in portable mode.
pub fn data_dir() -> Option<&'static PathBuf> {
    PORTABLE_DATA_DIR.get().and_then(|v| v.as_ref())
}

/// Portable-aware replacement for `app.path().app_data_dir()`.
pub fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, tauri::Error> {
    if let Some(dir) = data_dir() {
        Ok(dir.clone())
    } else {
        app.path().app_data_dir()
    }
}

/// Bundle identifiers this app shipped under before the current one. Their
/// data directories sit next to the current one (`~/Library/Application
/// Support/<identifier>` on macOS).
const LEGACY_IDENTIFIERS: &[&str] = &["com.maidictate.desktop"];

/// Copies settings, history and recordings written under a previous bundle
/// identifier into the current data directory, once: only while the current
/// directory has no settings store yet (a plugin may have created the
/// directory itself already). The old directory is left untouched so an older
/// install keeps working. Must run before anything opens the settings store
/// (the store plugin caches what it read first and writes it back), i.e.
/// right after `Builder::build`, not in `setup`.
pub fn migrate_legacy_app_data(app: &tauri::AppHandle) {
    if data_dir().is_some() {
        return;
    }
    let Ok(current) = app.path().app_data_dir() else {
        return;
    };
    let Some(legacy) = legacy_data_dir(&current, LEGACY_IDENTIFIERS) else {
        return;
    };
    let result = if current.exists() {
        copy_tree(&legacy, &current, false)
    } else {
        copy_dir_atomically(&legacy, &current)
    };
    match result {
        Ok(()) => log::info!(
            "Copied app data from {} to {}",
            legacy.display(),
            current.display()
        ),
        Err(e) => log::error!(
            "Failed to copy app data from {} to {}: {}",
            legacy.display(),
            current.display(),
            e
        ),
    }
}

/// The first legacy data directory holding a settings store, while `current`
/// has none.
fn legacy_data_dir(current: &Path, identifiers: &[&str]) -> Option<PathBuf> {
    let store = crate::settings::SETTINGS_STORE_PATH;
    if current.join(store).exists() {
        return None;
    }
    let parent = current.parent()?;
    identifiers
        .iter()
        .map(|id| parent.join(id))
        .find(|dir| dir.join(store).is_file())
}

/// Copies `from` to `to` through a sibling staging directory and a rename, so
/// an interrupted copy never leaves a half-filled `to` that would block the
/// next attempt.
fn copy_dir_atomically(from: &Path, to: &Path) -> std::io::Result<()> {
    let name = to
        .file_name()
        .ok_or_else(|| std::io::Error::other("target has no file name"))?;
    let staging = to.with_file_name(format!("{}.migrating", name.to_string_lossy()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    copy_tree(from, &staging, true)?;
    std::fs::rename(&staging, to)
}

/// Recursive copy of directories and regular files (symlinks are skipped).
/// With `overwrite` false, files that already exist in `to` are kept.
fn copy_tree(from: &Path, to: &Path, overwrite: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, overwrite)?;
        } else if kind.is_file() && (overwrite || !target.exists()) {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Portable-aware replacement for `app.path().app_log_dir()`.
pub fn app_log_dir(app: &tauri::AppHandle) -> Result<PathBuf, tauri::Error> {
    if let Some(dir) = data_dir() {
        Ok(dir.join("logs"))
    } else {
        app.path().app_log_dir()
    }
}

/// Resolve a relative path against the app data directory (portable-aware).
/// Replaces `app.path().resolve(path, BaseDirectory::AppData)`.
pub fn resolve_app_data(app: &tauri::AppHandle, relative: &str) -> Result<PathBuf, tauri::Error> {
    Ok(app_data_dir(app)?.join(relative))
}

/// Get the path to use with `tauri-plugin-store`.
/// Returns an absolute path in portable mode (so the store plugin writes to
/// the portable Data dir) or the original relative path otherwise.
pub fn store_path(relative: &str) -> PathBuf {
    if let Some(dir) = data_dir() {
        dir.join(relative)
    } else {
        PathBuf::from(relative)
    }
}

/// Check if a marker file path contains the portable magic string.
/// Extracted for testability.
fn is_valid_portable_marker(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| s.trim().starts_with("Handy Portable Mode"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn legacy_data_is_copied_once_and_left_in_place() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("com.old.app");
        std::fs::create_dir_all(legacy.join("recordings")).unwrap();
        std::fs::write(legacy.join("settings_store.json"), "{}").unwrap();
        std::fs::write(legacy.join("recordings/a.wav"), "RIFF").unwrap();
        let current = root.path().join("com.new.app");

        let found = legacy_data_dir(&current, &["com.missing.app", "com.old.app"]).unwrap();
        assert_eq!(found, legacy);
        copy_dir_atomically(&found, &current).unwrap();

        assert_eq!(
            std::fs::read_to_string(current.join("recordings/a.wav")).unwrap(),
            "RIFF"
        );
        assert!(current.join("settings_store.json").is_file());
        assert!(
            legacy.join("settings_store.json").is_file(),
            "old data kept"
        );
        assert!(!root.path().join("com.new.app.migrating").exists());
        // Once the current directory exists, nothing is migrated again.
        assert!(legacy_data_dir(&current, &["com.old.app"]).is_none());
    }

    #[test]
    fn leftover_staging_directory_is_replaced() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("com.old.app");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("settings_store.json"), "{}").unwrap();
        std::fs::write(legacy.join("history.db"), "db").unwrap();
        let stale = root.path().join("com.new.app.migrating");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("partial"), "x").unwrap();

        let current = root.path().join("com.new.app");
        copy_dir_atomically(&legacy, &current).unwrap();
        assert!(current.join("history.db").is_file());
        assert!(!current.join("partial").exists());
    }

    #[test]
    fn existing_current_directory_without_settings_is_filled_in() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("com.old.app");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("settings_store.json"), "old").unwrap();
        std::fs::write(legacy.join("history.db"), "old").unwrap();
        // A plugin already created the new directory and one file in it.
        let current = root.path().join("com.new.app");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("history.db"), "new").unwrap();

        let found = legacy_data_dir(&current, &["com.old.app"]).unwrap();
        copy_tree(&found, &current, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(current.join("settings_store.json")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(current.join("history.db")).unwrap(),
            "new"
        );
        assert!(legacy_data_dir(&current, &["com.old.app"]).is_none());
    }

    #[test]
    fn legacy_directory_without_settings_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("com.old.app")).unwrap();
        assert!(legacy_data_dir(&root.path().join("com.new.app"), &["com.old.app"]).is_none());
    }

    #[test]
    fn no_legacy_directory_means_no_migration() {
        let root = tempfile::tempdir().unwrap();
        assert!(legacy_data_dir(&root.path().join("com.new.app"), &["com.old.app"]).is_none());
    }

    #[test]
    fn test_valid_magic_string_enables_portable() {
        let dir = std::env::temp_dir().join("handy_test_valid");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        let mut f = std::fs::File::create(&marker).unwrap();
        write!(f, "Handy Portable Mode").unwrap();
        assert!(is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_empty_file_does_not_enable_portable() {
        let dir = std::env::temp_dir().join("handy_test_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        std::fs::File::create(&marker).unwrap();
        assert!(!is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_wrong_content_does_not_enable_portable() {
        let dir = std::env::temp_dir().join("handy_test_wrong");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        let mut f = std::fs::File::create(&marker).unwrap();
        write!(f, "some other content").unwrap();
        assert!(!is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_missing_file_does_not_enable_portable() {
        let path = std::path::Path::new("/nonexistent/portable");
        assert!(!is_valid_portable_marker(path));
    }

    #[test]
    fn test_legacy_empty_marker_without_data_dir_does_not_enable_portable() {
        // Empty marker alone (scoop scenario) — no Data/ dir → not portable
        let dir = std::env::temp_dir().join("handy_test_legacy_no_data");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        std::fs::File::create(&marker).unwrap();
        assert!(!is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_magic_string_with_whitespace_enables_portable() {
        let dir = std::env::temp_dir().join("handy_test_ws");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        let mut f = std::fs::File::create(&marker).unwrap();
        write!(f, "  Handy Portable Mode\n").unwrap();
        assert!(is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_hugging_face_home_is_inside_portable_data() {
        let data_dir = Path::new("portable-root").join("Data");

        assert_eq!(hugging_face_home(&data_dir), data_dir.join("huggingface"));
    }
}
