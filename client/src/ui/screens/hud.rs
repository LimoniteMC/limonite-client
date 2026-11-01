//! The HUD: the crosshair and the hotbar, and in survival hearts, hunger, armour and
//! air (m5-plan Decision 15). The slots stay empty until Step 9.
//!
//! **Where everything goes** comes from the install's `ui/hud_screen.json`, read while
//! this was written and not at runtime, until milestone 10's JSON UI:
//! - the hotbar is 182 × 22: a cap, nine 20-wide slots, and a cap. Its bottom is 1.5
//!   units above the window's, which is what the chain of offsets there comes to;
//! - the selected slot's frame is 24 × 24, centred on the slot;
//! - hearts and hunger are 9 × 9 and 8 apart, so they overlap by a column. Their rows
//!   span 91 units either side of the middle, 41 units up;
//! - armour goes a row above the hearts, and air a row above the hunger;
//! - the crosshair is 16 × 16, in the middle, and inverts what's behind it.
//!
//! **Hunger and air fill from the right**, so those rows are built from their far end.
//!
//! **The experience bar** isn't in M5's HUD (Decision 15), though the space above the
//! hotbar is its. **Armour** isn't sent by the server, because Bedrock works it out
//! from what the player wears, so its row waits for Step 9's inventory.

use pumpkin_util::GameMode;

use crate::{
    game::Game,
    ui::{
        atlas::{Atlas, SpriteId},
        layout::{Anchor, Arrange, Axis, Layout},
        tree::{Node, NodeId, Tree},
    },
};

const HOTBAR_START: &str = "textures/ui/hotbar_start_cap";
const HOTBAR_END: &str = "textures/ui/hotbar_end_cap";
const HOTBAR_SLOTS: [&str; 9] = [
    "textures/ui/hotbar_0",
    "textures/ui/hotbar_1",
    "textures/ui/hotbar_2",
    "textures/ui/hotbar_3",
    "textures/ui/hotbar_4",
    "textures/ui/hotbar_5",
    "textures/ui/hotbar_6",
    "textures/ui/hotbar_7",
    "textures/ui/hotbar_8",
];
const SELECTED: &str = "textures/ui/selected_hotbar_slot";
const HEART: Icons = Icons {
    background: Some("textures/ui/heart_background"),
    full: "textures/ui/heart",
    half: "textures/ui/heart_half",
};
const HUNGER: Icons = Icons {
    background: Some("textures/ui/hunger_background"),
    full: "textures/ui/hunger_full",
    half: "textures/ui/hunger_half",
};
const ARMOUR: Icons = Icons {
    background: Some("textures/ui/armor_empty"),
    full: "textures/ui/armor_full",
    half: "textures/ui/armor_half",
};
const AIR: Icons = Icons {
    background: None,
    full: "textures/ui/bubble",
    half: "textures/ui/bubble_pop",
};
const CROSSHAIR: &str = "crosshair";

/// The whole pictures the HUD asks for.
pub const SPRITES: [&str; 23] = [
    HOTBAR_START,
    HOTBAR_END,
    HOTBAR_SLOTS[0],
    HOTBAR_SLOTS[1],
    HOTBAR_SLOTS[2],
    HOTBAR_SLOTS[3],
    HOTBAR_SLOTS[4],
    HOTBAR_SLOTS[5],
    HOTBAR_SLOTS[6],
    HOTBAR_SLOTS[7],
    HOTBAR_SLOTS[8],
    SELECTED,
    "textures/ui/heart_background",
    HEART.full,
    HEART.half,
    "textures/ui/hunger_background",
    HUNGER.full,
    HUNGER.half,
    "textures/ui/armor_empty",
    ARMOUR.full,
    ARMOUR.half,
    AIR.full,
    AIR.half,
];

/// A piece of a bigger picture.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub name: &'static str,
    pub path: &'static str,
    /// Its left, top, width and height, in texels.
    pub rect: [u32; 4],
}

/// The parts of bigger pictures the HUD asks for.
pub const PARTS: [Part; 1] = [Part {
    name: CROSSHAIR,
    path: "textures/gui/icons",
    rect: [0, 0, 16, 16],
}];

