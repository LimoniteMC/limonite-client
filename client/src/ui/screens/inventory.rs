//! The inventory screen, in survival and in creative (m5-plan Decision 15 and Step 12).
//!
//! **Where everything goes** comes from the install's `ui/inventory_screen.json` and
//! `ui/ui_common.json`, read while this was written and not at runtime, as the HUD's was:
//! - **survival** is the 176 × 166 panel: the armour down the left from (7, 7), the
//!   player's picture beside it, the off hand at (79, 61), the 27 slots from (7, 86) and
//!   the hotbar at (7, 143). Every slot is 18 × 18, with its item 16 × 16 in the middle and
//!   the count at its bottom right. The crafting grid waits for milestone 5b, and the
//!   player's picture for entities (Step 15), so its frame is empty;
//! - **creative** is the creative layout's, 326 × 166: the four category tabs over its top
//!   left, each 25 wide and reaching 4 into it, the tab's name at the top right, the list,
//!   17 columns of 18 from (7, 26), with its scroll bar down the right, and the hotbar in a
//!   panel of its own that hangs from the bottom, 5 into it;
//! - **behind both**, the world is dimmed by `screen_background` at 75%.
//!
//! **The list scrolls by rows.** The UI can't clip a picture yet, so the list shows 7 whole
//! rows, where the official client shows part of an eighth, and the wheel moves it a row at
//! a time. A group with a name shows as one cell, and a click opens it (`creative.rs`).
//!
//! **A click acts when the button goes down**, as it does in the official client: the
//! capture's request goes with the press. That's unlike a button's, which waits for the
//! release (`input.rs`).
//!
//! **What the screen decides, and what it doesn't.** It knows what was clicked; what that
//! does to the slots is `inventory/requests.rs`'s, which the session sends. The tabs, the
//! scrolling and opening a group are the screen's own.

use std::collections::{HashMap, HashSet};

use pumpkin_util::GameMode;

use crate::{
    game::Game,
    inventory::{
        Item,
        creative::{Cell, CreativeList, Tab},
        items::ItemIcons,
        slots::{ARMOUR, SlotRef, Slots},
    },
    ui::{
        atlas::{Atlas, SpriteId},
        lang::Lang,
        layout::{Anchor, Arrange, Layout},
        tree::{Node, NodeId, Tree},
    },
};

const SCREEN_BACKGROUND: &str = "textures/ui/screen_background";
const PANEL: &str = "textures/ui/dialog_background_opaque";
const HOTBAR_PANEL: &str = "textures/ui/dialog_background_opaque_overlap_bottom";
const CELL: &str = "textures/ui/cell_image";
const HIGHLIGHT: &str = "textures/ui/highlight_slot";
const HOVER_BORDER: &str = "textures/ui/focus_border_white";
const EMPTY_ARMOUR: [&str; ARMOUR] = [
    "textures/ui/empty_armor_slot_helmet",
    "textures/ui/empty_armor_slot_chestplate",
    "textures/ui/empty_armor_slot_leggings",
    "textures/ui/empty_armor_slot_boots",
];
const EMPTY_OFFHAND: &str = "textures/ui/empty_armor_slot_shield";
const PREVIEW_BORDER: &str = "textures/ui/player_preview_border";
const PREVIEW: &str = "textures/ui/Black";
/// The tabs' pictures: the leftmost tab's, and the others'.
const TAB_BACK: [&str; 2] = ["textures/ui/TabTopBackLeftMost", "textures/ui/TabTopBack"];
const TAB_FRONT: [&str; 2] = ["textures/ui/TabTopFrontLeftMost", "textures/ui/TabTopFront"];
const TAB_ICONS: [&str; 4] = [
    "textures/ui/icon_recipe_construction",
    "textures/ui/icon_recipe_equipment",
    "textures/ui/icon_recipe_item",
    "textures/ui/icon_recipe_nature",
];
const TAB_NAMES: [&str; 4] = [
    "craftingScreen.tab.construction",
    "craftingScreen.tab.equipment",
    "craftingScreen.tab.items",
    "craftingScreen.tab.nature",
];
const LIST_BACKGROUND: &str = "textures/ui/recipe_book_pane_bg";
const SCROLL_TRACK: &str = "textures/ui/ScrollGutterWithBG";
const SCROLL_BOX: &str = "textures/ui/ScrollBox";
/// A cell of the list: an item on its own, a group shut and open, and an open group's item.
const LIST_ITEM: &str = "textures/ui/recipe_book_item_bg";
const GROUP_SHUT: &str = "textures/ui/recipe_book_light_button";
const GROUP_OPEN: &str = "textures/ui/recipe_book_dark_button_pressed";
const GROUP_ITEM: &str = "textures/ui/recipe_book_dark_button";
const EXPAND: &str = "textures/ui/recipe_book_expand_icon";
const COLLAPSE: &str = "textures/ui/recipe_book_collapse_icon";

