//! The world list's worlds (m5-plan Decision 17, Step 16).
//!
//! **A world is a folder,** `<home>/worlds/<id>/`, and while it's played it's the embedded
//! server's working directory: its `world/`, its `data/ops.json` and the rest are Pumpkin's.
//! Beside them, `limonite-world.toml` is the client's own: the name the list shows, the
//! gamemode and seed the world was made with, and when it was made and last played.
//!
//! **A click never destroys a world** (worth teaching). Delete moves the folder into
//! `deleted/`, and Copy World writes a new folder into `backups/`; neither changes the world
//! itself. Moving a folder within one disk is a rename, a single step for the file system, so
//! a world is either still in `worlds/` or already in `deleted/`, never half in each.
//! Emptying `deleted/` stays the player's job while the client is young.
//!
//! **The folder's name is the world's id:** the name made safe for every file system, and
//! numbered if it's taken, so two worlds can share a name.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use pumpkin_config::PumpkinConfig;
use pumpkin_util::{GameMode, world_seed::Seed};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};
use tracing::warn;

use crate::{
    embedded,
    home::{Home, save_atomically},
    settings::Settings,
};

/// The client's own file in a world's folder.
pub const META: &str = "limonite-world.toml";
/// The longest name a world can have, in characters, so it fits a row of the list.
pub const MAX_NAME: usize = 32;
/// The names Windows keeps for devices, with or without an extension after them.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
/// What no file system's names may hold, and Windows' least of all.
const UNSAFE: &str = r#"<>:"/\|?*"#;

/// What `limonite-world.toml` holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldMeta {
    /// The name the list shows.
    pub name: String,
    /// The gamemode it was made in: a new player's, as the host is on its first visit.
    #[serde(with = "gamemode_name")]
    pub gamemode: GameMode,
    /// The seed it was made with. A world found without this file has none: its own is
    /// in `world/level.dat`, and the server reads it from there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// When it was made, and last played, in seconds since 1970 began (UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<i64>,
}

/// A gamemode as the settings file writes it: `survival`, `creative` and so on.
mod gamemode_name {
    use pumpkin_util::GameMode;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(mode: &GameMode, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(mode.name())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<GameMode, D::Error> {
        let name = String::deserialize(deserializer)?;
        name.parse().map_err(|_| D::Error::custom(format!("`{name}` isn't a gamemode")))
    }
}

/// A world in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldEntry {
    /// Its folder's name.
    pub id: String,
    pub dir: PathBuf,
    pub meta: WorldMeta,
}

impl WorldEntry {
    /// When it was last played, or made if it never has been.
    #[must_use]
    pub fn when(&self) -> Option<i64> {
        self.meta.last_played.or(self.meta.created)
    }

    /// The embedded server's configuration for this world: [`embedded::config`]'s, with the
    /// gamemode and seed the world was made with. Each counts only the first time: the seed
    /// while the world has yet to be made, and the gamemode for a player it hasn't seen.
    #[must_use]
    pub fn config(&self, settings: &Settings) -> PumpkinConfig {
        let mut config = embedded::config(settings);
        config.basic.default_gamemode = self.meta.gamemode;
        if let Some(seed) = self.meta.seed {
            config.basic.seed = Seed(seed as u64);
        }
        config
    }
}

/// Whether `name` can be a world's: 1 to [`MAX_NAME`] characters once the spaces round it
/// are gone, none of them control characters.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    let name = name.trim();
    (1..=MAX_NAME).contains(&name.chars().count()) && !name.chars().any(char::is_control)
}

/// A folder name for a world called `name`: what's unsafe in a file name becomes `_`, the
/// spaces and dots at either end go (Windows drops a trailing one, and a leading dot hides a
/// folder elsewhere), and a device name Windows keeps gets a `_` in front. `world` if
/// nothing is left. If `taken` holds it, in lower case, it's numbered from ` (2)` on, since
/// Windows' folder names ignore case.
#[must_use]
pub fn folder_id(name: &str, taken: &HashSet<String>) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_control() || UNSAFE.contains(c) { '_' } else { c })
        .collect();
    let safe = safe.trim().trim_matches('.').trim();
    let mut base = if safe.is_empty() { "world".to_string() } else { safe.to_string() };
    let stem = base.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        base.insert(0, '_');
    }
    let mut id = base.clone();
    let mut number = 2;
    while taken.contains(&id.to_lowercase()) {
        id = format!("{base} ({number})");
        number += 1;
    }
    id
}

