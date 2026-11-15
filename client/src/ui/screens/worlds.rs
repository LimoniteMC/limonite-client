//! The world list: pick a world to play, or create, copy or delete one (m5-plan Decision 17,
//! Step 16).
//!
//! **Four pages on one screen,** of which one shows at a time:
//! - **the list,** the most recently played first, five rows at a time, with Play Selected
//!   World, Create New World, Copy World and Delete under it. A click selects a row, and a
//!   second click on it plays it; Up and Down move the selection, and Enter plays it;
//! - **creating:** the world's name, survival or creative (Decision 6) on a button that
//!   switches between them, as Java's create screen has it, and a seed, which may be left empty
//!   for a random one. Tab moves between the fields, Enter creates and Escape goes back;
//! - **deleting:** a question first, since the world leaves the list;
//! - **busy:** one line while a world opens or saves.
//!
//! **The screen only decides what was asked for.** The files are `worlds.rs`'s, and opening a
//! world is `app.rs`'s: they get a [`Choice`], and show the list again with what came of it.
//!
//! The pictures are the vanilla UI's light buttons and edit boxes, on its plain dialog panel;
//! the strings are the install's own, and the few it lacks the client's (`limonite.lang`).
//! Bedrock's light button turns green both under the pointer and when pressed, so a row shows
//! only whether it's selected: a hovered row would look selected too.

use std::time::{Duration, Instant};

use pumpkin_util::GameMode;

use crate::{
    ui::{
        atlas::Atlas,
        button::{Button, ButtonSprites, LIT_TEXT, Look, TEXT},
        font::Font,
        lang::Lang,
        layout::{Anchor, Layout},
        text_field::{Edit, FieldSprites, TextField},
        tree::{Node, NodeId, Tree},
    },
    worlds::{self, MAX_NAME, WorldEntry},
};

const BACKGROUND: &str = "textures/ui/screen_background";
const PANEL: &str = "textures/ui/dialog_background_opaque";

/// The pictures the screen asks for, besides its buttons' and fields'.
pub const SPRITES: [&str; 2] = [BACKGROUND, PANEL];

/// How far down the title is, and the message's lines under it, at most two.
const TITLE_Y: f32 = 6.0;
const MESSAGE_Y: f32 = 17.0;
const MESSAGE_LINES: usize = 2;
const LINE: f32 = 9.0;
/// How much of the background's black shows: the inventory's 75%, over the sky's colour.
const DIM: u8 = 191;
/// The list: how many rows show, and how big each is.
const ROWS: usize = 5;
const ROW: [f32; 2] = [292.0, 22.0];
/// The panel round the rows, how far down it is, and how far in the rows sit.
const LIST: [f32; 2] = [300.0, 118.0];
const LIST_Y: f32 = 37.0;
const LIST_IN: f32 = 4.0;
/// The buttons: two to a line.
const BUTTON: [f32; 2] = [148.0, 20.0];
const SMALL_BUTTON: [f32; 2] = [98.0, 20.0];
const GAP: f32 = 4.0;
/// A text field.
const FIELD: [f32; 2] = [200.0, 20.0];
/// The longest seed to type, in characters.
const MAX_SEED: usize = 32;
/// A second click on a row within this plays it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// A row's second line, fainter than its first, on the light button and on the green.
const DETAIL: [u8; 4] = [110, 110, 110, 255];
const LIT_DETAIL: [u8; 4] = [220, 220, 220, 255];
/// A hint's colour, on the dark background.
const HINT: [u8; 4] = [170, 170, 170, 255];
/// An error's colour, `§c`, and a note's, white.
const ERROR: [u8; 4] = [255, 85, 85, 255];
const NOTE: [u8; 4] = [255, 255, 255, 255];

/// What the player asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Play the world with this id.
    Play(String),
    /// Make a world, and play it.
    Create { name: String, gamemode: GameMode, seed: String },
    /// Copy the world with this id into `backups/`.
    Copy(String),
    /// Move the world with this id into `deleted/`. The player has said yes.
    Delete(String),
}