/// How long the breath lasts, in ticks: the server's `MAX_AIR`.
const MAX_AIR: f32 = 300.0;
/// How many icons fit a row.
const ROW: usize = 10;
/// The width of the whole bottom of the HUD: the hotbar's.
const WIDTH: f32 = 182.0;
/// A slot's width.
const SLOT: f32 = 20.0;
/// How far the hotbar's bottom is above the window's.
const HOTBAR_UP: f32 = 1.5;
/// How far the bottom of the hearts and hunger is above the window's.
const STATS_UP: f32 = 32.0;
/// How much higher the next row is.
const ROW_UP: f32 = 10.0;
/// How far the bottom of a note is above the window's: over the hearts and armour.
const NOTE_UP: f32 = 59.0;

/// What the HUD shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudState {
    pub gamemode: GameMode,
    pub health: f32,
    pub max_health: f32,
    pub hunger: f32,
    /// Armour points, 0 to 20. Nothing fills it in until Step 9's inventory.
    pub armour: u32,
    /// Breath, in ticks.
    pub air: f32,
    /// The selected hotbar slot, 0 to 8. Nothing changes it until Step 9.
    pub selected: u8,
}

impl HudState {
    /// What the game says now. There's nothing to show before the player exists.
    #[must_use]
    pub fn of(game: &Game) -> Option<Self> {
        let player = game.player()?;
        Some(Self {
            gamemode: player.gamemode,
            health: player.attributes.health,
            max_health: player.attributes.max_health,
            hunger: player.attributes.hunger,
            armour: 0,
            air: player.attributes.air,
            selected: 0,
        })
    }
}

/// A row's pictures, by path.
#[derive(Clone, Copy)]
struct Icons {
    background: Option<&'static str>,
    full: &'static str,
    half: &'static str,
}

/// A row of ten icons, each a background with what fills it on top. Index 0 is where
/// the row fills from.
struct Row {
    node: NodeId,
    cells: [NodeId; ROW],
    fills: [NodeId; ROW],
    full: Option<SpriteId>,
    half: Option<SpriteId>,
}

impl Row {
    fn build(tree: &mut Tree, atlas: &Atlas, parent: NodeId, layout: Layout, icons: Icons, from_right: bool) -> Self {
        let layout = layout.arranged(Arrange::Grid {
            columns: ROW as u32,
            cell: [8.0, 9.0],
        });
        let node = tree.add(parent, Node::panel(layout));
        let background = icons.background.and_then(|path| atlas.sprite(path));
        let icon = Layout::px(9.0, 9.0).at(Anchor::TopLeft);
        let mut built = Vec::with_capacity(ROW);
        for _ in 0..ROW {
            let cell = tree.add(node, Node::image(icon, background));
            built.push((cell, tree.add(cell, Node::image(icon, None))));
        }
        if from_right {
            built.reverse();
        }
        Self {
            node,
            cells: std::array::from_fn(|i| built[i].0),
            fills: std::array::from_fn(|i| built[i].1),
            full: atlas.sprite(icons.full),
            half: atlas.sprite(icons.half),
        }
    }

    /// Shows `points`, two to an icon, so an odd number ends in a half.
    fn show(&self, tree: &mut Tree, points: u32) {
        for (index, &fill) in (0_u32..).zip(&self.fills) {
            let sprite = if points >= index * 2 + 2 {
                self.full
            } else if points == index * 2 + 1 {
                self.half
            } else {
                None
            };
            tree.set_sprite(fill, sprite);
        }
    }

    /// Shows `full` whole icons, and then `popping` of the half kind.
    fn show_counted(&self, tree: &mut Tree, full: u32, popping: u32) {
        for (index, &fill) in (0_u32..).zip(&self.fills) {
            let sprite = if index < full {
                self.full
            } else if index < full + popping {
                self.half
            } else {
                None
            };
            tree.set_sprite(fill, sprite);
        }
    }
}

