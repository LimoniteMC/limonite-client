//! The retained widget tree (m5-plan Decision 14; worth teaching).
//!
//! **Retained, not immediate.** An immediate-mode UI, like `egui`, describes every
//! widget again every frame. Here a screen builds its tree once, keeps the ids of
//! what can change, and sets those when the game changes. The setters notice when
//! nothing really changed, so most frames no work happens at all: layout and the
//! quads are only redone when something is different. It also gives JSON UI
//! (milestone 10) a persistent tree to map onto.
//!
//! **An arena of nodes.** The nodes live in one `Vec`, and refer to each other by
//! index ([`NodeId`]) rather than by reference. A tree of references would have
//! the borrow checker fighting every edit, since a parent would borrow its children
//! while something else changes one. With ids, there's one owner, the `Tree`.
//!
//! Widgets arrive with the first screen that uses them: this step's HUD needs
//! panels, images and labels.

use super::{
    atlas::SpriteId,
    layout::{Layout, Offset, Rect},
};
use crate::render::icons::Icon;

/// A node of a [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

/// How a picture joins what's behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blend {
    /// As its alpha says.
    #[default]
    Alpha,
    /// It inverts what's behind it, so the crosshair shows against anything.
    Invert,
    /// As its alpha says, from the terrain's textures: a block icon.
    Block,
}

/// A picture, or nothing yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    pub sprite: Option<SpriteId>,
    pub blend: Blend,
}

/// A line of white text with a shadow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub text: String,
}

/// What a node draws, before its children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Nothing: it only places its children.
    Panel,
    Image(Image),
    Label(Label),
    /// What an item looks like, or nothing.
    Icon(Option<Icon>),
}

/// One widget.
#[derive(Clone, Debug)]
pub struct Node {
    pub layout: Layout,
    pub kind: Kind,
    /// A hidden node draws nothing, takes no room, and neither do its children.
    pub visible: bool,
    /// Whether the pointer can land on it. Everything else lets it through.
    pub clickable: bool,
    /// Where it is, from the last layout, in UI units.
    pub rect: Rect,
    children: Vec<NodeId>,
}

impl Node {
    fn new(layout: Layout, kind: Kind) -> Self {
        Self {
            layout,
            kind,
            visible: true,
            clickable: false,
            rect: Rect::default(),
            children: Vec::new(),
        }
    }

    #[must_use]
    pub fn panel(layout: Layout) -> Self {
        Self::new(layout, Kind::Panel)
    }

    #[must_use]
    pub fn image(layout: Layout, sprite: Option<SpriteId>) -> Self {
        let blend = Blend::Alpha;
        Self::new(layout, Kind::Image(Image { sprite, blend }))
    }

    #[must_use]
    pub fn label(layout: Layout, text: &str) -> Self {
        let text = text.to_string();
        Self::new(layout, Kind::Label(Label { text }))
    }

    #[must_use]
    pub fn icon(layout: Layout) -> Self {
        Self::new(layout, Kind::Icon(None))
    }

    /// Starts hidden.
    #[must_use]
    pub const fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    /// Inverts what's behind it, if it's a picture.
    #[must_use]
    pub const fn inverting(mut self) -> Self {
        if let Kind::Image(image) = &mut self.kind {
            image.blend = Blend::Invert;
        }
        self
    }

    /// Lets the pointer land on it. Nothing on the HUD needs it, so it comes alive
    /// with Step 12's inventory.
    #[cfg(test)]
    #[must_use]
    pub const fn clickable(mut self) -> Self {
        self.clickable = true;
        self
    }

    #[must_use]
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }
}

