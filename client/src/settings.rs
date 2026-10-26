//! What the player chooses, kept in `settings.toml` in the home folder
//! (m5-plan Decision 16).
//!
//! **Forgiving to read, careful to write** (worth teaching). The file is the
//! player's to edit, so a slip in it mustn't stop the game. Each setting is
//! read on its own: one that's missing takes its default, and so does one
//! that's wrong, with a warning; a key the client doesn't know is dropped,
//! with a warning. If any of that happened, the whole file is written back,
//! so it always lists every setting with its comment. A file that isn't TOML
//! at all is first kept beside it as `settings.toml.broken`. Writing goes
//! through [`save_atomically`], so a crash can't leave half a file.
//!
//! The flags `--name`, `--radius` and `--gamemode` change [`Settings`] after
//! it's loaded, for one run: the file never hears of them ([`Overrides`]).

use std::{fmt::Write as _, fs, io, path::Path};

use pumpkin_util::GameMode;
use toml::{Table, Value};
use tracing::{info, warn};

use crate::{
    home::save_atomically,
    input::{Action, Button, Keymap},
    render::camera::FOV_Y,
};

/// The view distance asked for by default, in chunks.
pub const DEFAULT_RADIUS: i32 = 8;
/// The fewest chunks the view distance can be.
pub const MIN_RADIUS: i32 = 2;
/// The most: the embedded server's Bedrock view distance.
pub const MAX_RADIUS: i32 = 32;
/// The longest name, in characters: an Xbox gamertag's limit, and the one
/// Pumpkin applies to Java names.
const MAX_NAME: usize = 16;

/// The player's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The host's display name.
    pub name: String,
    /// The view distance to ask for, in chunks.
    pub radius: i32,
    /// The vertical field of view, in degrees.
    pub fov: f64,
    /// How fast the mouse turns the view, as a multiple of the default.
    pub mouse_sensitivity: f64,
    /// How big the UI draws (Step 8).
    pub ui_scale: UiScale,
    /// Smooth lighting and ambient occlusion (Step 14).
    pub smooth_lighting: bool,
    /// The gamemode the create-world screen offers first (Step 16), and the
    /// one a `--data-dir` world gives a host it hasn't seen before.
    ///
    /// Creative by default (m5-plan Decision 6): survival works, but without
    /// crafting or containers it can't get far until milestone 5b.
    ///
    /// A host the world has saved keeps its own mode: the server reads it
    /// from the player's data on join.
    pub gamemode: GameMode,
    /// Which buttons do what.
    pub keymap: Keymap,
}

/// How big the UI draws (m5-plan Decision 14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiScale {
    /// The largest whole scale at which a 320 × 240 screen fits the window.
    Auto,
    /// Each UI pixel is this many window pixels, 1 to 8.
    Fixed(u8),
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: "Limonite".to_string(),
            radius: DEFAULT_RADIUS,
            fov: f64::from(FOV_Y),
            mouse_sensitivity: 1.0,
            ui_scale: UiScale::Auto,
            smooth_lighting: true,
            gamemode: GameMode::Creative,
            keymap: Keymap::default(),
        }
    }
}

/// What the flags change for one run.
#[derive(Debug, Default)]
pub struct Overrides {
    pub name: Option<String>,
    pub radius: Option<i32>,
    pub gamemode: Option<GameMode>,
}

/// Whether `name` can be the host's: 1 to 16 characters, none of them
/// control characters.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    (1..=MAX_NAME).contains(&name.chars().count()) && !name.chars().any(char::is_control)
}

/// The file's opening comment.
const HEADER: &str = "\
# Limonite Client settings.
#
# Edit this while the client is closed. A setting that's missing or wrong gets
# its default back, and the client then rewrites this file, comments and all.";

/// The keymap's comment.
const KEYS_HELP: &str = "\
# What each button does. A button is a key's name (KeyW, Space, ShiftLeft,
# ControlLeft, Digit1, F1, Escape, Slash, ...), named by where the key sits on a
# US keyboard; or MouseLeft, MouseRight, MouseMiddle, MouseBack, MouseForward,
# WheelUp or WheelDown. A list binds several buttons, and [] binds none.";