/// How many bubbles are whole and how many are bursting, as vanilla works it out: the
/// last one bursts for the two ticks before it's gone.
fn bubbles(air: f32) -> (u32, u32) {
    // Multiplying first: a tenth of a bubble per tick isn't exact in an `f32`, and a
    // full breath times it comes out a hair over ten, which would round up to eleven.
    let in_bubbles = |ticks: f32| (ticks * ROW as f32 / MAX_AIR).ceil().max(0.0) as u32;
    let full = in_bubbles(air - 2.0);
    (full, in_bubbles(air).saturating_sub(full))
}

/// A point for each half-icon, rounded up so that the last sliver still shows.
fn points(value: f32) -> u32 {
    value.ceil().max(0.0) as u32
}

/// The HUD's nodes, kept to change them.
pub struct Hud {
    root: NodeId,
    hotbar: NodeId,
    selected: NodeId,
    stats: NodeId,
    hearts: Row,
    hunger: Row,
    armour: Row,
    air: Row,
    note: NodeId,
}

impl Hud {
    /// Builds the HUD, hidden until there's a player.
    pub fn build(tree: &mut Tree, atlas: &Atlas) -> Self {
        let root = tree.add(Tree::ROOT, Node::panel(Layout::default()).hidden());
        tree.add(
            root,
            Node::image(Layout::px(16.0, 16.0), atlas.sprite(CROSSHAIR)).inverting(),
        );

        // The stats, under the hotbar in the tree: they don't overlap it, and a slot's
        // frame goes over both.
        let stats_layout = Layout::px(WIDTH, 50.0).at(Anchor::BottomMiddle);
        let stats = tree.add(root, Node::panel(stats_layout));
        let row = |up: f32, side: Anchor, inset: f32| Layout::content().at(side).moved(inset, -up);
        let left = |up| row(up, Anchor::BottomLeft, 0.0);
        // A row is 80 wide, and its last icon hangs one over: inset by that much,
        // the right-hand rows end level with the hotbar.
        let right = |up| row(up, Anchor::BottomRight, -1.0);
        let hearts = Row::build(tree, atlas, stats, left(STATS_UP), HEART, false);
        let hunger = Row::build(tree, atlas, stats, right(STATS_UP), HUNGER, true);
        let armour = Row::build(tree, atlas, stats, left(STATS_UP + ROW_UP), ARMOUR, false);
        let air = Row::build(tree, atlas, stats, right(STATS_UP + ROW_UP), AIR, true);

        let hotbar_layout = Layout::content()
            .at(Anchor::BottomMiddle)
            .moved(0.0, -HOTBAR_UP)
            .arranged(Arrange::Stack(Axis::X));
        let hotbar = tree.add(root, Node::panel(hotbar_layout));
        let cap = Layout::px(1.0, 22.0);
        tree.add(hotbar, Node::image(cap, atlas.sprite(HOTBAR_START)));
        for path in HOTBAR_SLOTS {
            tree.add(hotbar, Node::image(Layout::px(SLOT, 22.0), atlas.sprite(path)));
        }
        tree.add(hotbar, Node::image(cap, atlas.sprite(HOTBAR_END)));

        // One frame that moves, after the hotbar, so that it draws over the slots on
        // both sides of the selected one.
        let selected = Layout::px(24.0, 24.0).at(Anchor::BottomMiddle);
        let selected = tree.add(root, Node::image(selected, atlas.sprite(SELECTED)));

        let note = Layout::content().at(Anchor::BottomMiddle).moved(0.0, -NOTE_UP);
        let note = tree.add(root, Node::label(note, "").hidden());

        let hud = Self {
            root,
            hotbar,
            selected,
            stats,
            hearts,
            hunger,
            armour,
            air,
            note,
        };
        hud.select(tree, 0);
        hud
    }

    /// Puts the frame on a slot.
    fn select(&self, tree: &mut Tree, slot: u8) {
        // Slot 4 is the middle one. The frame is 1 unit taller than the slot each way.
        let x = (f32::from(slot) - 4.0) * SLOT;
        tree.set_offset(self.selected, x, -(HOTBAR_UP - 1.0));
    }

