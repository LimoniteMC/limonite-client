//! The pause menu, and the death screen: what shows over a world, and takes the mouse and the
//! keys while it does (m5-plan Decision 15, Step 16).
//!
//! **Both are a few buttons over the world, dimmed,** so they're one kind of screen, an
//! [`Overlay`], built twice:
//! - **pause** (Escape, or the window losing the focus): Resume Game, and Save & Quit, which
//!   goes back to the world list. Escape resumes too;
//! - **death:** "You died!" over the world tinted red, with Respawn, and Main menu, which here
//!   is the world list, as Save & Quit is.
//!
//! **A paused world goes on.** Pumpkin has no pause: its ticks don't stop, and the host keeps
//! sending its input, standing still, as it does with the inventory open.

use crate::ui::{
    atlas::Atlas,
    button::{Button, ButtonSprites, Look},
    lang::Lang,
    layout::{Anchor, Layout},
    screens::worlds::Key,
    text_field::WHITE,
    tree::{Node, NodeId, Tree},
};

const DIM: &str = "textures/ui/screen_background";

/// The pictures these screens ask for, besides their buttons'.
pub const SPRITES: [&str; 2] = [DIM, WHITE];

/// How much the world is dimmed behind the pause menu: half.
const PAUSE_DIM: u8 = 128;
/// The death screen's tint, Java's dark red at its strongest, about 0.63 opaque.
const DEATH_TINT: [u8; 4] = [128, 48, 48, 160];
/// A button, and the gap between two.
const BUTTON: [f32; 2] = [200.0, 20.0];
const GAP: f32 = 4.0;

/// What a pause or death screen's button asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayChoice {
    /// Back to the game, or to life.
    Resume,
    Respawn,
    /// Back to the world list: the host leaves and the server saves.
    Quit,
}

/// A screen of buttons over the world.
pub struct Overlay {
    root: NodeId,
    buttons: Vec<(Button, OverlayChoice)>,
    sprites: ButtonSprites,
    hovered: Option<NodeId>,
    /// What Escape and Enter do: `None` for nothing.
    escape: Option<OverlayChoice>,
    enter: Option<OverlayChoice>,
}

impl Overlay {
    /// The pause menu, hidden.
    pub fn pause(tree: &mut Tree, atlas: &Atlas, lang: &Lang) -> Self {
        let background = Node::image(Layout::default(), atlas.sprite(DIM)).faded(PAUSE_DIM);
        let buttons = [("menu.returnToGame", OverlayChoice::Resume), ("menu.quit", OverlayChoice::Quit)];
        let mut overlay = Self::build(tree, atlas, lang, background, "menu.game", &buttons);
        overlay.escape = Some(OverlayChoice::Resume);
        overlay
    }

    /// The death screen, hidden. Escape does nothing on it: the host has to choose.
    pub fn death(tree: &mut Tree, atlas: &Atlas, lang: &Lang) -> Self {
        let background = Node::image(Layout::default(), atlas.sprite(WHITE)).tinted(DEATH_TINT);
        let buttons = [("deathScreen.respawn", OverlayChoice::Respawn), ("deathScreen.quit", OverlayChoice::Quit)];
        let mut overlay = Self::build(tree, atlas, lang, background, "deathScreen.title", &buttons);
        overlay.enter = Some(OverlayChoice::Respawn);
        overlay
    }

    fn build(
        tree: &mut Tree,
        atlas: &Atlas,
        lang: &Lang,
        background: Node,
        title: &str,
        buttons: &[(&str, OverlayChoice)],
    ) -> Self {
        let sprites = ButtonSprites::find(atlas);
        let root = tree.add(Tree::ROOT, Node::panel(Layout::default()).hidden());
        tree.add(root, background);
        let title_at = Layout::content().at(Anchor::Center).moved(0.0, -36.0);
        tree.add(root, Node::label(title_at, lang.text(title)));
        let buttons = (0_u8..)
            .zip(buttons)
            .map(|(index, &(text, choice))| {
                let y = -12.0 + f32::from(index) * (BUTTON[1] + GAP);
                let layout = Layout::px(BUTTON[0], BUTTON[1]).at(Anchor::Center).moved(0.0, y);
                (Button::add(tree, root, layout, lang.text(text), &sprites), choice)
            })
            .collect();
        Self {
            root,
            buttons,
            sprites,
            hovered: None,
            escape: None,
            enter: None,
        }
    }

    pub fn set_visible(&mut self, tree: &mut Tree, visible: bool) {
        tree.set_visible(self.root, visible);
        if !visible {
            self.hovered = None;
        }
    }

    /// Shows the button under the pointer as such.
    pub fn update(&self, tree: &mut Tree) {
        for (button, _) in &self.buttons {
            let look = Look {
                hovered: self.hovered == Some(button.node),
                pressed: false,
            };
            button.show(tree, &self.sprites, look);
        }
    }

    pub fn pointer_over(&mut self, node: Option<NodeId>) {
        self.hovered = node;
    }

    /// What a click on `node` asked for, if it's one of the buttons.
    #[must_use]
    pub fn clicked(&self, node: NodeId) -> Option<OverlayChoice> {
        self.buttons.iter().find(|(button, _)| button.node == node).map(|&(_, choice)| choice)
    }

    /// What a key asked for.
    #[must_use]
    pub const fn key(&self, key: Key) -> Option<OverlayChoice> {
        match key {
            Key::Escape => self.escape,
            Key::Enter => self.enter,
            _ => None,
        }
    }

    /// Each button's node and what it does, for tests and sheets.
    #[cfg(test)]
    pub fn buttons(&self) -> std::collections::HashMap<OverlayChoice, NodeId> {
        self.buttons.iter().map(|(button, choice)| (*choice, button.node)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        atlas::SpriteId,
        layout::{self, Measure},
    };

    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, text: &str) -> [f32; 2] {
            [text.len() as f32 * 6.0, 8.0]
        }
    }

    #[test]
    fn pause_resumes_or_quits_and_escape_resumes() {
        let mut tree = Tree::new();
        let mut pause = Overlay::pause(&mut tree, &Atlas::pack(Vec::new()).0, &Lang::default());
        pause.set_visible(&mut tree, true);
        layout::run(&mut tree, [320.0, 240.0], &Fake);
        let buttons = pause.buttons();
        let resume = buttons[&OverlayChoice::Resume];
        let quit = buttons[&OverlayChoice::Quit];
        let middle = |node: NodeId| {
            let rect = tree.node(node).rect;
            [rect.x + rect.width / 2.0, rect.y + rect.height / 2.0]
        };
        assert_eq!(tree.hit(middle(resume)), Some(resume));
        assert_eq!(tree.hit(middle(quit)), Some(quit));
        assert_eq!(pause.clicked(quit), Some(OverlayChoice::Quit));
        assert_eq!(pause.key(Key::Escape), Some(OverlayChoice::Resume));
        assert_eq!(pause.key(Key::Enter), None);
    }

    #[test]
    fn death_offers_respawn_and_the_list_and_escape_does_nothing() {
        let mut tree = Tree::new();
        let death = Overlay::death(&mut tree, &Atlas::pack(Vec::new()).0, &Lang::default());
        let buttons = death.buttons();
        assert_eq!(death.clicked(buttons[&OverlayChoice::Respawn]), Some(OverlayChoice::Respawn));
        assert_eq!(death.clicked(buttons[&OverlayChoice::Quit]), Some(OverlayChoice::Quit));
        assert_eq!(death.key(Key::Escape), None, "the host has to choose");
        assert_eq!(death.key(Key::Enter), Some(OverlayChoice::Respawn));
    }
}