/// A line over the list: what came of the last thing asked, or how the last world ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

/// A key the screen answers to, besides typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    /// Tab, or Shift and Tab.
    Tab,
    Up,
    Down,
    /// Keys that move a field's caret or take characters from it.
    Edit(EditKey),
}

/// The keys that edit a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
}

impl EditKey {
    #[must_use]
    pub const fn edit(self) -> Edit {
        match self {
            Self::Backspace => Edit::Backspace,
            Self::Delete => Edit::Delete,
            Self::Left => Edit::Left,
            Self::Right => Edit::Right,
            Self::Home => Edit::Home,
            Self::End => Edit::End,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    List,
    Create,
    Confirm,
    Busy,
}

/// Which field has the keys on the create page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name,
    Seed,
}

/// A row's nodes.
struct Row {
    button: Button,
    name: NodeId,
    detail: NodeId,
}

/// A world as a row shows it.
struct Shown {
    id: String,
    name: String,
    detail: String,
}

/// The screen's nodes, and its own state.
pub struct WorldsScreen {
    root: NodeId,
    pages: [NodeId; 4],
    message: [NodeId; MESSAGE_LINES],
    rows: Vec<Row>,
    empty: NodeId,
    play: Button,
    create: Button,
    copy: Button,
    delete: Button,
    name: TextField,
    seed: TextField,
    gamemode_button: Button,
    create_ok: Button,
    create_cancel: Button,
    confirm_name: NodeId,
    confirm_delete: Button,
    confirm_cancel: Button,
    busy: NodeId,
    buttons: ButtonSprites,
    fields: FieldSprites,
    page: Page,
    worlds: Vec<Shown>,
    selected: Option<usize>,
    /// The first world that shows.
    scroll: usize,
    /// Wheel motion short of a whole notch.
    wheel: f64,
    gamemode: GameMode,
    focus: Field,
    /// What the gamemode button says before the mode: "Game Mode".
    gamemode_label: String,
    /// The modes' names on it.
    mode_names: [String; 2],
    /// The row clicked last, and when: a second click soon after plays it.
    last_click: Option<(usize, Instant)>,
    hovered: Option<NodeId>,
    /// The world the confirmation is about.
    doomed: Option<String>,
}

/// A node of this size, with its top middle this far down its parent.
fn centred(y: f32, size: [f32; 2]) -> Layout {
    Layout::px(size[0], size[1]).at(Anchor::TopMiddle).moved(0.0, y)
}

/// A node of this size, with its top left here in its parent.
fn placed(at: [f32; 2], size: [f32; 2]) -> Layout {
    Layout::px(size[0], size[1]).at(Anchor::TopLeft).moved(at[0], at[1])
}

/// A line of text with its top middle this far down.
fn line(y: f32) -> Layout {
    Layout::content().at(Anchor::TopMiddle).moved(0.0, y)
}

/// Two buttons side by side, each `size`, their pair centred this far down.
fn pair(tree: &mut Tree, parent: NodeId, y: f32, size: [f32; 2], texts: [&str; 2], sprites: &ButtonSprites) -> [Button; 2] {
    let half = (size[0] + GAP) / 2.0;
    let mut make = |x: f32, text: &str| {
        let layout = Layout::px(size[0], size[1]).at(Anchor::TopMiddle).moved(x, y);
        Button::add(tree, parent, layout, text, sprites)
    };
    let left = make(-half, texts[0]);
    let right = make(half, texts[1]);
    [left, right]
}

impl WorldsScreen {
    /// Builds the screen, hidden.
    pub fn build(tree: &mut Tree, atlas: &Atlas, lang: &Lang) -> Self {
        let buttons = ButtonSprites::find(atlas);
        let fields = FieldSprites::find(atlas);
        let root = tree.add(Tree::ROOT, Node::panel(Layout::default()).hidden());
        tree.add(root, Node::image(Layout::default(), atlas.sprite(BACKGROUND)).faded(DIM));
        let page = |tree: &mut Tree| tree.add(root, Node::panel(Layout::default()).hidden());
        let pages = [page(tree), page(tree), page(tree), page(tree)];
        let [list_page, create_page, confirm_page, busy_page] = pages;

        // The list.
        tree.add(list_page, Node::label(line(TITLE_Y), lang.text("selectWorld.title")));
        let message = [0, 1].map(|at| tree.add(list_page, Node::label(line(MESSAGE_Y + at as f32 * LINE), "").hidden()));
        let panel = tree.add(list_page, Node::image(centred(LIST_Y, LIST), atlas.sprite(PANEL)));
        let rows = (0..ROWS)
            .map(|index| {
                let at = [LIST_IN, LIST_IN + index as f32 * ROW[1]];
                let button = Button::add(tree, panel, placed(at, [ROW[0], ROW[1] - 1.0]), "", &buttons);
                let name = button.add_line(tree, [6.0, 3.0], "", TEXT);
                let detail = button.add_line(tree, [6.0, 12.0], "", DETAIL);
                Row { button, name, detail }
            })
            .collect();
        let empty = tree.add(panel, Node::label(Layout::content(), lang.text("selectWorld.empty")).colored(DETAIL).unshadowed());
        let list_bottom = LIST_Y + LIST[1] + 6.0;
        let [play, create] = pair(tree, list_page, list_bottom, BUTTON, [lang.text("selectWorld.select"), lang.text("selectWorld.create")], &buttons);
        let [copy, delete] = pair(tree, list_page, list_bottom + BUTTON[1] + GAP, BUTTON, [lang.text("gui.copyWorld"), lang.text("selectWorld.delete")], &buttons);

        // Creating.
        tree.add(create_page, Node::label(line(TITLE_Y), lang.text("selectWorld.create")));
        tree.add(create_page, Node::label(line(28.0), lang.text("selectWorld.enterName")));
        let mut name = TextField::add(tree, create_page, centred(38.0, FIELD), MAX_NAME, &fields, true);
        name.set(lang.text("createWorldScreen.defaultName"), Instant::now());
        let gamemode_button = Button::add(tree, create_page, centred(68.0, FIELD), "", &buttons);
        tree.add(create_page, Node::label(line(98.0), lang.text("selectWorld.enterSeed")));
        let seed = TextField::add(tree, create_page, centred(108.0, FIELD), MAX_SEED, &fields, true);
        tree.add(create_page, Node::label(line(131.0), lang.text("selectWorld.seedInfo")).colored(HINT));
        let [create_ok, create_cancel] = pair(
            tree,
            create_page,
            152.0,
            SMALL_BUTTON,
            [lang.text("createWorldScreen.create"), lang.text("gui.cancel")],
            &buttons,
        );

        // Deleting.
        tree.add(confirm_page, Node::label(line(70.0), lang.text("selectWorld.deleteQuestion")));
        let confirm_name = tree.add(confirm_page, Node::label(line(84.0), ""));
        tree.add(confirm_page, Node::label(line(98.0), lang.text("limonite.deleteMoves")).colored(HINT));
        let [confirm_delete, confirm_cancel] = pair(
            tree,
            confirm_page,
            118.0,
            SMALL_BUTTON,
            [lang.text("selectWorld.deleteButton"), lang.text("gui.cancel")],
            &buttons,
        );

        // Busy.
        let busy = tree.add(busy_page, Node::label(Layout::content(), ""));

        Self {
            root,
            pages,
            message,
            rows,
            empty,
            play,
            create,
            copy,
            delete,
            name,
            seed,
            gamemode_button,
            create_ok,
            create_cancel,
            confirm_name,
            confirm_delete,
            confirm_cancel,
            busy,
            buttons,
            fields,
            page: Page::List,
            worlds: Vec::new(),
            selected: None,
            scroll: 0,
            wheel: 0.0,
            gamemode: GameMode::Creative,
            focus: Field::Name,
            gamemode_label: lang.text("selectWorld.gameMode").to_string(),
            mode_names: [
                lang.text("selectWorld.gameMode.survival").to_string(),
                lang.text("selectWorld.gameMode.creative").to_string(),
            ],
            last_click: None,
            hovered: None,
            doomed: None,
        }
    }

