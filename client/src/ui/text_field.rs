//! Text fields: a line of text to type into, with a caret (m5-plan Decision 14, Step 16).
//!
//! **Two halves** (worth teaching). [`Editor`] is the text and where the caret is, and the edits
//! keys make to them. It knows nothing of nodes or fonts, so its tests are plain strings.
//! [`TextField`] puts an editor on the screen: Bedrock's indented box, the text in it, and the
//! caret, placed after the characters before it by the font's own widths.
//!
//! **The caret counts characters, not bytes.** Rust's strings are UTF-8, where `é` is two bytes
//! and `€` three, and slicing one between the bytes of a character panics. So the caret is a
//! count of characters, and turned into a byte offset only where the string is cut.
//!
//! **Keyboard focus** is which field the keys type into: the screen keeps it, and gives a
//! field the focus by showing its caret. A field without it shows its text and nothing else.

use std::time::{Duration, Instant};

use super::{
    atlas::{Atlas, SpriteId},
    font::Font,
    layout::{Anchor, Layout},
    tree::{Node, NodeId, Tree},
};

const BOX: &str = "textures/ui/edit_box_indent";
const BOX_HOVER: &str = "textures/ui/edit_box_indent_hover";
/// What the caret is drawn with: plain white, stretched. (`textures/ui/white` is white in a
/// black frame.)
pub const WHITE: &str = "textures/ui/white_background";

/// The pictures text fields ask for.
pub const SPRITES: [&str; 3] = [BOX, BOX_HOVER, WHITE];

/// How long the caret shows, and then hides: Java's and Bedrock's blink.
const BLINK: Duration = Duration::from_millis(500);
/// How far the text sits in from the box's left, and the caret's size.
const INSET: f32 = 4.0;
const CARET: [f32; 2] = [1.0, 9.0];

/// An edit a key makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Typed text, which may be more than one character, such as a dead key's.
    Insert(String),
    /// The character before the caret goes, or after it.
    Backspace,
    Delete,
    /// The caret moves.
    Left,
    Right,
    Home,
    End,
}

/// A line of text and its caret.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Editor {
    text: String,
    /// In characters from the start, 0 to the text's length.
    caret: usize,
    /// The most characters it takes.
    max: usize,
}

impl Editor {
    #[must_use]
    pub const fn new(max: usize) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            max,
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text, with the caret at its end. What's past the most it takes is cut off.
    pub fn set(&mut self, text: &str) {
        self.text = text.chars().take(self.max).collect();
        self.caret = self.len();
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte where the `chars`-th character starts, or the end.
    fn byte(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(at, _)| at)
    }

    /// The text before the caret, which is how far along the caret is drawn.
    #[must_use]
    pub fn before_caret(&self) -> &str {
        &self.text[..self.byte(self.caret)]
    }

    /// Makes an edit. Says whether anything changed.
    pub fn apply(&mut self, edit: &Edit) -> bool {
        let before = (self.text.len(), self.caret);
        match edit {
            Edit::Insert(typed) => {
                // Control characters aren't text: Enter, Tab and Escape come as keys of their own.
                let room = self.max.saturating_sub(self.len());
                let typed: String = typed.chars().filter(|c| !c.is_control()).take(room).collect();
                let at = self.byte(self.caret);
                self.text.insert_str(at, &typed);
                self.caret += typed.chars().count();
                return !typed.is_empty();
            }
            Edit::Backspace if self.caret > 0 => {
                let at = self.byte(self.caret - 1);
                self.text.remove(at);
                self.caret -= 1;
            }
            Edit::Delete if self.caret < self.len() => {
                let at = self.byte(self.caret);
                self.text.remove(at);
            }
            Edit::Left => self.caret = self.caret.saturating_sub(1),
            Edit::Right => self.caret = (self.caret + 1).min(self.len()),
            Edit::Home => self.caret = 0,
            Edit::End => self.caret = self.len(),
            Edit::Backspace | Edit::Delete => {}
        }
        (self.text.len(), self.caret) != before
    }
}

/// The two pictures a field's box can have, found once.
#[derive(Clone, Copy, Debug)]
pub struct FieldSprites {
    plain: Option<SpriteId>,
    hover: Option<SpriteId>,
    white: Option<SpriteId>,
}

impl FieldSprites {
    #[must_use]
    pub fn find(atlas: &Atlas) -> Self {
        Self {
            plain: atlas.sprite(BOX),
            hover: atlas.sprite(BOX_HOVER),
            white: atlas.sprite(WHITE),
        }
    }
}

/// An editor on the screen.
#[derive(Debug)]
pub struct TextField {
    /// The box: the node a click lands on, to give the field the focus.
    pub node: NodeId,
    label: NodeId,
    caret: NodeId,
    /// Whether it draws Bedrock's box, or sits on a background of its screen's own.
    boxed: bool,
    pub editor: Editor,
    /// When the caret last moved, which restarts its blink so it shows while typing.
    moved: Instant,
}

