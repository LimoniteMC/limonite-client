//! The client's own UI (PLAN.md, UI; m5-plan Decision 14): a retained tree of
//! widgets, laid out the JSON UI way, drawn last over the frame in textured quads.
//!
//! Each file is one idea:
//! - [`lang`], [`atlas`] and [`font`] are what the UI reads from the install: strings,
//!   pictures and text. Nothing is bundled, or written to disk (CLAUDE.md, Rules).
//! - [`tree`] and [`layout`] are the widgets and where they go. They know nothing of
//!   the GPU, so they're tested without one.
//! - [`render`] turns the tree into quads and draws them. [`input`] is the pointer.
//! - [`screens`] are what's built from all that. The HUD is the first.
//!
//! **The UI reads the game; it never reads packets** (PLAN.md, UI). [`Ui::update`]
//! is handed the [`Game`], and that's its only way in.

pub mod atlas;
pub mod font;
pub mod input;
pub mod lang;
pub mod layout;
pub mod render;
pub mod screens;
#[cfg(test)]
pub mod sheet;
pub mod tree;

use std::time::Instant;

use image::RgbaImage;
use limonite_packs::PackStack;
use tracing::{info, warn};

use self::{
    atlas::{Atlas, Input},
    font::{ASCII_CELLS, ASCII_SHEET, AsciiCells, Font, page_path},
    input::Pointer,
    lang::Lang,
    render::{Drawing, Looks},
    screens::hud::{self, Hud, HudState},
    tree::Tree,
};
use crate::{game::Game, settings::UiScale};

/// The smallest screen the UI is laid out for, in UI units (m5-plan Decision 14).
const MIN_SCREEN: [u32; 2] = [320, 240];

/// The strings the menus will show, checked at startup so that one a new version of
/// the game renames is noticed before a menu needs it (m5-plan Finding 16).
const STRINGS: [&str; 10] = [
    "selectWorld.title",
    "selectWorld.create",
    "selectWorld.delete",
    "menu.quit",
    "menu.returnToGame",
    "container.inventory",
    "gameMode.creative",
    "gameMode.survival",
    "deathScreen.title",
    "deathScreen.respawn",
];

/// What the UI reads from the install, before the window opens.
pub struct Assets {
    pub atlas: Atlas,
    /// Uploaded when the GPU comes up, then freed.
    pub picture: RgbaImage,
    pub font: Font,
}

/// Reads the strings, the sprites the screens ask for and the font's sheets, and logs
/// what it found and what's missing. A missing sprite draws as nothing, and a missing
/// sheet's characters as spaces: the game goes on.
pub fn load(stack: &PackStack) -> Assets {
    load_with(stack, &[])
}

/// As [`load`], with some more sprites.
fn load_with(stack: &PackStack, extra: &[&str]) -> Assets {
    let start = Instant::now();
    let (lang, problems) = Lang::load(stack);
    for problem in &problems {
        warn!("Skipped a strings file: {problem}");
    }
    let mut missing = Vec::new();
    let mut inputs: Vec<Input> = Vec::new();
    let mut take = |path: &str, read: Result<Input, String>| match read {
        Ok(input) => inputs.push(input),
        Err(reason) => missing.push(format!("{path} ({reason})")),
    };
    for path in hud::SPRITES.iter().chain(extra) {
        take(path, atlas::read_sprite(stack, path));
    }
    for part in hud::PARTS {
        take(part.path, atlas::read_part(stack, part.name, part.path, part.rect));
    }
    take(ASCII_SHEET, atlas::read_sprite(stack, ASCII_SHEET));
    let pages = lang.glyph_pages();
    for &page in &pages {
        let path = page_path(page);
        take(&path, atlas::read_sprite(stack, &path));
    }
    let (atlas, picture) = Atlas::pack(inputs);
    let cells = stack.read(ASCII_CELLS).ok().flatten().and_then(|found| AsciiCells::parse(&found.bytes));
    if cells.is_none() {
        warn!("{ASCII_CELLS} is missing or not as expected, so the font's cells are taken to be square");
    }
    let font = Font::new(&atlas, &picture, cells.unwrap_or(AsciiCells::PLAIN), &pages);

    if !missing.is_empty() {
        warn!("Missing from the install, so the UI goes without: {}", missing.join("; "));
    }
    let lost_strings: Vec<&str> = STRINGS.iter().copied().filter(|key| lang.get(key).is_none()).collect();
    if !lost_strings.is_empty() {
        warn!("Strings the menus want that the install lacks: {}", lost_strings.join(", "));
    }
    let [width, height] = atlas.size();
    info!(
        "UI: {} sprites in a {width}×{height} atlas ({} missing), with ASCII and {} glyph pages; \
         {} strings ({} of the menus' {} missing); {:.0} ms",
        atlas.len(),
        missing.len(),
        pages.len(),
        lang.len(),
        lost_strings.len(),
        STRINGS.len(),
        start.elapsed().as_secs_f64() * 1000.0,
    );
    Assets { atlas, picture, font }
}

/// How many window pixels a UI unit is.
#[must_use]
pub fn scale_for(setting: UiScale, width: u32, height: u32) -> u32 {
    match setting {
        UiScale::Fixed(scale) => u32::from(scale),
        // The largest whole scale at which the smallest screen still fits, as Java's
        // GUI scale does. It's never 0, however small the window.
        UiScale::Auto => (width / MIN_SCREEN[0]).min(height / MIN_SCREEN[1]).max(1),
    }
}