    /// Brings the HUD up to date, hiding it if there's nothing to show.
    pub fn update(&self, tree: &mut Tree, state: Option<&HudState>) {
        let Some(state) = state else {
            tree.set_visible(self.root, false);
            return;
        };
        tree.set_visible(self.root, true);
        // A spectator holds nothing and has nothing to lose.
        let playing = state.gamemode != GameMode::Spectator;
        tree.set_visible(self.hotbar, playing);
        tree.set_visible(self.selected, playing);
        self.select(tree, state.selected);
        tree.set_visible(self.stats, matches!(state.gamemode, GameMode::Survival | GameMode::Adventure));

        self.hearts.show(tree, points(state.health));
        // A smaller maximum means fewer hearts, and their row stays on the left. Above
        // 20 it stays at ten for now: effects that raise it arrive with entities' data.
        let hearts = ((state.max_health / 2.0).ceil() as usize).clamp(1, ROW);
        for (index, &cell) in self.hearts.cells.iter().enumerate() {
            tree.set_visible(cell, index < hearts);
        }
        self.hunger.show(tree, points(state.hunger));

        tree.set_visible(self.armour.node, state.armour > 0);
        self.armour.show(tree, state.armour);

        tree.set_visible(self.air.node, state.air < MAX_AIR);
        let (full, popping) = bubbles(state.air);
        self.air.show_counted(tree, full, popping);
    }