impl Settings {
    /// Reads the settings at `path`, filling in and correcting what it must,
    /// and writes the file back if it had to (see the module's comment).
    ///
    /// It never fails: at worst the player gets the defaults, with a warning
    /// saying why.
    pub fn load(path: &Path) -> Self {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                info!("No settings yet: writing the defaults to {}", path.display());
                let settings = Self::default();
                settings.save_or_warn(path);
                return settings;
            }
            Err(err) => {
                warn!(
                    "Can't read {}: {err}. Using the default settings, and leaving the file alone",
                    path.display()
                );
                return Self::default();
            }
        };
        let table = String::from_utf8(bytes)
            .map_err(|err| err.to_string())
            .and_then(|text| text.parse::<Table>().map_err(|err| err.to_string()));
        let table = match table {
            Ok(table) => table,
            Err(err) => {
                let broken = path.with_extension("toml.broken");
                warn!(
                    "{} isn't valid TOML, so it's kept as {} and the defaults replace it. {err}",
                    path.display(),
                    broken.display()
                );
                let settings = Self::default();
                // Only replace a file we managed to keep.
                match fs::rename(path, &broken) {
                    Ok(()) => settings.save_or_warn(path),
                    Err(err) => warn!("Couldn't keep it ({err}), so it's left as it is"),
                }
                return settings;
            }
        };

        let (settings, notes) = Self::from_table(table);
        for key in &notes.filled {
            info!("Settings: `{key}` was missing, so it gets its default");
        }
        for problem in &notes.problems {
            warn!("Settings: {problem}");
        }
        if !notes.filled.is_empty() || !notes.problems.is_empty() {
            settings.save_or_warn(path);
        }
        settings
    }

    /// Writes the whole file, safely.
    ///
    /// # Errors
    /// If the file can't be written.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        save_atomically(path, self.to_toml().as_bytes())
    }

    fn save_or_warn(&self, path: &Path) {
        match self.save(path) {
            Ok(()) => info!("Wrote the settings to {}", path.display()),
            Err(err) => warn!("Couldn't write the settings to {}: {err}", path.display()),
        }
    }

    /// Applies the flags, for this run only: nothing is saved.
    pub fn apply(&mut self, overrides: &Overrides) {
        if let Some(name) = &overrides.name {
            self.name.clone_from(name);
        }
        if let Some(radius) = overrides.radius {
            self.radius = radius;
        }
        if let Some(gamemode) = overrides.gamemode {
            self.gamemode = gamemode;
        }
    }

    /// Reads each setting from the table on its own, noting what it had to
    /// put right.
    fn from_table(mut table: Table) -> (Self, Notes) {
        let defaults = Self::default();
        let mut read = Notes::default();
        let settings = Self {
            name: read.field(&mut table, "name", defaults.name, "a name of 1 to 16 characters", |value| {
                value.as_str().filter(|name| is_valid_name(name)).map(str::to_string)
            }),
            radius: read.field(&mut table, "radius", defaults.radius, "a whole number from 2 to 32", |value| {
                value
                    .as_integer()
                    .and_then(|radius| i32::try_from(radius).ok())
                    .filter(|radius| (MIN_RADIUS..=MAX_RADIUS).contains(radius))
            }),
            fov: read.field(&mut table, "fov", defaults.fov, "a number from 30 to 110", |value| {
                number(value).filter(|fov| (30.0..=110.0).contains(fov))
            }),
            mouse_sensitivity: read.field(
                &mut table,
                "mouse_sensitivity",
                defaults.mouse_sensitivity,
                "a number from 0.1 to 10",
                |value| number(value).filter(|speed| (0.1..=10.0).contains(speed)),
            ),
            ui_scale: read.field(
                &mut table,
                "ui_scale",
                defaults.ui_scale,
                "\"auto\" or a whole number from 1 to 8",
                |value| match value {
                    Value::String(auto) if auto == "auto" => Some(UiScale::Auto),
                    Value::Integer(scale) => u8::try_from(*scale)
                        .ok()
                        .filter(|scale| (1..=8).contains(scale))
                        .map(UiScale::Fixed),
                    _ => None,
                },
            ),
            smooth_lighting: read.field(
                &mut table,
                "smooth_lighting",
                defaults.smooth_lighting,
                "true or false",
                Value::as_bool,
            ),
            gamemode: read.field(
                &mut table,
                "gamemode",
                defaults.gamemode,
                "survival, creative, adventure or spectator",
                |value| value.as_str().and_then(|mode| mode.parse().ok()),
            ),
            keymap: match table.remove("keys") {
                Some(Value::Table(keys)) => read.keymap(keys),
                Some(other) => {
                    read.problems.push(format!("`keys = {other}` isn't a table; using the default keys"));
                    defaults.keymap
                }
                None => {
                    read.filled.push("keys".to_string());
                    defaults.keymap
                }
            },
        };
        for key in table.keys() {
            read.problems.push(format!("`{key}` isn't a setting; dropping it"));
        }
        (settings, read)
    }

    /// The whole file: every setting, each with a comment.
    fn to_toml(&self) -> String {
        let mut out = String::from(HEADER);
        let mut setting = |comment: &str, key: &str, value: Value| {
            let _ = write!(out, "\n\n# {comment}\n{key} = {value}");
        };
        setting(
            "Your name in the game: 1 to 16 characters.",
            "name",
            Value::String(self.name.clone()),
        );
        setting(
            "How far you see, in chunks: 2 to 32.",
            "radius",
            Value::Integer(self.radius.into()),
        );
        setting(
            "The field of view, up and down, in degrees: 30 to 110.",
            "fov",
            Value::Float(self.fov),
        );
        setting(
            "How fast the mouse turns the view, as a multiple of the default: 0.1 to 10.",
            "mouse_sensitivity",
            Value::Float(self.mouse_sensitivity),
        );
        setting(
            "How big the menus and the HUD draw: \"auto\" fits them to the window, or a\n# whole number \
             from 1 to 8 fixes it.",
            "ui_scale",
            match self.ui_scale {
                UiScale::Auto => Value::String("auto".to_string()),
                UiScale::Fixed(scale) => Value::Integer(scale.into()),
            },
        );
        setting(
            "Smooth lighting and ambient occlusion: true or false.",
            "smooth_lighting",
            Value::Boolean(self.smooth_lighting),
        );
        setting(
            "The gamemode a new world starts in: survival, creative, adventure or\n# spectator. A world \
             you've played keeps yours.",
            "gamemode",
            Value::String(self.gamemode.name().to_string()),
        );
        let _ = write!(out, "\n\n{KEYS_HELP}\n[keys]");
        for (action, name) in Action::ALL {
            let mut buttons: Vec<Value> = self
                .keymap
                .buttons(action)
                .filter_map(Button::name)
                .map(|button| Value::String(button.to_string()))
                .collect();
            let value = if buttons.len() == 1 {
                buttons.remove(0)
            } else {
                Value::Array(buttons)
            };
            let _ = write!(out, "\n{name} = {value}");
        }
        out.push('\n');
        out
    }
}