/// The UI, on the CPU.
pub struct Ui {
    tree: Tree,
    hud: Hud,
    atlas: Atlas,
    font: Font,
    setting: UiScale,
    /// The window's size in pixels, and how many pixels a UI unit is.
    window: [u32; 2],
    scale: u32,
    pointer: Pointer,
    drawing: Drawing,
}

impl Ui {
    /// Builds the screens.
    #[must_use]
    pub fn new(atlas: Atlas, font: Font, setting: UiScale) -> Self {
        let mut tree = Tree::new();
        let hud = Hud::build(&mut tree, &atlas);
        Self {
            tree,
            hud,
            atlas,
            font,
            setting,
            window: [1, 1],
            scale: 1,
            pointer: Pointer::default(),
            drawing: Drawing::default(),
        }
    }

    /// Follows the window's size.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || self.window == [width, height] {
            return;
        }
        self.window = [width, height];
        self.scale = scale_for(self.setting, width, height);
        self.tree.invalidate();
    }

    /// Brings the screens up to date with the game.
    pub fn update(&mut self, game: &Game) {
        self.hud.update(&mut self.tree, HudState::of(game).as_ref());
    }

    /// Shows a line above the hotbar, or nothing.
    pub fn set_note(&mut self, text: Option<&str>) {
        self.hud.set_note(&mut self.tree, text);
    }

    /// The quads to draw, if they've changed since the last time this was asked.
    pub fn drawing(&mut self) -> Option<&Drawing> {
        if !self.tree.take_changed() {
            return None;
        }
        let screen = self.screen();
        let looks = Looks {
            atlas: &self.atlas,
            font: &self.font,
            scale: self.scale as f32,
        };
        layout::run(&mut self.tree, screen, &looks);
        render::build(&self.tree, looks, &mut self.drawing);
        Some(&self.drawing)
    }

    /// How many UI units fit in the window.
    fn screen(&self) -> [f32; 2] {
        self.window.map(|side| side as f32 / self.scale as f32)
    }

    /// The pointer moved, to here in window pixels.
    pub fn pointer_moved(&mut self, x: f64, y: f64) {
        let scale = f64::from(self.scale);
        self.pointer.moved([(x / scale) as f32, (y / scale) as f32]);
    }

    /// The pointer's button went down. Says whether the UI took it: if not, the game
    /// can have the click.
    pub fn pointer_pressed(&mut self) -> bool {
        self.pointer.press(&self.tree)
    }

    /// The pointer's button came up. Nothing acts on a click until Step 12's inventory.
    pub fn pointer_released(&mut self) {
        self.pointer.release(&self.tree);
    }
}

#[cfg(test)]
pub mod tests {
    use image::Rgba;

    use super::{atlas::Input, *};

    /// Every sprite the screens ask for, at its real size and fully drawn, and an empty
    /// font. Made up, so the tests need no install.
    pub fn atlas() -> Atlas {
        let flat = |name: &str, width, height| Input {
            name: name.to_string(),
            image: RgbaImage::from_pixel(width, height, Rgba([255; 4])),
            nine_slice: None,
        };
        let mut inputs: Vec<Input> = hud::SPRITES
            .iter()
            .map(|path| {
                let (width, height) = if path.contains("hotbar_start") || path.contains("hotbar_end") {
                    (1, 22)
                } else if path.contains("selected") {
                    (24, 24)
                } else if path.contains("hotbar") {
                    (20, 22)
                } else {
                    (9, 9)
                };
                flat(path, width, height)
            })
            .collect();
        inputs.extend(hud::PARTS.iter().map(|part| flat(part.name, part.rect[2], part.rect[3])));
        Atlas::pack(inputs).0
    }

    #[test]
    fn the_automatic_scale_is_the_largest_that_fits() {
        let auto = |width, height| scale_for(UiScale::Auto, width, height);
        assert_eq!(auto(320, 240), 1);
        assert_eq!(auto(640, 480), 2);
        assert_eq!(auto(639, 480), 1, "one pixel short of 2");
        assert_eq!(auto(1280, 720), 3, "the height is what limits it");
        assert_eq!(auto(1920, 1080), 4);
        assert_eq!(auto(3840, 2160), 9);
        assert_eq!(auto(100, 100), 1, "never 0");
    }

    #[test]
    fn a_fixed_scale_ignores_the_window() {
        assert_eq!(scale_for(UiScale::Fixed(5), 100, 100), 5);
    }

    #[test]
    fn nothing_changing_draws_nothing_new() {
        let mut ui = Ui::new(atlas(), Font::default(), UiScale::Auto);
        ui.resize(1280, 720);
        assert!(ui.drawing().is_some(), "the first frame");
        assert!(ui.drawing().is_none());

        ui.resize(1280, 720);
        assert!(ui.drawing().is_none(), "the same size is no change");

        ui.resize(1600, 900);
        assert!(ui.drawing().is_some());
        assert_eq!(ui.scale, 3);
    }

    #[test]
    fn a_minimised_window_keeps_its_last_size() {
        let mut ui = Ui::new(atlas(), Font::default(), UiScale::Auto);
        ui.resize(1280, 720);
        ui.drawing();
        ui.resize(0, 0);
        assert_eq!(ui.window, [1280, 720]);
        assert!(ui.drawing().is_none());
    }

    #[test]
    fn the_screen_is_in_units_and_the_pointer_too() {
        let mut ui = Ui::new(atlas(), Font::default(), UiScale::Fixed(2));
        ui.resize(800, 600);
        assert_eq!(ui.screen(), [400.0, 300.0]);
        // Nothing on the HUD takes a click, so the game gets it.
        ui.pointer_moved(400.0, 300.0);
        assert!(!ui.pointer_pressed());
        ui.pointer_released();
    }
}