/// The pictures the screen asks for.
pub const SPRITES: [&str; 33] = [
    SCREEN_BACKGROUND,
    PANEL,
    HOTBAR_PANEL,
    CELL,
    HIGHLIGHT,
    HOVER_BORDER,
    EMPTY_ARMOUR[0],
    EMPTY_ARMOUR[1],
    EMPTY_ARMOUR[2],
    EMPTY_ARMOUR[3],
    EMPTY_OFFHAND,
    PREVIEW_BORDER,
    PREVIEW,
    TAB_BACK[0],
    TAB_BACK[1],
    TAB_FRONT[0],
    TAB_FRONT[1],
    TAB_ICONS[0],
    TAB_ICONS[1],
    TAB_ICONS[2],
    TAB_ICONS[3],
    LIST_BACKGROUND,
    SCROLL_TRACK,
    SCROLL_BOX,
    LIST_ITEM,
    GROUP_SHUT,
    GROUP_OPEN,
    GROUP_ITEM,
    EXPAND,
    COLLAPSE,
    // Repeated by the survival and creative hotbars; the atlas keeps one of each.
    CELL,
    HIGHLIGHT,
    HOVER_BORDER,
];

/// A slot, and the item in it.
const CELL_SIZE: f32 = 18.0;
const ITEM_SIZE: f32 = 16.0;
const SURVIVAL: [f32; 2] = [176.0, 166.0];
const CREATIVE: [f32; 2] = [326.0, 166.0];
const ARMOUR_AT: [f32; 2] = [7.0, 7.0];
const PREVIEW_BORDER_AT: [f32; 4] = [25.0, 7.0, 54.0, 72.0];
const PREVIEW_AT: [f32; 4] = [26.0, 8.0, 52.0, 70.0];
const OFFHAND_AT: [f32; 2] = [79.0, 61.0];
const ROWS_AT: [f32; 2] = [7.0, 86.0];
const HOTBAR_AT: [f32; 2] = [7.0, 143.0];
/// A tab, from 23 above the panel to 4 into it, with its icon in the part above.
const TAB: [f32; 2] = [25.0, 27.0];
const TAB_UP: f32 = 23.0;
const TAB_ICON: f32 = 17.0;
/// The list: where it starts, how many cells across and down, and how tall its pane is.
const LIST_AT: [f32; 2] = [7.0, 26.0];
const COLUMNS: usize = 17;
const ROWS: usize = 7;
const PANE: [f32; 2] = [306.0, 133.0];
/// The scroll bar, down the right of the list.
const SCROLL_AT: [f32; 2] = [316.0, 26.0];
const SCROLL_WIDTH: f32 = 4.0;
/// The tab's name, at the top right.
const LABEL_AT: [f32; 2] = [-7.0, 8.0];
/// The creative hotbar's panel, and where the slots are in it.
const HOTBAR_PANEL_SIZE: [f32; 2] = [174.0, 28.0];
const HOTBAR_PANEL_INTO: f32 = 5.0;
const HOTBAR_IN: [f32; 2] = [6.0, 5.0];
/// A group's mark, at the bottom right of its cell.
const MARK: f32 = 7.0;
/// `$title_text_color`, 0.3 grey, which has no shadow.
const TITLE: [u8; 4] = [77, 77, 77, 255];
/// How much the world behind is dimmed, and how strong the highlight is: 0.75 and 0.8.
const DIM: u8 = 191;
const HIGHLIGHT_ALPHA: u8 = 204;

/// What the screen shows: the inventory, in which mode, and the creative list. `None` is
/// the screen shut.
pub struct View<'a> {
    pub creative: bool,
    pub slots: &'a Slots,
    pub list: &'a CreativeList,
}

impl<'a> View<'a> {
    /// What the game says, if the server has the inventory open.
    #[must_use]
    pub fn of(game: &'a Game) -> Option<Self> {
        let player = game.player()?;
        game.inventory().is_open().then(|| Self {
            creative: player.gamemode == GameMode::Creative,
            slots: game.inventory().slots(),
            list: game.creative(),
        })
    }
}

/// What a press on the screen did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    /// A slot, for `requests.rs` to decide about.
    Slot(SlotRef),
    /// The creative list: on an item, or anywhere if the cursor holds something.
    Creative(Option<usize>),
    /// Another tab, which the official client tells the server about.
    Tab(Tab),
    /// Something the screen dealt with itself, or nothing at all.
    Nothing,
}

/// What a clickable node is.
#[derive(Clone, Copy, Debug)]
enum Target {
    Slot(SlotRef),
    /// A cell of the list, by where it is on the screen, from the top left.
    List(usize),
    /// The list's pane, between cells.
    Pane,
    Tab(Tab),
}