/// What reading the file had to put right.
#[derive(Debug, Default)]
struct Notes {
    /// Keys that were missing.
    filled: Vec<String>,
    /// Bad values and unknown keys, as sentences.
    problems: Vec<String>,
}

impl Notes {
    /// Takes `key` out of the table and reads it with `parse`, or notes why
    /// it gets `default`. `expected` says what a good value looks like.
    fn field<T>(
        &mut self,
        table: &mut Table,
        key: &str,
        default: T,
        expected: &str,
        parse: impl FnOnce(&Value) -> Option<T>,
    ) -> T {
        let Some(value) = table.remove(key) else {
            self.filled.push(key.to_string());
            return default;
        };
        parse(&value).unwrap_or_else(|| {
            self.problems
                .push(format!("`{key} = {value}` isn't {expected}; using the default"));
            default
        })
    }

    /// Reads the `[keys]` table. An action that's missing or has a bad
    /// button keeps its default buttons.
    fn keymap(&mut self, mut keys: Table) -> Keymap {
        let mut keymap = Keymap::default();
        for (action, name) in Action::ALL {
            let Some(value) = keys.remove(name) else {
                self.filled.push(format!("keys.{name}"));
                continue;
            };
            let names = match &value {
                Value::String(button) => Some(vec![button.as_str()]),
                Value::Array(buttons) => buttons.iter().map(Value::as_str).collect(),
                _ => None,
            };
            let buttons: Option<Vec<Button>> = names.and_then(|names| names.into_iter().map(Button::from_name).collect());
            match buttons {
                Some(buttons) => keymap.bind(action, buttons),
                None => self.problems.push(format!(
                    "`keys.{name} = {value}` isn't a button or a list of buttons; using the default"
                )),
            }
        }
        for name in keys.keys() {
            self.problems.push(format!("`keys.{name}` isn't an action; dropping it"));
        }
        keymap
    }
}