/// Seconds since 1970 began, now.
#[must_use]
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}

/// A moment in the player's own time zone, or in UTC where the system won't say which
/// that is.
fn local(seconds: i64) -> OffsetDateTime {
    let at = OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    at.to_offset(UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC))
}

/// A moment as the list shows it: `2026-11-16 14:03`. Numbers only, so no month's name has
/// to be written in English (PLAN.md, UI: no strings in the code).
#[must_use]
pub fn shown(seconds: i64) -> String {
    let at = local(seconds);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute()
    )
}

/// A moment as a folder name ends: `2026-11-16_14-03-12`, which sorts in time order.
fn stamp(seconds: i64) -> String {
    let at = local(seconds);
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    )
}

/// The home's worlds, and the folders beside them.
pub struct Worlds {
    worlds: PathBuf,
    backups: PathBuf,
    deleted: PathBuf,
}

impl Worlds {
    /// The home's worlds, creating the three folders if they're missing.
    ///
    /// # Errors
    /// If a folder can't be created.
    pub fn open(home: &Home) -> io::Result<Self> {
        let worlds = Self {
            worlds: home.worlds(),
            backups: home.backups(),
            deleted: home.deleted(),
        };
        for dir in [&worlds.worlds, &worlds.backups, &worlds.deleted] {
            fs::create_dir_all(dir)?;
        }
        Ok(worlds)
    }

    /// Every world, the most recently played first. A folder without the client's file, such
    /// as a world copied in by hand, is listed under its folder's name; one whose file can't
    /// be read is too, with a warning, and the file is written afresh when it's next played.
    #[must_use]
    pub fn list(&self) -> Vec<WorldEntry> {
        let mut found = Vec::new();
        let entries = match fs::read_dir(&self.worlds) {
            Ok(entries) => entries,
            Err(err) => {
                warn!("Can't list the worlds in {}: {err}", self.worlds.display());
                return found;
            }
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let dir = entry.path();
            let meta = read_meta(&dir).unwrap_or_else(|| WorldMeta {
                name: id.clone(),
                gamemode: GameMode::Survival,
                seed: None,
                created: None,
                last_played: None,
            });
            found.push(WorldEntry { id, dir, meta });
        }
        // Newest first, and those never dated last; then by name, so the order holds still.
        found.sort_by(|a, b| b.when().cmp(&a.when()).then_with(|| a.meta.name.cmp(&b.meta.name)));
        found
    }

    /// Makes a world's folder and its file. The seed is read as Java and Pumpkin read one: a
    /// number as it is, other text by its hash, and nothing as a random seed. Its server
    /// makes the world itself on its first start.
    ///
    /// # Errors
    /// If the folder or the file can't be written.
    pub fn create(&self, name: &str, gamemode: GameMode, seed: &str, now: i64) -> io::Result<WorldEntry> {
        let taken: HashSet<String> = fs::read_dir(&self.worlds)?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
            .collect();
        let id = folder_id(name, &taken);
        let dir = self.worlds.join(&id);
        fs::create_dir(&dir)?;
        let meta = WorldMeta {
            name: name.trim().to_string(),
            gamemode,
            seed: Some(Seed::from(seed).0 as i64),
            created: Some(now),
            last_played: None,
        };
        write_meta(&dir, &meta)?;
        Ok(WorldEntry { id, dir, meta })
    }

    /// Notes that `entry` is being played now.
    ///
    /// # Errors
    /// If its file can't be written.
    pub fn played(&self, entry: &mut WorldEntry, now: i64) -> io::Result<()> {
        entry.meta.last_played = Some(now);
        write_meta(&entry.dir, &entry.meta)
    }

    /// Copies a world into `backups/`, as `<id>-<when>`. Only while it isn't running, which
    /// the list makes sure of: it shows only while no world is open. Says where the copy is.
    ///
    /// # Errors
    /// If it can't be copied. What was copied by then is removed again.
    pub fn back_up(&self, entry: &WorldEntry, now: i64) -> io::Result<PathBuf> {
        let target = free_name(&self.backups, &format!("{}-{}", entry.id, stamp(now)));
        if let Err(err) = copy_folder(&entry.dir, &target) {
            let _ = fs::remove_dir_all(&target);
            return Err(err);
        }
        Ok(target)
    }

