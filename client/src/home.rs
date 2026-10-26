//! The client's home folder (m5-plan Decision 16): the player's settings and
//! this install's identity. M5's world list adds `worlds/` here, with
//! `backups/` and `deleted/` beside it.
//!
//! `--home <dir>` picks it, `run/home` by default.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// The home folder, which exists once [`Home::open`] returns.
pub struct Home {
    root: PathBuf,
}

impl Home {
    /// Creates the folder if it's missing.
    ///
    /// # Errors
    /// If the folder can't be created.
    pub fn open(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `settings.toml`: what the player chose (`settings.rs`).
    #[must_use]
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.toml")
    }

    /// `identity.toml`: who this install logs in as (`identity.rs`).
    #[must_use]
    pub fn identity(&self) -> PathBuf {
        self.root.join("identity.toml")
    }
}

/// Replaces `path` with `contents` so that a crash can't leave half a file
/// (worth teaching).
///
/// Writing a file in place truncates it first, so a crash, a full disk or a
/// power cut part-way leaves something that is neither the old file nor the
/// new one. Instead this writes a temporary file beside it, flushes it to the
/// disk, and renames it over the old one. A rename within one folder is a
/// single step for the file system: afterwards the path names either the
/// whole old file or the whole new one, never a mix. On Windows `fs::rename`
/// replaces an existing file, as it does elsewhere.
///
/// # Errors
/// If the temporary file can't be written or renamed.
pub fn save_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file = File::create(&temporary)?;
    file.write_all(contents)?;
    // On the disk before the rename, so the rename never exposes a file
    // whose contents are still only in memory.
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    #[test]
    fn a_save_replaces_the_whole_file_and_leaves_nothing_beside_it() {
        let dir = TempDir::new("home-save");
        let path = dir.path().join("settings.toml");
        save_atomically(&path, b"a long first version\n").unwrap();
        save_atomically(&path, b"short\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"short\n");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["settings.toml"]);
    }

    #[test]
    fn opening_creates_the_folder() {
        let dir = TempDir::new("home-open");
        let home = Home::open(dir.path().join("a").join("home")).unwrap();
        assert!(home.root().is_dir());
        assert_eq!(home.settings().file_name().unwrap(), "settings.toml");
        assert_eq!(home.identity().file_name().unwrap(), "identity.toml");
    }
}