impl TextField {
    /// Adds a field to `parent`, laid out by `layout`, taking at most `max` characters, in
    /// Bedrock's box if `boxed`.
    pub fn add(tree: &mut Tree, parent: NodeId, layout: Layout, max: usize, sprites: &FieldSprites, boxed: bool) -> Self {
        let picture = if boxed { sprites.plain } else { None };
        let node = tree.add(parent, Node::image(layout, picture).clickable());
        let text = Layout::content().at(Anchor::LeftMiddle).moved(INSET, 0.0);
        let label = tree.add(node, Node::label(text, ""));
        let caret = Layout::px(CARET[0], CARET[1]).at(Anchor::LeftMiddle).moved(INSET, 0.0);
        let caret = tree.add(node, Node::image(caret, sprites.white).hidden());
        Self {
            node,
            label,
            caret,
            boxed,
            editor: Editor::new(max),
            moved: Instant::now(),
        }
    }

    /// Makes an edit, and shows the caret at once, as typing does. Says whether anything
    /// changed.
    pub fn apply(&mut self, edit: &Edit, now: Instant) -> bool {
        self.moved = now;
        self.editor.apply(edit)
    }

    /// Replaces the text, with the caret at its end.
    pub fn set(&mut self, text: &str, now: Instant) {
        self.editor.set(text);
        self.moved = now;
    }

    /// Brings its nodes up to date: the text, the box under the pointer, and the caret,
    /// blinking, if it has the focus.
    pub fn show(&self, tree: &mut Tree, font: &Font, sprites: &FieldSprites, hovered: bool, focused: bool, now: Instant) {
        tree.set_text(self.label, self.editor.text());
        if self.boxed {
            tree.set_sprite(self.node, if hovered || focused { sprites.hover } else { sprites.plain });
        }
        let blinks = (now.saturating_duration_since(self.moved).as_millis() / BLINK.as_millis()) as u64;
        tree.set_visible(self.caret, focused && blinks.is_multiple_of(2));
        tree.set_offset(self.caret, INSET + font.width(self.editor.before_caret()), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Editor {
        let mut editor = Editor::new(10);
        editor.apply(&Edit::Insert(text.to_string()));
        editor
    }

    #[test]
    fn typing_goes_in_at_the_caret() {
        let mut editor = typed("held");
        assert_eq!((editor.text(), editor.before_caret()), ("held", "held"));
        editor.apply(&Edit::Left);
        editor.apply(&Edit::Left);
        editor.apply(&Edit::Insert("ll w".to_string()));
        assert_eq!(editor.text(), "hell wld");
        assert_eq!(editor.before_caret(), "hell w");
    }

    #[test]
    fn backspace_and_delete_take_the_character_either_side() {
        let mut editor = typed("abc");
        editor.apply(&Edit::Left);
        assert!(editor.apply(&Edit::Backspace));
        assert_eq!((editor.text(), editor.before_caret()), ("ac", "a"));
        assert!(editor.apply(&Edit::Delete));
        assert_eq!(editor.text(), "a");
        assert!(!editor.apply(&Edit::Delete), "nothing after the caret");
        editor.apply(&Edit::Home);
        assert!(!editor.apply(&Edit::Backspace), "nothing before it");
        assert_eq!(editor.text(), "a");
    }

    #[test]
    fn the_caret_stays_inside_the_text() {
        let mut editor = typed("ab");
        assert!(!editor.apply(&Edit::Right), "already at the end");
        editor.apply(&Edit::Home);
        assert!(!editor.apply(&Edit::Left), "already at the start");
        assert_eq!(editor.before_caret(), "");
        editor.apply(&Edit::End);
        assert_eq!(editor.before_caret(), "ab");
    }

    #[test]
    fn the_caret_counts_characters_not_bytes() {
        // `é` is two bytes and `€` three: cutting the string between a character's bytes would
        // panic.
        let mut editor = typed("é€x");
        editor.apply(&Edit::Left);
        assert_eq!(editor.before_caret(), "é€");
        editor.apply(&Edit::Backspace);
        assert_eq!(editor.text(), "éx");
        editor.apply(&Edit::Left);
        editor.apply(&Edit::Delete);
        assert_eq!(editor.text(), "x");
    }

    #[test]
    fn a_full_field_takes_no_more_and_control_characters_are_not_text() {
        let mut editor = typed("0123456789");
        assert!(!editor.apply(&Edit::Insert("a".to_string())));
        assert_eq!(editor.text(), "0123456789");
        let mut editor = typed("ab\r\n\tc");
        assert_eq!(editor.text(), "abc");
        editor.set("a much longer text than ten");
        assert_eq!(editor.text(), "a much lon");
        assert_eq!(editor.before_caret(), "a much lon", "set puts the caret at the end");
        assert!(!Editor::new(3).apply(&Edit::Insert("\u{8}".to_string())), "a typed backspace isn't text");
    }
}
