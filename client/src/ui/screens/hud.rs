//! The HUD: the crosshair and the hotbar, and in survival the experience bar, hearts,
//! hunger, armour and air (m5-plan Decision 15 and Deviation 49).
//!
//! **Where everything goes** comes from the install's `ui/hud_screen.json`, read while
//! this was written and not at runtime, until milestone 10's JSON UI:
//! - the hotbar is 182 × 22: a cap, nine 20-wide slots, and a cap. Its bottom is 1.5
//!   units above the window's, which is what the chain of offsets there comes to;
//! - the selected slot's frame is 24 × 24, centred on the slot;
//! - the experience bar is as wide as the hotbar and 5 tall, with its bottom 26 units up:
//!   2.5 over the hotbar, which leaves room for the selected slot's frame, and 1 under the
//!   hearts. The level goes on top of it, in the middle, once there is one;
//! - hearts and hunger are 9 × 9 and 8 apart, so they overlap by a column. Their rows
//!   span 91 units either side of the middle, 41 units up;
//! - armour goes a row above the hearts, and air a row above the hunger;
//! - the crosshair is 16 × 16, in the middle, and inverts what's behind it.
//!
//! **Hunger and air fill from the right**, so those rows are built from their far end.
//!
//! **The experience bar** is three pictures: the empty bar, the full one on top of it with
//! only its left part showing, and over both a tick every 11 units, from 5 in. Without it
//! the hearts look adrift, since the gap above the hotbar is its.
//!
//! **Armour** isn't sent by the server: Bedrock works it out from what the player wears, and
//! Pumpkin doesn't send a Bedrock player's armour slots either. So its row waits for Step
//! 12's inventory screen, where the client sees its own moves.
//!
//! **A slot** is 20 × 22, with its icon 16 × 16 in the middle of it, as the JSON has them,
//! and the count at its bottom right.

use pumpkin_util::GameMode;