    fn show_page(&mut self, tree: &mut Tree, page: Page) {
        self.page = page;
        tree.set_visible(self.root, true);
        let index = match page {
            Page::List => 0,
            Page::Create => 1,
            Page::Confirm => 2,
            Page::Busy => 3,
        };
        for (at, &node) in self.pages.iter().enumerate() {
            tree.set_visible(node, at == index);
        }
    }

    /// Shows the list of `worlds`, with `message` over it, or none, wrapped to the list's width.
    /// The selection stays on the world it was on, if it's still there, and is otherwise the
    /// first world.
    pub fn show_list(&mut self, tree: &mut Tree, lang: &Lang, font: &Font, worlds: &[WorldEntry], message: Option<&Message>) {
        let selected_id = self.selected.and_then(|index| self.worlds.get(index)).map(|shown| shown.id.clone());
        self.worlds = worlds
            .iter()
            .map(|entry| {
                let mode = match entry.meta.gamemode {
                    GameMode::Creative => lang.text("gameMode.creative"),
                    _ => lang.text("gameMode.survival"),
                };
                let detail = match entry.when() {
                    Some(when) => format!("{mode} - {}", worlds::shown(when)),
                    None => mode.to_string(),
                };
                Shown { id: entry.id.clone(), name: entry.meta.name.clone(), detail }
            })
            .collect();
        self.selected = selected_id
            .and_then(|id| self.worlds.iter().position(|shown| shown.id == id))
            .or((!self.worlds.is_empty()).then_some(0));
        self.scroll = self.scroll.min(self.worlds.len().saturating_sub(ROWS));
        self.keep_selection_in_view();
        let lines = message.map(|message| font.wrap(&message.text, LIST[0] + 40.0)).unwrap_or_default();
        let color = if message.is_some_and(|message| message.error) { ERROR } else { NOTE };
        for (index, &node) in self.message.iter().enumerate() {
            let text = lines.get(index);
            tree.set_visible(node, text.is_some());
            tree.set_text(node, text.map_or("", String::as_str));
            tree.set_color(node, color);
        }
        self.doomed = None;
        self.show_page(tree, Page::List);
        self.show_rows(tree);
    }