    /// Shows a line above the hotbar, or nothing.
    pub fn set_note(&self, tree: &mut Tree, text: Option<&str>) {
        tree.set_visible(self.note, text.is_some());
        tree.set_text(self.note, text.unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        font::Font,
        layout::{self, Rect},
        render::Looks,
        tests::atlas,
        tree::Kind,
    };

    /// 1280 × 720 at the automatic scale of 3.
    const SCREEN: [f32; 2] = [1280.0 / 3.0, 240.0];

    fn state() -> HudState {
        HudState {
            gamemode: GameMode::Survival,
            health: 20.0,
            max_health: 20.0,
            hunger: 20.0,
            armour: 0,
            air: MAX_AIR,
            selected: 0,
        }
    }

    struct Fixture {
        tree: Tree,
        hud: Hud,
        atlas: Atlas,
    }

    impl Fixture {
        fn new() -> Self {
            let atlas = atlas();
            let mut tree = Tree::new();
            let hud = Hud::build(&mut tree, &atlas);
            let mut fixture = Self { tree, hud, atlas };
            fixture.update(state());
            fixture
        }

        fn update(&mut self, state: HudState) {
            self.hud.update(&mut self.tree, Some(&state));
            let font = Font::default();
            let looks = Looks {
                atlas: &self.atlas,
                font: &font,
                scale: 3.0,
            };
            layout::run(&mut self.tree, SCREEN, &looks);
        }

        fn rect(&self, id: NodeId) -> Rect {
            self.tree.node(id).rect
        }

        fn shown(&self, id: NodeId) -> bool {
            self.tree.node(id).visible
        }

        /// Which picture a row's icon has.
        fn sprite(&self, id: NodeId) -> Option<SpriteId> {
            match &self.tree.node(id).kind {
                Kind::Image(image) => image.sprite,
                other => panic!("not a picture: {other:?}"),
            }
        }

        /// A row, as `F` for full, `H` for half and `.` for nothing, from where it fills.
        fn row(&self, row: &Row) -> String {
            let icon = |&fill: &NodeId| match self.sprite(fill) {
                None => '.',
                sprite if sprite == row.full => 'F',
                _ => 'H',
            };
            row.fills.iter().map(icon).collect()
        }
    }

    /// The middle of the screen, across.
    const MIDDLE: f32 = SCREEN[0] / 2.0;

    #[test]
    fn the_hotbar_sits_where_the_official_clients_does() {
        let fixture = Fixture::new();
        let hotbar = fixture.rect(fixture.hud.hotbar);
        assert_eq!(hotbar.size(), [182.0, 22.0]);
        assert_eq!(hotbar.x, MIDDLE - 91.0);
        assert_eq!(hotbar.y + hotbar.height, SCREEN[1] - 1.5);
    }

    #[test]
    fn the_frame_goes_round_the_selected_slot() {
        let mut fixture = Fixture::new();
        for slot in 0..9_u8 {
            fixture.update(HudState { selected: slot, ..state() });
            let frame = fixture.rect(fixture.hud.selected);
            let hotbar = fixture.rect(fixture.hud.hotbar);
            // One cap, then the slots before it. The frame is 24 × 24, and centred on
            // the 20 × 22 slot.
            let slot_left = hotbar.x + 1.0 + f32::from(slot) * SLOT;
            assert_eq!(frame.x + 12.0, slot_left + SLOT / 2.0, "slot {slot}");
            assert_eq!(frame.y + 12.0, hotbar.y + 11.0, "slot {slot}");
        }
    }

    #[test]
    fn hearts_and_hunger_span_the_hotbar() {
        let fixture = Fixture::new();
        let first_heart = fixture.rect(fixture.hud.hearts.cells[0]);
        assert_eq!(first_heart.x, MIDDLE - 91.0);
        assert_eq!(first_heart.y, SCREEN[1] - 41.0);
        assert_eq!(first_heart.size(), [9.0, 9.0]);
        assert_eq!(fixture.rect(fixture.hud.hearts.cells[9]).x, MIDDLE - 91.0 + 72.0, "8 apart");

        // Index 0 of hunger is its rightmost, and it ends level with the hotbar.
        let last_hunger = fixture.rect(fixture.hud.hunger.cells[0]);
        assert_eq!(last_hunger.x + 9.0, MIDDLE + 91.0);
        assert_eq!(last_hunger.y, first_heart.y);
        assert_eq!(fixture.rect(fixture.hud.hunger.cells[1]).x, last_hunger.x - 8.0);
    }

    #[test]
    fn armour_and_air_go_a_row_above() {
        let mut fixture = Fixture::new();
        fixture.update(HudState { armour: 4, air: 150.0, ..state() });
        let heart = fixture.rect(fixture.hud.hearts.cells[0]);
        let armour = fixture.rect(fixture.hud.armour.cells[0]);
        assert_eq!((armour.x, armour.y), (heart.x, heart.y - 10.0));
        let hunger = fixture.rect(fixture.hud.hunger.cells[0]);
        let air = fixture.rect(fixture.hud.air.cells[0]);
        assert_eq!((air.x, air.y), (hunger.x, hunger.y - 10.0));
    }

    #[test]
    fn points_fill_two_to_an_icon_and_round_up() {
        let mut fixture = Fixture::new();
        assert_eq!(fixture.row(&fixture.hud.hearts), "FFFFFFFFFF");
        fixture.update(HudState { health: 13.0, ..state() });
        assert_eq!(fixture.row(&fixture.hud.hearts), "FFFFFFH...");
        fixture.update(HudState { health: 12.5, ..state() });
        assert_eq!(fixture.row(&fixture.hud.hearts), "FFFFFFH...", "12.5 shows as 13");
        fixture.update(HudState { health: 0.0, ..state() });
        assert_eq!(fixture.row(&fixture.hud.hearts), "..........");

        fixture.update(HudState { hunger: 7.0, ..state() });
        assert_eq!(fixture.row(&fixture.hud.hunger), "FFFH......");
        // It's index 0 that's on the right, so hunger empties from the left.
        let filled = fixture.hud.hunger.fills[0];
        assert!(fixture.rect(filled).x > fixture.rect(fixture.hud.hunger.fills[3]).x);
    }

    #[test]
    fn a_smaller_maximum_is_fewer_hearts() {
        let mut fixture = Fixture::new();
        fixture.update(HudState { health: 6.0, max_health: 10.0, ..state() });
        assert!(fixture.shown(fixture.hud.hearts.cells[4]));
        assert!(!fixture.shown(fixture.hud.hearts.cells[5]));
        // More than a row stays a row, for now.
        fixture.update(HudState { max_health: 40.0, ..state() });
        assert!(fixture.shown(fixture.hud.hearts.cells[9]));
    }

    #[test]
    fn armour_shows_only_when_worn() {
        let mut fixture = Fixture::new();
        assert!(!fixture.shown(fixture.hud.armour.node));
        fixture.update(HudState { armour: 5, ..state() });
        assert!(fixture.shown(fixture.hud.armour.node));
        assert_eq!(fixture.row(&fixture.hud.armour), "FFH.......");
    }

    #[test]
    fn air_shows_only_when_short_and_bursts_as_it_goes() {
        let mut fixture = Fixture::new();
        assert!(!fixture.shown(fixture.hud.air.node), "a full breath");
        fixture.update(HudState { air: 299.0, ..state() });
        assert!(fixture.shown(fixture.hud.air.node));
        assert_eq!(fixture.row(&fixture.hud.air), "FFFFFFFFFF");
        fixture.update(HudState { air: 150.0, ..state() });
        assert_eq!(fixture.row(&fixture.hud.air), "FFFFF.....");
        fixture.update(HudState { air: 151.0, ..state() });
        assert_eq!(fixture.row(&fixture.hud.air), "FFFFFH....", "the sixth is about to go");
        fixture.update(HudState { air: 0.0, ..state() });
        assert_eq!(fixture.row(&fixture.hud.air), "..........");
    }

    #[test]
    fn bubbles_are_vanillas_sums() {
        assert_eq!(bubbles(300.0), (10, 0));
        assert_eq!(bubbles(291.0), (10, 0));
        assert_eq!(bubbles(290.0), (10, 0));
        assert_eq!(bubbles(32.0), (1, 1), "30 ticks a bubble, and two to burst");
        assert_eq!(bubbles(31.0), (1, 1));
        assert_eq!(bubbles(30.0), (1, 0));
        assert_eq!(bubbles(2.0), (0, 1));
        assert_eq!(bubbles(0.0), (0, 0));
        assert_eq!(bubbles(-20.0), (0, 0), "the server can go below nothing while drowning");
    }

    #[test]
    fn each_gamemode_shows_what_it_should() {
        let mut fixture = Fixture::new();
        let shows = |fixture: &Fixture| (fixture.shown(fixture.hud.hotbar), fixture.shown(fixture.hud.stats));
        assert_eq!(shows(&fixture), (true, true), "survival");
        fixture.update(HudState { gamemode: GameMode::Adventure, ..state() });
        assert_eq!(shows(&fixture), (true, true), "adventure");
        fixture.update(HudState { gamemode: GameMode::Creative, ..state() });
        assert_eq!(shows(&fixture), (true, false), "creative");
        fixture.update(HudState { gamemode: GameMode::Spectator, ..state() });
        assert_eq!(shows(&fixture), (false, false), "spectator");
        assert!(!fixture.shown(fixture.hud.selected));
    }

    #[test]
    fn no_player_no_hud() {
        let mut fixture = Fixture::new();
        assert!(fixture.shown(fixture.hud.root));
        fixture.hud.update(&mut fixture.tree, None);
        assert!(!fixture.shown(fixture.hud.root));
    }

    #[test]
    fn a_note_shows_and_goes() {
        let mut fixture = Fixture::new();
        assert!(!fixture.shown(fixture.hud.note));
        fixture.hud.set_note(&mut fixture.tree, Some("oak_door"));
        assert!(fixture.shown(fixture.hud.note));
        fixture.hud.set_note(&mut fixture.tree, None);
        assert!(!fixture.shown(fixture.hud.note));
    }

    #[test]
    fn a_settled_hud_changes_nothing() {
        let mut fixture = Fixture::new();
        fixture.tree.take_changed();
        fixture.hud.update(&mut fixture.tree, Some(&state()));
        assert!(!fixture.tree.take_changed(), "the same state is no work");
    }

    #[test]
    fn every_sprite_the_hud_uses_is_asked_for() {
        for path in HOTBAR_SLOTS.iter().chain(&[HOTBAR_START, HOTBAR_END, SELECTED]) {
            assert!(SPRITES.contains(path), "{path}");
        }
        for icons in [HEART, HUNGER, ARMOUR, AIR] {
            assert!(SPRITES.contains(&icons.full) && SPRITES.contains(&icons.half));
            if let Some(background) = icons.background {
                assert!(SPRITES.contains(&background));
            }
        }
    }
}