    /// Moves a world into `deleted/`, as `<id>-<when>`. Says where it went.
    ///
    /// # Errors
    /// If it can't be moved, as when a program has a file in it open.
    pub fn delete(&self, entry: &WorldEntry, now: i64) -> io::Result<PathBuf> {
        let target = free_name(&self.deleted, &format!("{}-{}", entry.id, stamp(now)));
        fs::rename(&entry.dir, &target)?;
        Ok(target)
    }
}

/// `name` in `dir`, numbered from ` (2)` on if something already has it.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(name);
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("{name} ({number})"));
        number += 1;
    }
    path
}

/// The client's file in `dir`, or `None` if it's missing or can't be read, which is logged.
fn read_meta(dir: &Path) -> Option<WorldMeta> {
    let path = dir.join(META);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            warn!("Can't read {}: {err}", path.display());
            return None;
        }
    };
    match toml::from_str(&text) {
        Ok(meta) => Some(meta),
        Err(err) => {
            warn!("{} isn't what the client writes, so the world is listed by its folder: {err}", path.display());
            None
        }
    }
}

/// Writes the client's file in `dir`, so that a crash can't leave half of it.
fn write_meta(dir: &Path, meta: &WorldMeta) -> io::Result<()> {
    let text = toml::to_string(meta).map_err(io::Error::other)?;
    save_atomically(&dir.join(META), text.as_bytes())
}