    /// Shows one line, while something is under way.
    pub fn show_busy(&mut self, tree: &mut Tree, text: &str) {
        tree.set_text(self.busy, text);
        self.show_page(tree, Page::Busy);
    }

    /// Hides the screen, as a world opens.
    pub fn hide(&mut self, tree: &mut Tree) {
        tree.set_visible(self.root, false);
    }

    fn keep_selection_in_view(&mut self) {
        if let Some(selected) = self.selected {
            if selected < self.scroll {
                self.scroll = selected;
            } else if selected >= self.scroll + ROWS {
                self.scroll = selected + 1 - ROWS;
            }
        }
    }

    fn show_rows(&mut self, tree: &mut Tree) {
        for (index, row) in self.rows.iter().enumerate() {
            let shown = self.worlds.get(self.scroll + index);
            tree.set_visible(row.button.node, shown.is_some());
            if let Some(shown) = shown {
                tree.set_text(row.name, &shown.name);
                tree.set_text(row.detail, &shown.detail);
            }
        }
        tree.set_visible(self.empty, self.worlds.is_empty());
        let chosen = self.selected.is_some();
        self.play.set_enabled(chosen);
        self.copy.set_enabled(chosen);
        self.delete.set_enabled(chosen);
    }

    /// Brings the looks up to date: what's under the pointer, the chosen gamemode and row, and
    /// the fields' carets.
    pub fn update(&mut self, tree: &mut Tree, font: &Font, now: Instant) {
        let hovered = self.hovered;
        let look = |button: &Button, pressed: bool| Look {
            hovered: hovered == Some(button.node),
            pressed,
        };
        match self.page {
            Page::List => {
                for (index, row) in self.rows.iter().enumerate() {
                    let selected = self.selected == Some(self.scroll + index);
                    let only_selection = Look { hovered: false, pressed: selected };
                    let lit = row.button.show(tree, &self.buttons, only_selection);
                    tree.set_color(row.name, if lit { LIT_TEXT } else { TEXT });
                    tree.set_color(row.detail, if lit { LIT_DETAIL } else { DETAIL });
                }
                for button in [&self.play, &self.create, &self.copy, &self.delete] {
                    button.show(tree, &self.buttons, look(button, false));
                }
            }
            Page::Create => {
                let valid = worlds::is_valid_name(self.name.editor.text());
                self.create_ok.set_enabled(valid);
                let mode = &self.mode_names[usize::from(self.gamemode == GameMode::Creative)];
                self.gamemode_button.set_text(tree, &format!("{}: {mode}", self.gamemode_label));
                for button in [&self.gamemode_button, &self.create_ok, &self.create_cancel] {
                    button.show(tree, &self.buttons, look(button, false));
                }
                let name_hovered = hovered == Some(self.name.node);
                let seed_hovered = hovered == Some(self.seed.node);
                self.name.show(tree, font, &self.fields, name_hovered, self.focus == Field::Name, now);
                self.seed.show(tree, font, &self.fields, seed_hovered, self.focus == Field::Seed, now);
            }
            Page::Confirm => {
                for button in [&self.confirm_delete, &self.confirm_cancel] {
                    button.show(tree, &self.buttons, look(button, false));
                }
            }
            Page::Busy => {}
        }
    }

