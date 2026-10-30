//! Finding the user's own Java Edition jar, and reading files from it (m5-plan
//! Step 7c).
//!
//! **Why a Java jar, in a Bedrock client** (worth teaching). Bedrock's engine
//! draws vanilla blocks from its own code, so its install has no shapes for them
//! (m4-plan Finding 8). Java's jar keeps them as data: every block's model, with
//! the boxes and texture rectangles the game draws.
//!
//! **It's read, and never kept,** as the Bedrock install is. Mojang's terms don't
//! let anyone hand its files on, and a tighter encoding of them would still be a
//! copy, so nothing from a jar goes in the repo or the binary. The copy has to be
//! the user's own, and has to come from Mojang: the launcher's folder, or a path
//! the user names. With no jar, the hand-made models in `blocks/models.rs` draw.

use std::{
    fmt,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use pumpkin_world::CURRENT_MC_VERSION;
use tracing::warn;
use zip::ZipArchive;

/// Every jar has this, so a file without it isn't one.
const A_BLOCK_STATE_FILE: &str = "assets/minecraft/blockstates/stone.json";
/// Far more than any file wanted from a jar: it only stops a damaged one from
/// claiming to be huge.
const MAX_FILE_LEN: u64 = 16 * 1024 * 1024;

/// How the jar was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundBy {
    /// `--java-jar`.
    Flag,
    /// The launcher's folder.
    Launcher,
    /// Where Blockbench's Asset Browser keeps what it fetched from Mojang.
    AssetBrowser,
}

impl fmt::Display for FoundBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag => write!(f, "from --java-jar"),
            Self::Launcher => write!(f, "the launcher's copy"),
            Self::AssetBrowser => write!(f, "the copy Blockbench's Asset Browser fetched"),
        }
    }
}

/// An open jar.
#[derive(Debug)]
pub struct Jar {
    pub path: PathBuf,
    pub found_by: FoundBy,
    /// The version it says it is, if it says.
    pub version: Option<String>,
    archive: ZipArchive<BufReader<File>>,
}

impl Jar {
    /// Finds the jar: `--java-jar` if given, otherwise the version Pumpkin plays,
    /// from where it's usually kept. `None` if there isn't one, which is fine:
    /// the hand-made models draw.
    ///
    /// # Errors
    /// If `--java-jar` can't be read as a jar.
    pub fn find(flag: Option<&Path>) -> Result<Option<Self>, String> {
        if let Some(path) = flag {
            return Self::open(path, FoundBy::Flag)
                .map(Some)
                .map_err(|err| format!("--java-jar {}: {err}", path.display()));
        }
        // %APPDATA% only exists on Windows. Elsewhere the jar has to be named,
        // as the Bedrock install does.
        let Some(roaming) = std::env::var_os("APPDATA") else {
            return Ok(None);
        };
        for (path, found_by) in usual_places(Path::new(&roaming), CURRENT_MC_VERSION) {
            if path.is_file() {
                match Self::open(&path, found_by) {
                    Ok(jar) => return Ok(Some(jar)),
                    Err(err) => warn!("Skipped a Java jar at {}: {err}", path.display()),
                }
            }
        }
        Ok(None)
    }

    /// Opens a jar.
    ///
    /// # Errors
    /// If the file can't be read, isn't a zip, or isn't a Minecraft jar.
    pub fn open(path: &Path, found_by: FoundBy) -> Result<Self, String> {
        let file = File::open(path).map_err(|err| err.to_string())?;
        let archive = ZipArchive::new(BufReader::new(file)).map_err(|err| format!("not a jar: {err}"))?;
        if archive.index_for_name(A_BLOCK_STATE_FILE).is_none() {
            return Err("not a Minecraft client jar: it has no block state files".to_string());
        }
        let mut jar = Self {
            path: path.to_path_buf(),
            found_by,
            version: None,
            archive,
        };
        jar.version = jar
            .read("version.json")
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|version| Some(version.get("id")?.as_str()?.to_string()));
        Ok(jar)
    }

    /// One file's bytes, or `None` if the jar hasn't got it or it's damaged.
    pub fn read(&mut self, entry: &str) -> Option<Vec<u8>> {
        let file = self.archive.by_name(entry).ok()?;
        let mut bytes = Vec::with_capacity(file.size().min(MAX_FILE_LEN) as usize);
        file.take(MAX_FILE_LEN).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

/// Where a version's jar is usually kept, most official first.
fn usual_places(roaming: &Path, version: &str) -> [(PathBuf, FoundBy); 2] {
    let file = format!("{version}.jar");
    [
        (
            roaming.join(".minecraft").join("versions").join(version).join(&file),
            FoundBy::Launcher,
        ),
        (
            roaming.join("Blockbench").join("minecraft_assets_cache").join(&file),
            FoundBy::AssetBrowser,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::test_util::TempDir;

    fn write_zip(path: &Path, files: &[(&str, &str)]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, content) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn reads_files_and_the_version_from_a_jar() {
        let dir = TempDir::new("jar_reads");
        let path = dir.path().join("client.jar");
        write_zip(
            &path,
            &[
                (A_BLOCK_STATE_FILE, "{}"),
                ("version.json", r#"{"id": "1.2.3"}"#),
                ("assets/minecraft/a.txt", "hello"),
            ],
        );
        let mut jar = Jar::open(&path, FoundBy::Flag).unwrap();
        assert_eq!(jar.version.as_deref(), Some("1.2.3"));
        assert_eq!(jar.read("assets/minecraft/a.txt").as_deref(), Some(&b"hello"[..]));
        assert_eq!(jar.read("assets/minecraft/b.txt"), None);
    }

    #[test]
    fn refuses_what_isnt_a_jar() {
        let dir = TempDir::new("jar_refuses");
        let not_zip = dir.path().join("not_zip.jar");
        std::fs::write(&not_zip, "hello").unwrap();
        assert!(Jar::open(&not_zip, FoundBy::Flag).unwrap_err().contains("not a jar"));
        let not_minecraft = dir.path().join("not_minecraft.jar");
        write_zip(&not_minecraft, &[("a.txt", "hello")]);
        assert!(
            Jar::open(&not_minecraft, FoundBy::Flag)
                .unwrap_err()
                .contains("not a Minecraft client jar")
        );
        // A named jar that's wrong is the user's to fix, so it's an error.
        assert!(Jar::find(Some(&not_zip)).is_err());
        assert!(Jar::find(Some(&dir.path().join("missing.jar"))).is_err());
    }

    #[test]
    fn the_launchers_copy_comes_first() {
        let [(first, by_first), (second, by_second)] = usual_places(Path::new("R"), "26.3");
        assert_eq!(first, Path::new("R/.minecraft/versions/26.3/26.3.jar"));
        assert_eq!(by_first, FoundBy::Launcher);
        assert_eq!(second, Path::new("R/Blockbench/minecraft_assets_cache/26.3.jar"));
        assert_eq!(by_second, FoundBy::AssetBrowser);
    }
}