/// A slot's nodes.
struct SlotCell {
    slot: SlotRef,
    icon: NodeId,
    count: NodeId,
    /// The picture an empty armour slot or off hand shows.
    empty: Option<NodeId>,
}

/// A cell of the list's nodes.
struct ListCell {
    node: NodeId,
    icon: NodeId,
    mark: NodeId,
}

/// A tab's nodes: behind the panel, and in front of it when it's the open one.
struct TabNodes {
    back: NodeId,
    front: NodeId,
}

/// The screen's nodes, kept to change them, and its own state.
pub struct InventoryScreen {
    root: NodeId,
    survival: NodeId,
    creative: NodeId,
    /// The survival slots and the creative hotbar.
    survival_cells: Vec<SlotCell>,
    creative_cells: Vec<SlotCell>,
    list: Vec<ListCell>,
    tabs: Vec<TabNodes>,
    label: NodeId,
    scroll_box: NodeId,
    /// What follows the pointer: the cursor's stack.
    cursor: NodeId,
    cursor_icon: NodeId,
    cursor_count: NodeId,
    targets: HashMap<NodeId, Target>,
    /// Each clickable cell's highlight and border, shown while the pointer is over it.
    highlights: HashMap<NodeId, [NodeId; 2]>,
    hovered: Option<NodeId>,
    sprites: Sprites,
    tab: Tab,
    /// The first row of the list that shows.
    scroll: usize,
    /// Wheel motion short of a whole notch.
    wheel: f64,
    open_groups: HashSet<usize>,
    /// The open tab's cells, worked out again when it changes.
    cells: Vec<Cell>,
    cells_for: Option<(Tab, usize)>,
    cursor_empty: bool,
    /// Whether it was showing at the last update.
    showing: bool,
}

/// The list's pictures, found once.
struct Sprites {
    item: Option<SpriteId>,
    group_shut: Option<SpriteId>,
    group_open: Option<SpriteId>,
    group_item: Option<SpriteId>,
    expand: Option<SpriteId>,
    collapse: Option<SpriteId>,
}

/// A node of this size, with its top left here in its parent.
fn placed(at: [f32; 2], size: [f32; 2]) -> Layout {
    Layout::px(size[0], size[1]).at(Anchor::TopLeft).moved(at[0], at[1])
}

impl InventoryScreen {
    /// Builds the screen, shut.
    pub fn build(tree: &mut Tree, atlas: &Atlas) -> Self {
        let root = tree.add(Tree::ROOT, Node::panel(Layout::default()).hidden());
        tree.add(root, Node::image(Layout::default(), atlas.sprite(SCREEN_BACKGROUND)).faded(DIM));
        let mut screen = Self {
            root,
            survival: root,
            creative: root,
            survival_cells: Vec::new(),
            creative_cells: Vec::new(),
            list: Vec::new(),
            tabs: Vec::new(),
            label: root,
            scroll_box: root,
            cursor: root,
            cursor_icon: root,
            cursor_count: root,
            targets: HashMap::new(),
            highlights: HashMap::new(),
            hovered: None,
            sprites: Sprites {
                item: atlas.sprite(LIST_ITEM),
                group_shut: atlas.sprite(GROUP_SHUT),
                group_open: atlas.sprite(GROUP_OPEN),
                group_item: atlas.sprite(GROUP_ITEM),
                expand: atlas.sprite(EXPAND),
                collapse: atlas.sprite(COLLAPSE),
            },
            tab: Tab::Construction,
            scroll: 0,
            wheel: 0.0,
            open_groups: HashSet::new(),
            cells: Vec::new(),
            cells_for: None,
            cursor_empty: true,
            showing: false,
        };
        screen.build_survival(tree, atlas);
        screen.build_creative(tree, atlas);

        // The cursor's stack, over everything, moved to the pointer.
        let cursor = tree.add(root, Node::panel(placed([0.0, 0.0], [ITEM_SIZE; 2])).hidden());
        screen.cursor_icon = tree.add(cursor, Node::icon(Layout::px(ITEM_SIZE, ITEM_SIZE)));
        let count = Layout::content().at(Anchor::BottomRight).moved(1.0, 2.0);
        screen.cursor_count = tree.add(cursor, Node::label(count, "").hidden());
        screen.cursor = cursor;
        screen
    }