    /// The pointer moved over `node`, or nothing clickable.
    pub fn pointer_over(&mut self, node: Option<NodeId>) {
        self.hovered = node;
    }

    /// Something was clicked: a press and a release on the same node.
    pub fn clicked(&mut self, tree: &mut Tree, lang: &Lang, node: NodeId, now: Instant) -> Option<Choice> {
        match self.page {
            Page::List => {
                if let Some(index) = self.rows.iter().position(|row| row.button.node == node) {
                    let world = self.scroll + index;
                    let again = self
                        .last_click
                        .is_some_and(|(last, at)| last == world && now.saturating_duration_since(at) <= DOUBLE_CLICK);
                    self.selected = Some(world);
                    self.last_click = Some((world, now));
                    self.show_rows(tree);
                    return again.then(|| self.chosen_id()).flatten().map(Choice::Play);
                }
                if node == self.play.node && self.play.enabled() {
                    return self.chosen_id().map(Choice::Play);
                }
                if node == self.create.node {
                    self.open_create(tree, lang, now);
                }
                if node == self.copy.node && self.copy.enabled() {
                    return self.chosen_id().map(Choice::Copy);
                }
                if node == self.delete.node && self.delete.enabled() {
                    self.open_confirm(tree);
                }
                None
            }
            Page::Create => {
                if node == self.name.node {
                    self.focus = Field::Name;
                } else if node == self.seed.node {
                    self.focus = Field::Seed;
                } else if node == self.gamemode_button.node {
                    self.gamemode = if self.gamemode == GameMode::Creative { GameMode::Survival } else { GameMode::Creative };
                } else if node == self.create_ok.node && self.create_ok.enabled() {
                    return self.create_choice();
                } else if node == self.create_cancel.node {
                    self.show_page(tree, Page::List);
                }
                None
            }
            Page::Confirm => {
                if node == self.confirm_delete.node {
                    return self.doomed.take().map(Choice::Delete);
                }
                if node == self.confirm_cancel.node {
                    self.doomed = None;
                    self.show_page(tree, Page::List);
                }
                None
            }
            Page::Busy => None,
        }
    }

