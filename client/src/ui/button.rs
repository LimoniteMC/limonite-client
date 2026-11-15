//! Buttons: Bedrock's light button, which the menus are made of (m5-plan Decision 14, Step 16).
//!
//! **A button is two nodes:** its picture, which is clickable, and its label inside it. The
//! picture is a nine-slice, so one sprite fits any size, and it changes with the pointer: one
//! sprite at rest, one while the pointer is over it, one while it's pressed, and one when the
//! button can't be used. The sprites are the vanilla UI's own, from
//! `ui/ui_template_buttons.json`, read while this was written and not at runtime.
//!
//! **What a click does isn't the button's business.** The screen that made it knows which node
//! is which button, and the pointer says which node was clicked (`input.rs`): a press and a
//! release on the same one.

use super::{
    atlas::{Atlas, SpriteId},
    layout::{Anchor, Layout},
    tree::{Node, NodeId, Tree},
};

const DEFAULT: &str = "textures/ui/button_borderless_light";
const HOVER: &str = "textures/ui/button_borderless_lighthover";
const PRESSED: &str = "textures/ui/button_borderless_lightpressed";
const LOCKED: &str = "textures/ui/disabledButtonNoBorder";

/// The pictures buttons ask for.
pub const SPRITES: [&str; 4] = [DEFAULT, HOVER, PRESSED, LOCKED];

/// A light button's text, which has no shadow: `$light_button_default_text_color`, 0.3 grey,
/// and `$light_button_locked_text_color`, the same on the locked button's darker picture.
pub const TEXT: [u8; 4] = [77, 77, 77, 255];
/// Its text while the pointer is over it or it's pressed, on the green those pictures are:
/// `$light_button_hover_text_color` and `$light_button_pressed_text_color`, white.
pub const LIT_TEXT: [u8; 4] = [255, 255, 255, 255];

/// The four pictures, found once.
#[derive(Clone, Copy, Debug)]
pub struct ButtonSprites {
    default: Option<SpriteId>,
    hover: Option<SpriteId>,
    pressed: Option<SpriteId>,
    locked: Option<SpriteId>,
}

impl ButtonSprites {
    #[must_use]
    pub fn find(atlas: &Atlas) -> Self {
        Self {
            default: atlas.sprite(DEFAULT),
            hover: atlas.sprite(HOVER),
            pressed: atlas.sprite(PRESSED),
            locked: atlas.sprite(LOCKED),
        }
    }
}

/// How a button looks, besides its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// The pointer is over it.
    pub hovered: bool,
    /// It's held down, or it's the chosen one of a set, such as a selected row.
    pub pressed: bool,
}

/// A button's nodes.
#[derive(Clone, Copy, Debug)]
pub struct Button {
    /// The picture: the node a click lands on.
    pub node: NodeId,
    label: NodeId,
    enabled: bool,
}

impl Button {
    /// Adds a button to `parent`, laid out by `layout`, with `text` in its middle.
    pub fn add(tree: &mut Tree, parent: NodeId, layout: Layout, text: &str, sprites: &ButtonSprites) -> Self {
        let node = tree.add(parent, Node::image(layout, sprites.default).clickable());
        let label = tree.add(node, Node::label(Layout::content(), text).colored(TEXT).unshadowed());
        Self { node, label, enabled: true }
    }

    /// Adds a label of its own inside it, as a row of the world list has two lines.
    pub fn add_line(&self, tree: &mut Tree, at: [f32; 2], text: &str, color: [u8; 4]) -> NodeId {
        let layout = Layout::content().at(Anchor::TopLeft).moved(at[0], at[1]);
        tree.add(self.node, Node::label(layout, text).colored(color).unshadowed())
    }

    /// Whether it can be used. A locked button looks it, and its clicks are the screen's to
    /// ignore.
    pub const fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Shows it as the pointer has it. Says whether it's lit, green under white text, for the
    /// lines a screen adds to follow.
    pub fn show(&self, tree: &mut Tree, sprites: &ButtonSprites, look: Look) -> bool {
        let (sprite, lit) = if !self.enabled {
            (sprites.locked, false)
        } else if look.pressed {
            (sprites.pressed, true)
        } else if look.hovered {
            (sprites.hover, true)
        } else {
            (sprites.default, false)
        };
        tree.set_sprite(self.node, sprite);
        tree.set_color(self.label, if lit { LIT_TEXT } else { TEXT });
        lit
    }

    /// Changes the text in its middle.
    pub fn set_text(&self, tree: &mut Tree, text: &str) {
        tree.set_text(self.label, text);
    }
}