    fn build_survival(&mut self, tree: &mut Tree, atlas: &Atlas) {
        let panel = tree.add(self.root, Node::image(Layout::px(SURVIVAL[0], SURVIVAL[1]), atlas.sprite(PANEL)));
        self.survival = panel;
        let [x, y, width, height] = PREVIEW_BORDER_AT;
        tree.add(panel, Node::image(placed([x, y], [width, height]), atlas.sprite(PREVIEW_BORDER)));
        let [x, y, width, height] = PREVIEW_AT;
        tree.add(panel, Node::image(placed([x, y], [width, height]), atlas.sprite(PREVIEW)));

        let column = |at: [f32; 2], rows: u32, columns: u32| {
            let size = [columns as f32 * CELL_SIZE, rows as f32 * CELL_SIZE];
            placed(at, size).arranged(Arrange::Grid {
                columns,
                cell: [CELL_SIZE; 2],
            })
        };
        let armour = tree.add(panel, Node::panel(column(ARMOUR_AT, 4, 1)));
        for (index, empty) in EMPTY_ARMOUR.iter().enumerate() {
            let cell = self.slot_cell(tree, atlas, armour, SlotRef::Armour(index as u8), Some(empty));
            self.survival_cells.push(cell);
        }
        let offhand = tree.add(panel, Node::panel(column(OFFHAND_AT, 1, 1)));
        let cell = self.slot_cell(tree, atlas, offhand, SlotRef::Offhand, Some(EMPTY_OFFHAND));
        self.survival_cells.push(cell);
        let rows = tree.add(panel, Node::panel(column(ROWS_AT, 3, 9)));
        for slot in 9..36 {
            let cell = self.slot_cell(tree, atlas, rows, SlotRef::Main(slot), None);
            self.survival_cells.push(cell);
        }
        let hotbar = tree.add(panel, Node::panel(column(HOTBAR_AT, 1, 9)));
        for slot in 0..9 {
            let cell = self.slot_cell(tree, atlas, hotbar, SlotRef::Main(slot), None);
            self.survival_cells.push(cell);
        }
    }

    fn build_creative(&mut self, tree: &mut Tree, atlas: &Atlas) {
        let screen = tree.add(self.root, Node::panel(Layout::px(CREATIVE[0], CREATIVE[1])).hidden());
        self.creative = screen;

        // The tabs behind the panel, then the hotbar's panel, which the panel overlaps.
        let tab_layout = |index: usize| placed([index as f32 * TAB[0], -TAB_UP], TAB);
        let icon_layout = placed([(TAB[0] - TAB_ICON) / 2.0, (TAB_UP - TAB_ICON) / 2.0], [TAB_ICON; 2]);
        let mut backs = Vec::new();
        for (index, tab) in Tab::ALL.iter().enumerate() {
            let picture = TAB_BACK[usize::from(index > 0)];
            let back = tree.add(screen, Node::image(tab_layout(index), atlas.sprite(picture)).clickable());
            tree.add(back, Node::image(icon_layout, atlas.sprite(TAB_ICONS[index])));
            self.targets.insert(back, Target::Tab(*tab));
            backs.push(back);
        }
        let hotbar_panel = Layout {
            anchor_from: Anchor::BottomMiddle,
            anchor_to: Anchor::TopMiddle,
            ..Layout::px(HOTBAR_PANEL_SIZE[0], HOTBAR_PANEL_SIZE[1])
        }
        .moved(0.0, -HOTBAR_PANEL_INTO);
        let hotbar_panel = tree.add(screen, Node::image(hotbar_panel, atlas.sprite(HOTBAR_PANEL)));
        let hotbar = placed(HOTBAR_IN, [9.0 * CELL_SIZE, CELL_SIZE]).arranged(Arrange::Grid {
            columns: 9,
            cell: [CELL_SIZE; 2],
        });
        let hotbar = tree.add(hotbar_panel, Node::panel(hotbar));
        for slot in 0..9 {
            let cell = self.slot_cell(tree, atlas, hotbar, SlotRef::Main(slot), None);
            self.creative_cells.push(cell);
        }

        tree.add(screen, Node::image(Layout::default(), atlas.sprite(PANEL)));
        // The open tab, over the panel's edge.
        for (index, back) in backs.into_iter().enumerate() {
            let picture = TAB_FRONT[usize::from(index > 0)];
            let front = tree.add(screen, Node::image(tab_layout(index), atlas.sprite(picture)).hidden());
            tree.add(front, Node::image(icon_layout, atlas.sprite(TAB_ICONS[index])));
            self.tabs.push(TabNodes { back, front });
        }
        let label = Layout::content().at(Anchor::TopRight).moved(LABEL_AT[0], LABEL_AT[1]);
        self.label = tree.add(screen, Node::label(label, "").colored(TITLE).unshadowed());

        let pane = tree.add(screen, Node::image(placed(LIST_AT, PANE), atlas.sprite(LIST_BACKGROUND)).clickable());
        self.targets.insert(pane, Target::Pane);
        let grid = Layout::px(PANE[0], PANE[1]).at(Anchor::TopLeft).arranged(Arrange::Grid {
            columns: COLUMNS as u32,
            cell: [CELL_SIZE; 2],
        });
        let grid = tree.add(pane, Node::panel(grid));
        for index in 0..COLUMNS * ROWS {
            let node = tree.add(grid, Node::image(Layout::px(CELL_SIZE, CELL_SIZE), None).clickable());
            self.add_highlight(tree, atlas, node);
            let icon = tree.add(node, Node::icon(Layout::px(ITEM_SIZE, ITEM_SIZE)));
            let mark = Layout::px(MARK, MARK).at(Anchor::BottomRight).moved(-1.0, -1.0);
            let mark = tree.add(node, Node::image(mark, None));
            self.targets.insert(node, Target::List(index));
            self.list.push(ListCell { node, icon, mark });
        }
        let track = tree.add(
            screen,
            Node::image(placed(SCROLL_AT, [SCROLL_WIDTH, PANE[1]]), atlas.sprite(SCROLL_TRACK)),
        );
        self.scroll_box = tree.add(track, Node::image(placed([0.0, 0.0], [SCROLL_WIDTH, PANE[1]]), atlas.sprite(SCROLL_BOX)));
    }