    /// The selected world's id.
    fn chosen_id(&self) -> Option<String> {
        self.selected.and_then(|index| self.worlds.get(index)).map(|shown| shown.id.clone())
    }

    /// The create page, fresh: the default name, the settings' gamemode, and no seed.
    fn open_create(&mut self, tree: &mut Tree, lang: &Lang, now: Instant) {
        self.name.set(lang.text("createWorldScreen.defaultName"), now);
        self.seed.set("", now);
        self.focus = Field::Name;
        self.show_page(tree, Page::Create);
    }

    /// Sets the gamemode the create page offers first, from the settings.
    pub fn set_default_gamemode(&mut self, gamemode: GameMode) {
        self.gamemode = if gamemode == GameMode::Survival { GameMode::Survival } else { GameMode::Creative };
    }

    fn open_confirm(&mut self, tree: &mut Tree) {
        let Some(shown) = self.selected.and_then(|index| self.worlds.get(index)) else {
            return;
        };
        tree.set_text(self.confirm_name, &format!("\"{}\"", shown.name));
        self.doomed = Some(shown.id.clone());
        self.show_page(tree, Page::Confirm);
    }

    fn create_choice(&self) -> Option<Choice> {
        let name = self.name.editor.text();
        worlds::is_valid_name(name).then(|| Choice::Create {
            name: name.trim().to_string(),
            gamemode: self.gamemode,
            seed: self.seed.editor.text().trim().to_string(),
        })
    }

    /// A key went down.
    pub fn key(&mut self, tree: &mut Tree, key: Key, now: Instant) -> Option<Choice> {
        match (self.page, key) {
            (Page::List, Key::Up) => self.move_selection(tree, -1),
            (Page::List, Key::Down) => self.move_selection(tree, 1),
            (Page::List, Key::Enter) => return self.chosen_id().map(Choice::Play),
            (Page::Create, Key::Enter) => return self.create_choice(),
            (Page::Create, Key::Escape) => self.show_page(tree, Page::List),
            (Page::Create, Key::Tab) => {
                self.focus = if self.focus == Field::Name { Field::Seed } else { Field::Name };
            }
            (Page::Create, Key::Edit(edit)) => {
                self.field().apply(&edit.edit(), now);
            }
            (Page::Confirm, Key::Escape) => {
                self.doomed = None;
                self.show_page(tree, Page::List);
            }
            (Page::Confirm, Key::Enter) => return self.doomed.take().map(Choice::Delete),
            _ => {}
        }
        None
    }

    /// Text was typed.
    pub fn typed(&mut self, text: &str, now: Instant) {
        if self.page == Page::Create {
            self.field().apply(&Edit::Insert(text.to_string()), now);
        }
    }

    fn field(&mut self) -> &mut TextField {
        match self.focus {
            Field::Name => &mut self.name,
            Field::Seed => &mut self.seed,
        }
    }

    fn move_selection(&mut self, tree: &mut Tree, by: isize) {
        if self.worlds.is_empty() {
            return;
        }
        let last = self.worlds.len() - 1;
        let from = self.selected.unwrap_or(0);
        self.selected = Some(from.saturating_add_signed(by).min(last));
        self.keep_selection_in_view();
        self.show_rows(tree);
    }

