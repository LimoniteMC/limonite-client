//! The pointer, while a screen is open (m5-plan Decision 14).
//!
//! **A click is a press and a release on the same thing** (worth teaching). A
//! button acts when the mouse comes up, and only if it comes up over the button it
//! went down on. That's what lets someone change their mind: press, slide off, and
//! let go.
//!
//! While playing, the cursor is captured and none of this runs: `input.rs` has the
//! game's own keys and the mouse. Keyboard focus arrives with the first text field,
//! Step 16's command line.

use super::tree::{NodeId, Tree};

/// Where the pointer is, and what it went down on.
#[derive(Debug, Default)]
pub struct Pointer {
    /// In UI units. Unknown until it first moves over the window.
    at: Option<[f32; 2]>,
    pressed: Option<NodeId>,
}

impl Pointer {
    pub const fn moved(&mut self, at: [f32; 2]) {
        self.at = Some(at);
    }

    /// The button went down. Says whether it landed on something: if not, the click
    /// isn't the UI's, and the game can have it.
    pub fn press(&mut self, tree: &Tree) -> bool {
        self.pressed = self.at.and_then(|at| tree.hit(at));
        self.pressed.is_some()
    }

    /// The button came up. Returns what was clicked, if anything was.
    pub fn release(&mut self, tree: &Tree) -> Option<NodeId> {
        let pressed = self.pressed.take()?;
        let over = self.at.and_then(|at| tree.hit(at))?;
        (over == pressed).then_some(pressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        atlas::SpriteId,
        layout::{self, Anchor, Layout, Measure},
        tree::Node,
    };

    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, _: &str) -> [f32; 2] {
            [0.0, 0.0]
        }
    }

    /// Two clickable boxes: one at the left, one 20 units on.
    fn tree() -> (Tree, NodeId, NodeId) {
        let mut tree = Tree::new();
        let button = |x| Node::panel(Layout::px(10.0, 10.0).at(Anchor::TopLeft).moved(x, 0.0)).clickable();
        let a = tree.add(Tree::ROOT, button(0.0));
        let b = tree.add(Tree::ROOT, button(20.0));
        layout::run(&mut tree, [100.0, 100.0], &Fake);
        (tree, a, b)
    }

    #[test]
    fn a_press_and_release_on_one_thing_clicks_it() {
        let (tree, a, _) = tree();
        let mut pointer = Pointer::default();
        pointer.moved([5.0, 5.0]);
        assert!(pointer.press(&tree));
        assert_eq!(pointer.release(&tree), Some(a));
        assert_eq!(pointer.release(&tree), None, "and that was the click");
    }

    #[test]
    fn sliding_off_before_letting_go_is_no_click() {
        let (tree, _, _) = tree();
        let mut pointer = Pointer::default();
        pointer.moved([5.0, 5.0]);
        pointer.press(&tree);
        pointer.moved([50.0, 50.0]);
        assert_eq!(pointer.release(&tree), None);

        // Nor onto something else.
        pointer.moved([5.0, 5.0]);
        pointer.press(&tree);
        pointer.moved([25.0, 5.0]);
        assert_eq!(pointer.release(&tree), None);
    }

    #[test]
    fn a_press_on_nothing_isnt_the_uis() {
        let (tree, _, _) = tree();
        let mut pointer = Pointer::default();
        assert!(!pointer.press(&tree), "the pointer hasn't been seen yet");
        pointer.moved([50.0, 50.0]);
        assert!(!pointer.press(&tree));
        // Sliding onto something afterwards doesn't make it a click.
        pointer.moved([5.0, 5.0]);
        assert_eq!(pointer.release(&tree), None);
    }
}