/// Copies the folder `from` to `to`, which mustn't exist yet, with everything in it.
fn copy_folder(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn taken(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_lowercase()).collect()
    }

    fn worlds(dir: &TempDir) -> Worlds {
        Worlds::open(&Home::open(dir.path().to_path_buf()).unwrap()).unwrap()
    }

    #[test]
    fn a_plain_name_is_its_own_folder() {
        assert_eq!(folder_id("My World", &taken(&[])), "My World");
        assert_eq!(folder_id("Ünïcode wörld", &taken(&[])), "Ünïcode wörld");
    }

    #[test]
    fn what_a_file_system_refuses_is_replaced_or_trimmed() {
        assert_eq!(folder_id(r#"a<b>c:d"e/f\g|h?i*j"#, &taken(&[])), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(folder_id("tab\there", &taken(&[])), "tab_here");
        assert_eq!(folder_id("  .hidden. ", &taken(&[])), "hidden");
        assert_eq!(folder_id("ends with dots...", &taken(&[])), "ends with dots");
        assert_eq!(folder_id(" ... ", &taken(&[])), "world");
        assert_eq!(folder_id("", &taken(&[])), "world");
    }

    #[test]
    fn a_device_name_windows_keeps_is_not_a_folder() {
        assert_eq!(folder_id("con", &taken(&[])), "_con");
        assert_eq!(folder_id("LPT1.txt", &taken(&[])), "_LPT1.txt");
        assert_eq!(folder_id("Console", &taken(&[])), "Console", "only the whole name counts");
    }

    #[test]
    fn a_taken_id_is_numbered_whatever_its_case() {
        assert_eq!(folder_id("My World", &taken(&["my world"])), "My World (2)");
        assert_eq!(folder_id("My World", &taken(&["MY WORLD", "my world (2)"])), "My World (3)");
    }

    #[test]
    fn names_are_one_to_32_characters_without_control_characters() {
        assert!(is_valid_name("My World"));
        assert!(is_valid_name(&"w".repeat(MAX_NAME)));
        assert!(!is_valid_name(&"w".repeat(MAX_NAME + 1)));
        assert!(!is_valid_name("   "));
        assert!(!is_valid_name("a\nb"));
    }

    #[test]
    fn the_file_comes_back_as_it_was_written() {
        let dir = TempDir::new("worlds-meta");
        let meta = WorldMeta {
            name: "Round trip".to_string(),
            gamemode: GameMode::Creative,
            seed: Some(-42),
            created: Some(1_763_300_000),
            last_played: Some(1_763_300_500),
        };
        write_meta(dir.path(), &meta).unwrap();
        assert_eq!(read_meta(dir.path()), Some(meta));
        let text = fs::read_to_string(dir.path().join(META)).unwrap();
        assert!(text.contains("gamemode = \"creative\""), "{text}");
    }

    #[test]
    fn a_world_without_its_file_or_with_a_bad_one_is_listed_by_its_folder() {
        let dir = TempDir::new("worlds-found");
        dir.write("worlds/copied in/world/level.dat", b"");
        dir.write("worlds/broken/limonite-world.toml", b"name = 3\ngamemode = \"hardcore\"\n");
        dir.write("worlds/not a world.txt", b"");
        let list = worlds(&dir).list();
        let ids: Vec<&str> = list.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["broken", "copied in"], "folders only, by name when undated");
        assert_eq!(list[1].meta.name, "copied in");
        assert_eq!(list[1].meta.seed, None, "its own seed is in level.dat");
    }

    #[test]
    fn the_list_is_the_most_recently_played_first() {
        let dir = TempDir::new("worlds-order");
        let worlds = worlds(&dir);
        let mut old = worlds.create("Old", GameMode::Survival, "1", 100).unwrap();
        worlds.create("New", GameMode::Survival, "2", 300).unwrap();
        worlds.create("Never played", GameMode::Survival, "3", 200).unwrap();
        worlds.played(&mut old, 400).unwrap();
        let names: Vec<String> = worlds.list().into_iter().map(|entry| entry.meta.name).collect();
        assert_eq!(names, ["Old", "New", "Never played"]);
    }

    #[test]
    fn a_new_world_has_its_folder_its_file_and_its_seed() {
        let dir = TempDir::new("worlds-create");
        let worlds = worlds(&dir);
        let entry = worlds.create("  Seeded  ", GameMode::Creative, "12345", 7).unwrap();
        assert_eq!(entry.id, "Seeded");
        assert!(entry.dir.join(META).is_file());
        assert_eq!(entry.meta.name, "Seeded");
        assert_eq!(entry.meta.seed, Some(12345), "a number is the seed");
        assert_eq!(entry.meta.created, Some(7));

        let worded = worlds.create("Seeded", GameMode::Survival, "glacier", 8).unwrap();
        assert_eq!(worded.id, "Seeded (2)", "the same name again");
        assert_eq!(worded.meta.seed, Some(Seed::from("glacier").0 as i64), "text is hashed, as Java hashes it");
        let random = worlds.create("Random", GameMode::Survival, "", 9).unwrap();
        assert!(random.meta.seed.is_some(), "an empty seed is a random one, kept");
        assert_eq!(read_meta(&random.dir), Some(random.meta));
    }

    #[test]
    fn delete_moves_the_folder_and_keeps_everything_in_it() {
        let dir = TempDir::new("worlds-delete");
        let worlds = worlds(&dir);
        let entry = worlds.create("Doomed", GameMode::Survival, "1", 5).unwrap();
        dir.write("worlds/Doomed/world/region/r.0.0.mca", b"chunks");
        let moved = worlds.delete(&entry, 1_763_300_000).unwrap();
        assert!(!entry.dir.exists());
        assert!(moved.starts_with(dir.path().join("deleted")));
        assert!(moved.file_name().unwrap().to_string_lossy().starts_with("Doomed-20"));
        assert_eq!(fs::read(moved.join("world/region/r.0.0.mca")).unwrap(), b"chunks");
        assert!(worlds.list().is_empty());
    }

    #[test]
    fn copy_copies_everything_and_leaves_the_world_as_it_was() {
        let dir = TempDir::new("worlds-copy");
        let worlds = worlds(&dir);
        let entry = worlds.create("Kept", GameMode::Survival, "1", 5).unwrap();
        dir.write("worlds/Kept/world/level.dat", b"level");
        dir.write("worlds/Kept/data/ops.json", b"[]");
        let first = worlds.back_up(&entry, 1_763_300_000).unwrap();
        let second = worlds.back_up(&entry, 1_763_300_000).unwrap();
        assert_ne!(first, second, "two copies in the same second get two folders");
        for copy in [&first, &second] {
            assert!(copy.starts_with(dir.path().join("backups")));
            assert_eq!(fs::read(copy.join("world/level.dat")).unwrap(), b"level");
            assert_eq!(fs::read(copy.join("data/ops.json")).unwrap(), b"[]");
            assert_eq!(read_meta(copy), Some(entry.meta.clone()));
        }
        assert_eq!(fs::read(entry.dir.join("world/level.dat")).unwrap(), b"level");
        assert_eq!(worlds.list(), [entry], "a copy isn't a world in the list");
    }

    #[test]
    fn a_moment_is_shown_by_its_numbers() {
        let shown = shown(1_763_300_000);
        assert_eq!(shown.len(), "2026-11-16 14:03".len(), "{shown}");
        assert!(shown.starts_with("2025-11-1"), "{shown}");
        assert_eq!(stamp(0).len(), "1970-01-01_00-00-00".len());
    }
}