    /// A slot: its picture, what an empty one shows, the highlight for when the pointer
    /// is over it, and the item and its count, in the order the JSON's layers put them:
    /// the highlight goes under the item.
    fn slot_cell(&mut self, tree: &mut Tree, atlas: &Atlas, parent: NodeId, slot: SlotRef, empty: Option<&str>) -> SlotCell {
        let node = tree.add(parent, Node::image(Layout::px(CELL_SIZE, CELL_SIZE), atlas.sprite(CELL)).clickable());
        let empty = empty.map(|path| tree.add(node, Node::image(Layout::px(ITEM_SIZE, ITEM_SIZE), atlas.sprite(path))));
        self.add_highlight(tree, atlas, node);
        let icon = tree.add(node, Node::icon(Layout::px(ITEM_SIZE, ITEM_SIZE)));
        let count = Layout::content().at(Anchor::BottomRight).moved(0.0, 1.0);
        let count = tree.add(node, Node::label(count, "").hidden());
        self.targets.insert(node, Target::Slot(slot));
        SlotCell { slot, icon, count, empty }
    }

    fn add_highlight(&mut self, tree: &mut Tree, atlas: &Atlas, cell: NodeId) {
        let highlight = Node::image(Layout::px(ITEM_SIZE, ITEM_SIZE), atlas.sprite(HIGHLIGHT)).faded(HIGHLIGHT_ALPHA);
        let highlight = tree.add(cell, highlight.hidden());
        let border = tree.add(cell, Node::image(Layout::default(), atlas.sprite(HOVER_BORDER)).hidden());
        self.highlights.insert(cell, [highlight, border]);
    }

    /// Brings the screen up to date with the game, or shuts it.
    pub fn update(&mut self, tree: &mut Tree, icons: &ItemIcons, lang: &Lang, view: Option<&View<'_>>) {
        let Some(view) = view else {
            if std::mem::take(&mut self.showing) {
                tree.set_visible(self.root, false);
                self.hover(tree, None);
            }
            return;
        };
        self.showing = true;
        tree.set_visible(self.root, true);
        tree.set_visible(self.survival, !view.creative);
        tree.set_visible(self.creative, view.creative);
        let cells = if view.creative { &self.creative_cells } else { &self.survival_cells };
        for cell in cells {
            show_item(tree, icons, cell.icon, cell.count, view.slots.item(cell.slot));
            if let Some(empty) = cell.empty {
                tree.set_visible(empty, view.slots.item(cell.slot).is_none());
            }
        }
        if view.creative {
            self.show_list(tree, icons, lang, view.list);
        }
        let cursor = view.slots.item(SlotRef::Cursor);
        self.cursor_empty = cursor.is_none();
        tree.set_visible(self.cursor, cursor.is_some());
        show_item(tree, icons, self.cursor_icon, self.cursor_count, cursor);
    }