/// The widgets, and whether they've changed since they were last laid out and drawn.
#[derive(Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    changed: bool,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    /// The screen itself. It's always there, and every other node is inside it.
    pub const ROOT: NodeId = NodeId(0);

    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::panel(Layout::default())],
            changed: true,
        }
    }

    /// Adds `node` as the last child of `parent`, so it draws over those before it.
    pub fn add(&mut self, parent: NodeId, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        self.nodes[parent.0 as usize].children.push(id);
        self.changed = true;
        id
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    /// Shows or hides a node.
    pub fn set_visible(&mut self, id: NodeId, visible: bool) {
        let node = self.node_mut(id);
        if node.visible != visible {
            node.visible = visible;
            self.changed = true;
        }
    }

    /// Changes a picture.
    pub fn set_sprite(&mut self, id: NodeId, sprite: Option<SpriteId>) {
        let node = self.node_mut(id);
        if let Kind::Image(image) = &mut node.kind
            && image.sprite != sprite
        {
            image.sprite = sprite;
            self.changed = true;
        }
    }

    /// Changes an icon.
    pub fn set_icon(&mut self, id: NodeId, icon: Option<Icon>) {
        let node = self.node_mut(id);
        if let Kind::Icon(current) = &mut node.kind
            && *current != icon
        {
            *current = icon;
            self.changed = true;
        }
    }

    /// Changes a label's text.
    pub fn set_text(&mut self, id: NodeId, text: &str) {
        let node = self.node_mut(id);
        if let Kind::Label(label) = &mut node.kind
            && label.text != text
        {
            text.clone_into(&mut label.text);
            self.changed = true;
        }
    }

    /// Moves a node from its anchor, in UI units.
    pub fn set_offset(&mut self, id: NodeId, x: f32, y: f32) {
        let node = self.node_mut(id);
        let offset = [Offset::Px(x), Offset::Px(y)];
        if node.layout.offset != offset {
            node.layout.offset = offset;
            self.changed = true;
        }
    }

    /// Asks for layout again, as when the window changes size.
    pub const fn invalidate(&mut self) {
        self.changed = true;
    }

    /// Where layout puts a node.
    pub(super) fn set_rect(&mut self, id: NodeId, rect: Rect) {
        self.node_mut(id).rect = rect;
    }

    /// Whether anything has changed since the last time this was asked.
    pub const fn take_changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    /// The clickable node at `at`, the one drawn last if several overlap. A hidden
    /// node isn't there, and neither are its children.
    #[must_use]
    pub fn hit(&self, at: [f32; 2]) -> Option<NodeId> {
        self.hit_in(Self::ROOT, at)
    }

    fn hit_in(&self, id: NodeId, at: [f32; 2]) -> Option<NodeId> {
        let node = self.node(id);
        if !node.visible {
            return None;
        }
        // Children draw over their parent, and later children over earlier ones,
        // so that's the order to ask in, last first.
        node.children
            .iter()
            .rev()
            .find_map(|&child| self.hit_in(child, at))
            .or_else(|| (node.clickable && node.rect.contains(at)).then_some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::{self, Anchor, Measure};

    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, _: &str) -> [f32; 2] {
            [0.0, 0.0]
        }
    }

    fn settled(tree: &mut Tree) {
        layout::run(tree, [100.0, 100.0], &Fake);
        tree.take_changed();
    }

    /// A clickable box, with its top-left corner here.
    fn clickable(x: f32, y: f32) -> Node {
        Node::panel(Layout::px(10.0, 10.0).at(Anchor::TopLeft).moved(x, y)).clickable()
    }

    #[test]
    fn nothing_really_changing_changes_nothing() {
        let mut tree = Tree::new();
        let label = tree.add(Tree::ROOT, Node::label(Layout::content(), "same"));
        let image = tree.add(Tree::ROOT, Node::image(Layout::content(), None));
        settled(&mut tree);

        tree.set_text(label, "same");
        tree.set_sprite(image, None);
        tree.set_visible(label, true);
        tree.set_offset(label, 0.0, 0.0);
        assert!(!tree.take_changed());
    }

    #[test]
    fn a_real_change_is_noticed_once() {
        let mut tree = Tree::new();
        let label = tree.add(Tree::ROOT, Node::label(Layout::content(), "old"));
        settled(&mut tree);

        tree.set_text(label, "new");
        assert!(tree.take_changed());
        assert!(!tree.take_changed(), "and it's only said once");

        tree.set_visible(label, false);
        assert!(tree.take_changed());

        tree.set_offset(label, 1.0, 0.0);
        assert!(tree.take_changed());
    }

    #[test]
    fn the_pointer_lands_on_what_it_is_over() {
        let mut tree = Tree::new();
        let a = tree.add(Tree::ROOT, clickable(0.0, 0.0));
        let b = tree.add(Tree::ROOT, clickable(20.0, 0.0));
        settled(&mut tree);
        assert_eq!(tree.hit([5.0, 5.0]), Some(a));
        assert_eq!(tree.hit([25.0, 5.0]), Some(b));
        assert_eq!(tree.hit([15.0, 5.0]), None);
        assert_eq!(tree.hit([50.0, 50.0]), None);
    }

    #[test]
    fn an_edge_belongs_to_one_side() {
        let mut tree = Tree::new();
        let a = tree.add(Tree::ROOT, clickable(0.0, 0.0));
        let b = tree.add(Tree::ROOT, clickable(10.0, 0.0));
        settled(&mut tree);
        assert_eq!(tree.hit([0.0, 0.0]), Some(a));
        assert_eq!(tree.hit([10.0, 0.0]), Some(b), "its left edge is its own");
        assert_eq!(tree.hit([20.0, 0.0]), None, "its right edge isn't");
        assert_eq!(tree.hit([0.0, 10.0]), None, "nor its bottom");
    }

    #[test]
    fn what_draws_last_gets_the_pointer() {
        let mut tree = Tree::new();
        let under = tree.add(Tree::ROOT, clickable(0.0, 0.0));
        let over = tree.add(Tree::ROOT, clickable(5.0, 5.0));
        settled(&mut tree);
        assert_eq!(tree.hit([7.0, 7.0]), Some(over));
        assert_eq!(tree.hit([2.0, 2.0]), Some(under));
    }

    #[test]
    fn a_child_is_over_its_parent() {
        let mut tree = Tree::new();
        let parent = tree.add(Tree::ROOT, Node::panel(Layout::px(50.0, 50.0).at(Anchor::TopLeft)).clickable());
        let child = tree.add(parent, clickable(0.0, 0.0));
        settled(&mut tree);
        assert_eq!(tree.hit([5.0, 5.0]), Some(child));
        assert_eq!(tree.hit([30.0, 30.0]), Some(parent));
    }

    #[test]
    fn what_isnt_clickable_or_shown_lets_the_pointer_through() {
        let mut tree = Tree::new();
        let under = tree.add(Tree::ROOT, clickable(0.0, 0.0));
        tree.add(Tree::ROOT, Node::panel(Layout::px(10.0, 10.0).at(Anchor::TopLeft)));
        let hidden = tree.add(Tree::ROOT, clickable(0.0, 0.0).hidden());
        tree.add(hidden, clickable(0.0, 0.0));
        settled(&mut tree);
        assert_eq!(tree.hit([5.0, 5.0]), Some(under));
    }
}