    /// The wheel turned, by `notches`, + away from the player: the list scrolls a row a notch.
    pub fn scroll(&mut self, tree: &mut Tree, notches: f64) {
        if self.page != Page::List {
            return;
        }
        self.wheel += notches;
        let rows = self.wheel.trunc();
        self.wheel -= rows;
        let most = self.worlds.len().saturating_sub(ROWS);
        self.scroll = self.scroll.saturating_add_signed(-(rows as isize)).min(most);
        self.show_rows(tree);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        ui::{
            atlas::{Atlas, SpriteId},
            layout::{self, Measure},
        },
        worlds::WorldMeta,
    };

    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, text: &str) -> [f32; 2] {
            [text.chars().count() as f32 * 6.0, 8.0]
        }
    }

    fn entry(id: &str, last_played: i64) -> WorldEntry {
        WorldEntry {
            id: id.to_string(),
            dir: PathBuf::from(id),
            meta: WorldMeta {
                name: id.to_string(),
                gamemode: GameMode::Survival,
                seed: Some(1),
                created: Some(last_played),
                last_played: Some(last_played),
            },
        }
    }

    /// A screen showing `count` worlds, laid out on a 320 × 240 screen.
    fn screen(count: usize) -> (Tree, WorldsScreen, Lang) {
        let mut tree = Tree::new();
        let lang = Lang::default();
        let mut screen = WorldsScreen::build(&mut tree, &Atlas::pack(Vec::new()).0, &lang);
        let worlds: Vec<WorldEntry> = (0..count).map(|index| entry(&format!("w{index}"), 100 - index as i64)).collect();
        screen.show_list(&mut tree, &lang, &Font::default(), &worlds, None);
        layout::run(&mut tree, [320.0, 240.0], &Fake);
        (tree, screen, lang)
    }

    /// Clicks the middle of a node.
    fn click(tree: &mut Tree, screen: &mut WorldsScreen, lang: &Lang, node: NodeId, now: Instant) -> Option<Choice> {
        let rect = tree.node(node).rect;
        let at = [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0];
        assert_eq!(tree.hit(at), Some(node), "the node is where a click lands");
        screen.clicked(tree, lang, node, now)
    }

    #[test]
    fn a_click_selects_a_row_and_a_second_soon_after_plays_it() {
        let (mut tree, mut screen, lang) = screen(3);
        let now = Instant::now();
        let second = screen.rows[1].button.node;
        assert_eq!(click(&mut tree, &mut screen, &lang, second, now), None);
        assert_eq!(screen.selected, Some(1));
        let later = now + DOUBLE_CLICK * 2;
        assert_eq!(click(&mut tree, &mut screen, &lang, second, later), None, "too slow for a double click");
        let play = click(&mut tree, &mut screen, &lang, second, later + Duration::from_millis(100));
        assert_eq!(play, Some(Choice::Play("w1".to_string())));
    }

    #[test]
    fn the_buttons_ask_for_the_selected_world() {
        let (mut tree, mut screen, lang) = screen(2);
        let now = Instant::now();
        assert_eq!(screen.selected, Some(0), "the most recent is selected first");
        let play = screen.play.node;
        let copy = screen.copy.node;
        assert_eq!(click(&mut tree, &mut screen, &lang, play, now), Some(Choice::Play("w0".to_string())));
        assert_eq!(click(&mut tree, &mut screen, &lang, copy, now), Some(Choice::Copy("w0".to_string())));
    }

    #[test]
    fn delete_asks_first() {
        let (mut tree, mut screen, lang) = screen(2);
        let now = Instant::now();
        screen.key(&mut tree, Key::Down, now);
        let delete = screen.delete.node;
        assert_eq!(click(&mut tree, &mut screen, &lang, delete, now), None);
        assert_eq!(screen.page, Page::Confirm);
        layout::run(&mut tree, [320.0, 240.0], &Fake);
        let cancel = screen.confirm_cancel.node;
        assert_eq!(click(&mut tree, &mut screen, &lang, cancel, now), None);
        assert_eq!(screen.page, Page::List, "no is no");

        layout::run(&mut tree, [320.0, 240.0], &Fake);
        click(&mut tree, &mut screen, &lang, delete, now);
        layout::run(&mut tree, [320.0, 240.0], &Fake);
        let yes = screen.confirm_delete.node;
        assert_eq!(click(&mut tree, &mut screen, &lang, yes, now), Some(Choice::Delete("w1".to_string())));
    }

    #[test]
    fn nothing_to_select_locks_the_buttons_that_need_a_world() {
        let (mut tree, mut screen, lang) = screen(0);
        let now = Instant::now();
        assert_eq!(screen.selected, None);
        for node in [screen.play.node, screen.copy.node, screen.delete.node] {
            assert_eq!(screen.clicked(&mut tree, &lang, node, now), None);
        }
        assert_eq!(screen.page, Page::List);
        assert!(tree.node(screen.empty).visible);
    }

    #[test]
    fn creating_takes_a_name_a_gamemode_and_a_seed() {
        let (mut tree, mut screen, lang) = screen(1);
        let now = Instant::now();
        let create = screen.create.node;
        click(&mut tree, &mut screen, &lang, create, now);
        assert_eq!(screen.page, Page::Create);
        // The default name is the key, with no install's strings here.
        for _ in 0.."createWorldScreen.defaultName".len() {
            screen.key(&mut tree, Key::Edit(EditKey::Backspace), now);
        }
        assert_eq!(screen.key(&mut tree, Key::Enter, now), None, "an empty name can't be made");
        screen.typed("Island", now);
        screen.key(&mut tree, Key::Tab, now);
        screen.typed("  1234 ", now);
        layout::run(&mut tree, [320.0, 240.0], &Fake);
        let gamemode = screen.gamemode_button.node;
        assert_eq!(screen.gamemode, GameMode::Creative, "the settings' default, until set");
        click(&mut tree, &mut screen, &lang, gamemode, now);
        assert_eq!(screen.gamemode, GameMode::Survival, "a click switches it");
        let made = screen.key(&mut tree, Key::Enter, now);
        assert_eq!(
            made,
            Some(Choice::Create {
                name: "Island".to_string(),
                gamemode: GameMode::Survival,
                seed: "1234".to_string(),
            })
        );
    }

    #[test]
    fn the_list_scrolls_to_keep_the_selection_in_view() {
        let (mut tree, mut screen, _) = screen(8);
        let now = Instant::now();
        for _ in 0..6 {
            screen.key(&mut tree, Key::Down, now);
        }
        assert_eq!(screen.selected, Some(6));
        assert_eq!(screen.scroll, 2, "the sixth row moved the list on");
        screen.scroll(&mut tree, 1.0);
        assert_eq!(screen.scroll, 1, "the wheel moves it back a row");
        screen.scroll(&mut tree, -10.0);
        assert_eq!(screen.scroll, 3, "and no further than the last row");
        screen.key(&mut tree, Key::Down, now);
        screen.key(&mut tree, Key::Down, now);
        assert_eq!(screen.selected, Some(7), "the selection stops at the last world");
    }

    #[test]
    fn the_selection_follows_its_world_when_the_list_changes() {
        let (mut tree, mut screen, lang) = screen(3);
        let now = Instant::now();
        screen.key(&mut tree, Key::Down, now);
        let worlds = [entry("new", 200), entry("w0", 100), entry("w1", 99), entry("w2", 98)];
        let font = Font::default();
        screen.show_list(&mut tree, &lang, &font, &worlds, None);
        assert_eq!(screen.chosen_id().as_deref(), Some("w1"));
        screen.show_list(&mut tree, &lang, &font, &worlds[..1], None);
        assert_eq!(screen.chosen_id().as_deref(), Some("new"), "a world that went leaves the first selected");
    }

    #[test]
    fn a_long_message_wraps_onto_two_lines() {
        let (mut tree, mut screen, lang) = screen(1);
        let message = Message {
            text: "word ".repeat(40),
            error: true,
        };
        screen.show_list(&mut tree, &lang, &Font::default(), &[entry("w0", 1)], Some(&message));
        for node in screen.message {
            assert!(tree.node(node).visible);
        }
        screen.show_list(&mut tree, &lang, &Font::default(), &[entry("w0", 1)], None);
        assert!(screen.message.iter().all(|&node| !tree.node(node).visible));
    }
}