use crate::{
    game::Game,
    inventory::{
        HOTBAR, Item,
        items::{ItemIcons, display_name},
    },
    ui::{
        atlas::{Atlas, SpriteId},
        lang::Lang,
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
const XP_EMPTY: &str = "textures/ui/experiencebarempty";
const XP_FULL: &str = "textures/ui/experiencebarfull";
const XP_TICK: &str = "textures/ui/experiencenub";
const CROSSHAIR: &str = "crosshair";

/// The whole pictures the HUD asks for.
pub const SPRITES: [&str; 26] = [
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
    XP_EMPTY,
    XP_FULL,
    XP_TICK,
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
/// How far the bottom of the experience bar is above the window's, and how tall it is.
const XP_UP: f32 = 26.0;
const XP_HEIGHT: f32 = 5.0;
/// The picture of a tick on the bar, with the tick in its middle: how wide it is, how far
/// in the first one starts, and how many reach across.
const XP_TICK_WIDTH: f32 = 11.0;
const XP_TICKS_IN: f32 = 5.0;
const XP_TICKS: usize = 16;
/// The colour of the level.
const XP_TEXT: [u8; 4] = [128, 255, 0, 255];
/// How far the bottom of the hearts and hunger is above the window's.
const STATS_UP: f32 = 32.0;
/// How much higher the next row is.
const ROW_UP: f32 = 10.0;
/// How far the bottom of a note is above the window's: over the hearts and armour.
const NOTE_UP: f32 = 59.0;
/// How long the held item's name stays up, in ticks.
const NAME_TICKS: u64 = 60;

/// What the HUD shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudState {
    pub gamemode: GameMode,
    pub health: f32,
    pub max_health: f32,
    pub hunger: f32,
    /// Armour points, 0 to 20. Nothing fills it in until Step 12's inventory screen.
    pub armour: u32,
    /// Breath, in ticks.
    pub air: f32,
    /// How far through this level the player is, 0 to 1, and the level.
    pub experience: f32,
    pub level: u32,
    /// The selected hotbar slot, 0 to 8.
    pub selected: u8,
    pub hotbar: [Option<Item>; HOTBAR],
    /// The game's ticks so far.
    pub tick: u64,
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
            experience: player.attributes.experience,
            level: player.attributes.level as u32,
            selected: game.inventory().selected(),
            hotbar: game.inventory().hotbar(),
            tick: game.ticks(),
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
    experience_full: NodeId,
    level: NodeId,
    /// The level the label says.
    shown_level: u32,
    note: NodeId,
    slots: [Slot; HOTBAR],
    /// What each slot shows now.
    shown: [Option<Item>; HOTBAR],
    /// The item whose name was last put up, and when that comes down.
    named: Option<(i16, u32)>,
    name_until: u64,
    /// Whether a note is up that isn't the held item's name.
    noted: bool,
}

/// What's on a slot.
#[derive(Clone, Copy)]
struct Slot {
    icon: NodeId,
    count: NodeId,
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

        let bar = Layout::px(WIDTH, XP_HEIGHT).at(Anchor::BottomMiddle).moved(0.0, -XP_UP);
        let experience = tree.add(stats, Node::image(bar, atlas.sprite(XP_EMPTY)));
        let experience_full = tree.add(experience, Node::image(Layout::px(WIDTH, XP_HEIGHT), atlas.sprite(XP_FULL)));
        let ticks = Layout::content()
            .at(Anchor::LeftMiddle)
            .moved(XP_TICKS_IN, 0.0)
            .arranged(Arrange::Stack(Axis::X));
        let ticks = tree.add(experience, Node::panel(ticks));
        for _ in 0..XP_TICKS {
            tree.add(ticks, Node::image(Layout::px(XP_TICK_WIDTH, XP_HEIGHT), atlas.sprite(XP_TICK)));
        }
        let level = Layout::content().at(Anchor::BottomMiddle).moved(0.0, -(XP_UP + XP_HEIGHT));
        let level = tree.add(stats, Node::label(level, "").colored(XP_TEXT).hidden());

        let hotbar_layout = Layout::content()
            .at(Anchor::BottomMiddle)
            .moved(0.0, -HOTBAR_UP)
            .arranged(Arrange::Stack(Axis::X));
        let hotbar = tree.add(root, Node::panel(hotbar_layout));
        let cap = Layout::px(1.0, 22.0);
        tree.add(hotbar, Node::image(cap, atlas.sprite(HOTBAR_START)));
        let slots = HOTBAR_SLOTS.map(|path| {
            let slot = tree.add(hotbar, Node::image(Layout::px(SLOT, 22.0), atlas.sprite(path)));
            let icon = tree.add(slot, Node::icon(Layout::px(16.0, 16.0)));
            let count = Layout::content().at(Anchor::BottomRight).moved(-1.0, -2.0);
            let count = tree.add(slot, Node::label(count, "").hidden());
            Slot { icon, count }
        });
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
            experience_full,
            level,
            shown_level: 0,
            note,
            slots,
            shown: [None; HOTBAR],
            named: None,
            name_until: 0,
            noted: false,
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
    pub fn update(&mut self, tree: &mut Tree, icons: &ItemIcons, lang: &Lang, state: Option<&HudState>) {
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
        self.show_slots(tree, icons, &state.hotbar);
        self.show_held_name(tree, icons, lang, state);
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

        tree.set_shown(self.experience_full, state.experience.clamp(0.0, 1.0));
        if state.level != self.shown_level {
            self.shown_level = state.level;
            tree.set_text(self.level, &state.level.to_string());
            tree.set_visible(self.level, state.level > 0);
        }
    }

    /// Puts what's in `hotbar` on the slots. Only a slot that changed costs anything.
    fn show_slots(&mut self, tree: &mut Tree, icons: &ItemIcons, hotbar: &[Option<Item>; HOTBAR]) {
        for ((slot, item), shown) in self.slots.iter().zip(hotbar).zip(&mut self.shown) {
            if item == shown {
                continue;
            }
            *shown = *item;
            tree.set_icon(slot.icon, item.and_then(|item| icons.get(item.id, item.aux)));
            let count = item.filter(|item| item.count > 1).map(|item| item.count.to_string());
            tree.set_visible(slot.count, count.is_some());
            tree.set_text(slot.count, count.as_deref().unwrap_or_default());
        }
    }

    /// Puts the held item's name up when it changes, and takes it down after a while.
    fn show_held_name(&mut self, tree: &mut Tree, icons: &ItemIcons, lang: &Lang, state: &HudState) {
        if self.noted {
            return;
        }
        let held = state.hotbar[usize::from(state.selected).min(HOTBAR - 1)];
        let key = held.map(|item| (item.id, item.aux));
        if key != self.named {
            self.named = key;
            let name = held.and_then(|item| icons.name(item.id)).map(|name| display_name(lang, name));
            tree.set_text(self.note, name.as_deref().unwrap_or_default());
            tree.set_visible(self.note, name.is_some());
            self.name_until = state.tick + NAME_TICKS;
        } else if state.tick >= self.name_until {
            tree.set_visible(self.note, false);
        }
    }

    /// Shows a line above the hotbar, in place of the held item's name, or nothing.
    pub fn set_note(&mut self, tree: &mut Tree, text: Option<&str>) {
        self.noted = text.is_some();
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
            experience: 0.0,
            level: 0,
            selected: 0,
            hotbar: [None; HOTBAR],
            tick: 0,
        }
    }

    struct Fixture {
        tree: Tree,
        hud: Hud,
        atlas: Atlas,
        icons: ItemIcons,
        lang: Lang,
    }

    impl Fixture {
        fn new() -> Self {
            let atlas = atlas();
            let mut tree = Tree::new();
            let hud = Hud::build(&mut tree, &atlas);
            let mut fixture = Self {
                tree,
                hud,
                atlas,
                icons: ItemIcons::default(),
                lang: Lang::default(),
            };
            fixture.update(state());
            fixture
        }

        fn update(&mut self, state: HudState) {
            self.hud.update(&mut self.tree, &self.icons, &self.lang, Some(&state));
            let font = Font::default();
            let looks = Looks {
                atlas: &self.atlas,
                font: &font,
                icons: &self.icons,
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
    fn the_experience_bar_fills_the_gap_between_the_hotbar_and_the_hearts() {
        let fixture = Fixture::new();
        let bar = fixture.rect(fixture.hud.experience_full);
        let hotbar = fixture.rect(fixture.hud.hotbar);
        let heart = fixture.rect(fixture.hud.hearts.cells[0]);
        assert_eq!(bar.size(), [182.0, 5.0]);
        assert_eq!(bar.x, hotbar.x, "as wide as the hotbar");
        assert_eq!(bar.y + bar.height, SCREEN[1] - 26.0);
        assert_eq!(hotbar.y - (bar.y + bar.height), 2.5, "over the hotbar");
        assert_eq!(bar.y - (heart.y + heart.height), 1.0, "under the hearts");
    }

    #[test]
    fn the_bar_fills_as_far_as_the_experience_goes() {
        let shown = |fixture: &Fixture| match &fixture.tree.node(fixture.hud.experience_full).kind {
            Kind::Image(image) => image.shown,
            other => panic!("not a picture: {other:?}"),
        };
        let mut fixture = Fixture::new();
        assert_eq!(shown(&fixture), 0.0);
        fixture.update(HudState { experience: 0.25, ..state() });
        assert_eq!(shown(&fixture), 0.25);
        fixture.update(HudState { experience: 7.0, ..state() });
        assert_eq!(shown(&fixture), 1.0, "and no further");
    }

    #[test]
    fn the_level_shows_on_the_bar_once_there_is_one() {
        let mut fixture = Fixture::new();
        assert!(!fixture.shown(fixture.hud.level), "level 0 says nothing");
        fixture.update(HudState { level: 7, ..state() });
        assert!(fixture.shown(fixture.hud.level));
        assert_eq!(text(&fixture, fixture.hud.level), "7");
        let (label, bar) = (fixture.rect(fixture.hud.level), fixture.rect(fixture.hud.experience_full));
        assert_eq!(label.y + label.height, bar.y, "on top of it");
        assert_eq!(label.x + label.width / 2.0, bar.x + bar.width / 2.0, "in the middle");
        fixture.update(state());
        assert!(!fixture.shown(fixture.hud.level));
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
        fixture.hud.update(&mut fixture.tree, &fixture.icons, &fixture.lang, None);
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
        fixture.hud.update(&mut fixture.tree, &fixture.icons, &fixture.lang, Some(&state()));
        assert!(!fixture.tree.take_changed(), "the same state is no work");
    }

    fn holding(hotbar: &[(usize, i16, u16)]) -> HudState {
        let mut state = state();
        for &(slot, id, count) in hotbar {
            state.hotbar[slot] = Some(Item { id, aux: 0, count });
        }
        state
    }

    fn text(fixture: &Fixture, id: NodeId) -> String {
        match &fixture.tree.node(id).kind {
            Kind::Label(label) => label.text.clone(),
            other => panic!("not a label: {other:?}"),
        }
    }

    #[test]
    fn a_count_shows_only_for_more_than_one() {
        let mut fixture = Fixture::new();
        fixture.update(holding(&[(0, 5, 64), (1, 6, 1)]));
        let [first, second, ..] = fixture.hud.slots;
        assert!(fixture.shown(first.count));
        assert_eq!(text(&fixture, first.count), "64");
        assert!(!fixture.shown(second.count), "one needs no number");

        fixture.update(state());
        assert!(!fixture.shown(first.count), "and an empty slot has none");
    }

    #[test]
    fn a_slots_icon_sits_in_its_middle() {
        let fixture = Fixture::new();
        let hotbar = fixture.rect(fixture.hud.hotbar);
        let icon = fixture.rect(fixture.hud.slots[0].icon);
        assert_eq!(icon.size(), [16.0, 16.0]);
        assert_eq!(icon.x, hotbar.x + 1.0 + 2.0, "a cap, and 2 either side in a slot of 20");
        assert_eq!(icon.y, hotbar.y + 3.0, "3 above and below in a slot of 22");
    }

    #[test]
    fn the_held_items_name_shows_when_it_changes_and_then_goes() {
        let mut fixture = Fixture::new();
        fixture.hud.update(&mut fixture.tree, &fixture.icons, &fixture.lang, Some(&state()));
        assert!(!fixture.shown(fixture.hud.note), "an empty hand has no name");

        // Unknown to this fixture's icons, so it has no name to show, but the timing's the
        // same with one: check the rule on the note itself.
        fixture.hud.named = Some((1, 0));
        fixture.hud.name_until = 60;
        fixture.tree.set_visible(fixture.hud.note, true);
        let mut later = holding(&[(0, 1, 1)]);
        later.tick = 59;
        fixture.hud.update(&mut fixture.tree, &fixture.icons, &fixture.lang, Some(&later));
        assert!(fixture.shown(fixture.hud.note), "still up");
        later.tick = 60;
        fixture.hud.update(&mut fixture.tree, &fixture.icons, &fixture.lang, Some(&later));
        assert!(!fixture.shown(fixture.hud.note), "and down");
    }

    #[test]
    fn a_note_outranks_the_held_items_name() {
        let mut fixture = Fixture::new();
        fixture.hud.set_note(&mut fixture.tree, Some("oak_door"));
        let mut later = holding(&[(0, 1, 1)]);
        later.tick = 1000;
        fixture.hud.update(&mut fixture.tree, &fixture.icons, &fixture.lang, Some(&later));
        assert!(fixture.shown(fixture.hud.note));
        assert_eq!(text(&fixture, fixture.hud.note), "oak_door");
    }

    #[test]
    fn every_sprite_the_hud_uses_is_asked_for() {
        for path in HOTBAR_SLOTS.iter().chain(&[HOTBAR_START, HOTBAR_END, SELECTED, XP_EMPTY, XP_FULL, XP_TICK]) {
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