    /// The open tab, the list from the row it's scrolled to, and the scroll bar.
    fn show_list(&mut self, tree: &mut Tree, icons: &ItemIcons, lang: &Lang, list: &CreativeList) {
        if self.cells_for != Some((self.tab, list.len())) {
            self.cells = list.cells(self.tab, &self.open_groups);
            self.cells_for = Some((self.tab, list.len()));
            self.scroll = self.scroll.min(self.most_scroll());
        }
        for (tab, nodes) in Tab::ALL.iter().zip(&self.tabs) {
            tree.set_visible(nodes.back, *tab != self.tab);
            tree.set_visible(nodes.front, *tab == self.tab);
        }
        let index = Tab::ALL.iter().position(|tab| *tab == self.tab).unwrap_or(0);
        let name = lang.get(TAB_NAMES[index]).map_or_else(|| format!("{:?}", self.tab), str::to_string);
        tree.set_text(self.label, &name);

        let first = self.scroll * COLUMNS;
        for (offset, cell) in self.list.iter().enumerate() {
            let shown = self.cells.get(first + offset);
            tree.set_visible(cell.node, shown.is_some());
            let Some(shown) = shown else {
                continue;
            };
            let (background, item, mark) = match *shown {
                Cell::Item { entry, grouped } => {
                    let stack = &list.entry(entry).stack;
                    let background = if grouped { self.sprites.group_item } else { self.sprites.item };
                    (background, Some((stack.id, stack.aux_value.0)), None)
                }
                Cell::Group { group, open } => {
                    let (background, mark) = if open {
                        (self.sprites.group_open, self.sprites.collapse)
                    } else {
                        (self.sprites.group_shut, self.sprites.expand)
                    };
                    (background, list.group_icon(group), mark)
                }
            };
            tree.set_sprite(cell.node, background);
            tree.set_icon(cell.icon, item.and_then(|(id, aux)| icons.get(id, aux)));
            tree.set_sprite(cell.mark, mark);
        }

        // The box is as tall as the share of the rows that shows, and as far down as the
        // list is scrolled.
        let rows = self.cells.len().div_ceil(COLUMNS).max(1);
        let height = (PANE[1] * ROWS as f32 / rows as f32).min(PANE[1]);
        let most = self.most_scroll();
        let down = if most == 0 { 0.0 } else { (PANE[1] - height) * self.scroll as f32 / most as f32 };
        tree.set_size(self.scroll_box, SCROLL_WIDTH, height);
        tree.set_offset(self.scroll_box, 0.0, down);
    }

    /// How far down the list can scroll, in rows.
    fn most_scroll(&self) -> usize {
        self.cells.len().div_ceil(COLUMNS).saturating_sub(ROWS)
    }

    /// The pointer moved, to here in UI units: the stack on the cursor follows it, and
    /// the slot under it lights up.
    pub fn pointer_moved(&mut self, tree: &mut Tree, at: [f32; 2]) {
        tree.set_offset(self.cursor, at[0] - ITEM_SIZE / 2.0, at[1] - ITEM_SIZE / 2.0);
        let over = tree.hit(at).filter(|node| self.highlights.contains_key(node));
        self.hover(tree, over);
    }

    fn hover(&mut self, tree: &mut Tree, over: Option<NodeId>) {
        if over == self.hovered {
            return;
        }
        for (node, shown) in [(self.hovered, false), (over, true)] {
            if let Some(nodes) = node.and_then(|node| self.highlights.get(&node)) {
                for &highlight in nodes {
                    tree.set_visible(highlight, shown);
                }
            }
        }
        self.hovered = over;
    }

    /// A button went down at `at`, in UI units.
    pub fn press(&mut self, tree: &Tree, at: [f32; 2]) -> Pressed {
        if !self.showing {
            return Pressed::Nothing;
        }
        let Some(target) = tree.hit(at).and_then(|node| self.targets.get(&node).copied()) else {
            return Pressed::Nothing;
        };
        match target {
            Target::Slot(slot) => Pressed::Slot(slot),
            Target::Pane => Pressed::Creative(None),
            Target::Tab(tab) => {
                if tab == self.tab {
                    return Pressed::Nothing;
                }
                self.tab = tab;
                self.scroll = 0;
                self.cells_for = None;
                Pressed::Tab(tab)
            }
            Target::List(offset) => match self.cells.get(self.scroll * COLUMNS + offset).copied() {
                // Anything on the cursor goes, wherever on the list the click is.
                _ if !self.cursor_empty => Pressed::Creative(None),
                Some(Cell::Item { entry, .. }) => Pressed::Creative(Some(entry)),
                Some(Cell::Group { group, open }) => {
                    if open {
                        self.open_groups.remove(&group);
                    } else {
                        self.open_groups.insert(group);
                    }
                    self.cells_for = None;
                    Pressed::Nothing
                }
                None => Pressed::Nothing,
            },
        }
    }

    /// The wheel turned by `notches`, + away from the player: the list moves a row a
    /// notch, up towards its start.
    pub fn scroll(&mut self, notches: f64) {
        self.wheel += notches;
        while self.wheel.abs() >= 1.0 {
            let up = self.wheel > 0.0;
            self.wheel -= if up { 1.0 } else { -1.0 };
            self.scroll = if up { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(self.most_scroll()) };
        }
    }
}