/// A TOML number as an `f64`: `fov = 70` is as good as `fov = 70.0`.
fn number(value: &Value) -> Option<f64> {
    match *value {
        Value::Float(number) => Some(number),
        Value::Integer(number) => Some(number as f64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use winit::{event::MouseButton, keyboard::KeyCode};

    use super::*;
    use crate::test_util::TempDir;

    fn read(text: &str) -> (Settings, Notes) {
        Settings::from_table(text.parse().unwrap())
    }

    /// Settings with every field changed from its default.
    fn changed() -> Settings {
        let mut keymap = Keymap::default();
        keymap.bind(Action::Jump, [Button::Key(KeyCode::KeyJ), Button::Mouse(MouseButton::Back)]);
        keymap.bind(Action::Drop, []);
        Settings {
            name: "Mr \"Q\" Name".to_string(),
            radius: 12,
            fov: 85.5,
            mouse_sensitivity: 0.75,
            ui_scale: UiScale::Fixed(3),
            smooth_lighting: false,
            gamemode: GameMode::Survival,
            keymap,
        }
    }

    #[test]
    fn settings_round_trip() {
        for settings in [Settings::default(), changed()] {
            let (read, notes) = read(&settings.to_toml());
            assert_eq!(read, settings);
            assert!(notes.filled.is_empty() && notes.problems.is_empty(), "{notes:?}");
        }
    }

    #[test]
    fn missing_settings_are_filled_in() {
        let (read, notes) = read("radius = 5\n[keys]\njump = \"KeyJ\"\n");
        let mut expected = Settings {
            radius: 5,
            ..Settings::default()
        };
        expected.keymap.bind(Action::Jump, [Button::Key(KeyCode::KeyJ)]);
        assert_eq!(read, expected);
        assert!(notes.problems.is_empty(), "{notes:?}");
        // Seven settings and 27 actions, less the two the file gives.
        assert_eq!(notes.filled.len(), 6 + 26, "{notes:?}");
        assert!(notes.filled.contains(&"name".to_string()));
        assert!(notes.filled.contains(&"keys.forward".to_string()));
        assert!(!notes.filled.contains(&"radius".to_string()));
        assert!(!notes.filled.contains(&"keys.jump".to_string()));
    }

    #[test]
    fn bad_values_and_unknown_keys_are_replaced() {
        let (read, notes) = read(
            "name = \"\"\nradius = 99\nfov = \"wide\"\nmouse_sensitivity = 0\nui_scale = 0\n\
             smooth_lighting = 1\ngamemode = \"hardcore\"\nvolume = 11\n\
             [keys]\nforward = \"Spacebar\"\nback = [\"KeyS\", 5]\nfly = \"KeyF\"\n",
        );
        assert_eq!(read, Settings::default());
        // The actions the `[keys]` table leaves out are filled in.
        assert_eq!(notes.filled.len(), Action::ALL.len() - 2, "{notes:?}");
        let problems = notes.problems.join("\n");
        for expected in [
            "`name = \"\"` isn't a name of 1 to 16 characters",
            "`radius = 99` isn't a whole number from 2 to 32",
            "`fov = \"wide\"` isn't a number from 30 to 110",
            "`mouse_sensitivity = 0` isn't a number from 0.1 to 10",
            "`ui_scale = 0` isn't \"auto\" or a whole number from 1 to 8",
            "`smooth_lighting = 1` isn't true or false",
            "`gamemode = \"hardcore\"` isn't survival, creative, adventure or spectator",
            "`volume` isn't a setting",
            "`keys.forward = \"Spacebar\"` isn't a button",
            "`keys.back = [\"KeyS\", 5]` isn't a button",
            "`keys.fly` isn't an action",
        ] {
            assert!(problems.contains(expected), "no `{expected}` in:\n{problems}");
        }
        assert_eq!(notes.problems.len(), 11, "{problems}");
    }

    #[test]
    fn loading_fills_in_the_file_and_leaves_a_good_one_alone() {
        let dir = TempDir::new("settings-load");
        let path = dir.path().join("settings.toml");

        // No file: the defaults, written down.
        assert_eq!(Settings::load(&path), Settings::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), Settings::default().to_toml());

        // A file with one setting gets the rest, and keeps that one.
        fs::write(&path, "radius = 12\n").unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.radius, 12);
        assert_eq!(fs::read_to_string(&path).unwrap(), settings.to_toml());

        // A complete, good file isn't rewritten, so the player's own
        // comments stay.
        let mine = format!("# mine\n{}", changed().to_toml());
        fs::write(&path, &mine).unwrap();
        assert_eq!(Settings::load(&path), changed());
        assert_eq!(fs::read_to_string(&path).unwrap(), mine);
    }

    #[test]
    fn a_file_that_isnt_toml_is_kept_beside_the_defaults() {
        let dir = TempDir::new("settings-broken");
        let path = dir.path().join("settings.toml");
        fs::write(&path, "radius = = 3\n").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert_eq!(
            fs::read_to_string(dir.path().join("settings.toml.broken")).unwrap(),
            "radius = = 3\n"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), Settings::default().to_toml());
    }

    #[test]
    fn flags_override_for_one_run() {
        let mut settings = Settings::default();
        settings.apply(&Overrides {
            name: Some("Flagged".to_string()),
            radius: None,
            gamemode: Some(GameMode::Spectator),
        });
        assert_eq!(settings.name, "Flagged");
        assert_eq!(settings.radius, DEFAULT_RADIUS);
        assert_eq!(settings.gamemode, GameMode::Spectator);
    }

    #[test]
    fn names_follow_the_rules() {
        assert!(is_valid_name("Limonite"));
        assert!(is_valid_name("Name With Spaces"));
        assert!(is_valid_name(&"É".repeat(16)), "16 characters, though 32 bytes");
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("SeventeenLetters!"));
        assert!(!is_valid_name("tab\there"));
    }
}