/// Puts `item` on an icon and its count, which shows above 1.
fn show_item(tree: &mut Tree, icons: &ItemIcons, icon: NodeId, count: NodeId, item: Option<Item>) {
    tree.set_icon(icon, item.and_then(|item| icons.get(item.id, item.aux)));
    let shown = item.filter(|item| item.count > 1).map(|item| item.count.to_string());
    tree.set_visible(count, shown.is_some());
    tree.set_text(count, shown.as_deref().unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::{
        bedrock::{
            client::creative_content::{CreativeCategory, CreativeGroupInfoPayload, CreativeItemEntryPayload},
            client_codecs::CreativeContent,
            network_item::NetworkItemDescriptor,
        },
        codec::{var_int::VarInt, var_uint::VarUInt},
    };

    use super::*;
    use crate::ui::{
        layout::{self, Measure},
        tests::atlas,
    };

    /// 1280 × 720 at the automatic scale of 3.
    const SCREEN: [f32; 2] = [1280.0 / 3.0, 240.0];

    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, _: &str) -> [f32; 2] {
            [0.0, 0.0]
        }
    }

    /// A list of `loose` items on their own, then a group of three, all in Construction.
    fn list(loose: u32) -> CreativeList {
        let item = |id| NetworkItemDescriptor {
            id: VarInt(id),
            stack_size: 1,
            ..NetworkItemDescriptor::default()
        };
        let group = |name: &str| CreativeGroupInfoPayload {
            creative_category: CreativeCategory::Construction,
            name: name.to_string(),
            group_icon_item: NetworkItemDescriptor::default(),
        };
        let mut entries: Vec<CreativeItemEntryPayload> = (0..3)
            .map(|index| CreativeItemEntryPayload {
                id: VarUInt(index + 1),
                item: item(5),
                group_index: VarUInt(1),
            })
            .collect();
        entries.extend((0..loose).map(|index| CreativeItemEntryPayload {
            id: VarUInt(index + 4),
            item: item(3),
            group_index: VarUInt(0),
        }));
        CreativeList::new(&CreativeContent {
            groups: vec![group(""), group("itemGroup.name.planks")],
            entries,
        })
    }

    struct Setup {
        tree: Tree,
        screen: InventoryScreen,
        slots: Slots,
        list: CreativeList,
    }

    impl Setup {
        fn new(loose: u32) -> Self {
            let mut tree = Tree::new();
            let screen = InventoryScreen::build(&mut tree, &atlas());
            Self {
                tree,
                screen,
                slots: Slots::default(),
                list: list(loose),
            }
        }

        /// Shows the screen, and lays it out.
        fn show(&mut self, creative: bool) {
            let view = View {
                creative,
                slots: &self.slots,
                list: &self.list,
            };
            self.screen.update(&mut self.tree, &ItemIcons::default(), &Lang::default(), Some(&view));
            layout::run(&mut self.tree, SCREEN, &Fake);
        }

        fn press(&mut self, at: [f32; 2]) -> Pressed {
            self.screen.press(&self.tree, at)
        }
    }

    /// The middle of the slot at `at` in a panel of `size`, centred on the screen.
    fn on(size: [f32; 2], at: [f32; 2]) -> [f32; 2] {
        [(SCREEN[0] - size[0]) / 2.0 + at[0] + 9.0, (SCREEN[1] - size[1]) / 2.0 + at[1] + 9.0]
    }

    #[test]
    fn a_press_on_a_slot_names_it() {
        let mut setup = Setup::new(0);
        assert_eq!(setup.press(on(SURVIVAL, HOTBAR_AT)), Pressed::Nothing, "shut");
        setup.show(false);
        assert_eq!(setup.press(on(SURVIVAL, HOTBAR_AT)), Pressed::Slot(SlotRef::Main(0)));
        assert_eq!(setup.press(on(SURVIVAL, [HOTBAR_AT[0] + 8.0 * 18.0, HOTBAR_AT[1]])), Pressed::Slot(SlotRef::Main(8)));
        assert_eq!(setup.press(on(SURVIVAL, ROWS_AT)), Pressed::Slot(SlotRef::Main(9)));
        assert_eq!(setup.press(on(SURVIVAL, [ROWS_AT[0], ROWS_AT[1] + 36.0])), Pressed::Slot(SlotRef::Main(27)));
        assert_eq!(setup.press(on(SURVIVAL, ARMOUR_AT)), Pressed::Slot(SlotRef::Armour(0)));
        assert_eq!(setup.press(on(SURVIVAL, [ARMOUR_AT[0], ARMOUR_AT[1] + 54.0])), Pressed::Slot(SlotRef::Armour(3)));
        assert_eq!(setup.press(on(SURVIVAL, OFFHAND_AT)), Pressed::Slot(SlotRef::Offhand));
        assert_eq!(setup.press([1.0, 1.0]), Pressed::Nothing, "off the panel");
    }

    #[test]
    fn creative_shows_its_own_hotbar() {
        let mut setup = Setup::new(0);
        setup.show(true);
        // The hotbar's panel hangs from the bottom of the creative panel, 5 into it.
        let left = (SCREEN[0] - HOTBAR_PANEL_SIZE[0]) / 2.0 + HOTBAR_IN[0];
        let top = (SCREEN[1] + CREATIVE[1]) / 2.0 - HOTBAR_PANEL_INTO + HOTBAR_IN[1];
        assert_eq!(setup.press([left + 9.0, top + 9.0]), Pressed::Slot(SlotRef::Main(0)));
        // The survival panel's slots aren't there.
        assert_ne!(setup.press(on(SURVIVAL, ARMOUR_AT)), Pressed::Slot(SlotRef::Armour(0)));
    }

    #[test]
    fn a_group_opens_and_an_item_is_taken() {
        let mut setup = Setup::new(2);
        setup.show(true);
        let cell = |column: f32| on(CREATIVE, [LIST_AT[0] + column * 18.0, LIST_AT[1]]);
        // The group, shut, then the two loose items.
        assert_eq!(setup.press(cell(1.0)), Pressed::Creative(Some(3)));
        assert_eq!(setup.press(cell(0.0)), Pressed::Nothing, "the group opens");
        setup.show(true);
        assert_eq!(setup.press(cell(1.0)), Pressed::Creative(Some(0)), "its first item");
        assert_eq!(setup.press(cell(4.0)), Pressed::Creative(Some(3)), "after its three");
        // Past the end, and between cells: nothing to take.
        assert_eq!(setup.press(cell(10.0)), Pressed::Creative(None));

        // With something on the cursor, anywhere on the list throws it away.
        setup.slots.cursor.id = 3;
        setup.slots.cursor.stack_size = 1;
        setup.show(true);
        assert_eq!(setup.press(cell(1.0)), Pressed::Creative(None));
        assert_eq!(setup.press(cell(0.0)), Pressed::Creative(None), "and a group doesn't shut");
    }

    #[test]
    fn a_tab_changes_and_starts_at_the_top() {
        let mut setup = Setup::new(400);
        setup.show(true);
        setup.screen.scroll(-3.0);
        assert_eq!(setup.screen.scroll, 3);
        let tab = |index: f32| [(SCREEN[0] - CREATIVE[0]) / 2.0 + index * TAB[0] + 12.0, (SCREEN[1] - CREATIVE[1]) / 2.0 - 10.0];
        assert_eq!(setup.press(tab(0.0)), Pressed::Nothing, "already open");
        assert_eq!(setup.press(tab(3.0)), Pressed::Tab(Tab::Nature));
        assert_eq!(setup.screen.scroll, 0);
        setup.show(true);
        assert!(setup.screen.cells.is_empty(), "nature has nothing in this list");
    }

    #[test]
    fn the_list_scrolls_a_row_a_notch_and_no_further_than_its_end() {
        // The group and 400 items: 401 cells, 24 rows, of which 7 show.
        let mut setup = Setup::new(400);
        setup.show(true);
        setup.screen.scroll(1.0);
        assert_eq!(setup.screen.scroll, 0, "already at the top");
        setup.screen.scroll(-0.5);
        assert_eq!(setup.screen.scroll, 0, "half a notch");
        setup.screen.scroll(-0.5);
        assert_eq!(setup.screen.scroll, 1);
        setup.screen.scroll(-100.0);
        assert_eq!(setup.screen.scroll, 24 - 7);
        setup.show(true);
        // The last row shows the last items.
        let last = on(CREATIVE, [LIST_AT[0], LIST_AT[1] + 6.0 * 18.0]);
        assert_eq!(setup.press(last), Pressed::Creative(Some((23 * 17 - 1) as usize + 3)));
    }

    #[test]
    fn the_slot_under_the_pointer_lights_up() {
        let mut setup = Setup::new(0);
        setup.show(false);
        let slot = on(SURVIVAL, ROWS_AT);
        setup.screen.pointer_moved(&mut setup.tree, slot);
        let lit = |setup: &Setup| {
            setup.screen.highlights.values().filter(|[highlight, _]| setup.tree.node(*highlight).visible).count()
        };
        assert_eq!(lit(&setup), 1);
        setup.screen.pointer_moved(&mut setup.tree, [1.0, 1.0]);
        assert_eq!(lit(&setup), 0);
        // Shutting the screen puts it out.
        setup.screen.pointer_moved(&mut setup.tree, slot);
        setup.screen.update(&mut setup.tree, &ItemIcons::default(), &Lang::default(), None);
        assert_eq!(lit(&setup), 0);
    }
}
