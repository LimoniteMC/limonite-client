//! Where things go: anchors, offsets and sizes, stack panels and grids (m5-plan
//! Decision 14; worth teaching).
//!
//! **The JSON UI way.** Bedrock's own UI describes a control by which point of
//! its parent (`anchor_from`) its own point (`anchor_to`) sits on, how far it then
//! moves (`offset`), and its size in pixels, percentages or whatever's left. So
//! "the hotbar sits at the bottom middle, a little up" is one line, and it stays
//! true at any window size. Laying out the same way means milestone 10's JSON UI
//! subset can map onto this tree without changing it.
//!
//! **UI units.** Everything here is in UI units, not window pixels: a sprite's
//! texel is one unit, and the UI scale says how many pixels a unit is. So layout
//! knows nothing of the window but how many units fit in it.
//!
//! **Two questions, in order.** How big is a node? A percentage asks its parent,
//! so sizes are worked out from the root down. But `Content`, as big as what's
//! inside, asks its children instead. That only works if they don't ask back, so
//! a child sized by its parent counts for nothing when its parent is sized by
//! its content.

use super::{
    atlas::SpriteId,
    tree::{Kind, NodeId, Tree},
};

/// A rectangle in UI units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    #[must_use]
    pub const fn size(self) -> [f32; 2] {
        [self.width, self.height]
    }

    /// Left, top, right and bottom.
    #[must_use]
    pub fn edges(self) -> [f32; 4] {
        [self.x, self.y, self.x + self.width, self.y + self.height]
    }

    /// Whether `at` is inside. A shared edge belongs to one side only: the left
    /// and top are in, and the right and bottom are out.
    #[must_use]
    pub fn contains(self, at: [f32; 2]) -> bool {
        let [left, top, right, bottom] = self.edges();
        (left..right).contains(&at[0]) && (top..bottom).contains(&at[1])
    }
}

/// A point of a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "the HUD uses five; the rest come with the menus"))]
pub enum Anchor {
    TopLeft,
    TopMiddle,
    TopRight,
    LeftMiddle,
    Center,
    RightMiddle,
    BottomLeft,
    BottomMiddle,
    BottomRight,
}

impl Anchor {
    /// How far along a rectangle this point is, from 0 to 1 each way.
    const fn fraction(self) -> [f32; 2] {
        let x = match self {
            Self::TopLeft | Self::LeftMiddle | Self::BottomLeft => 0.0,
            Self::TopMiddle | Self::Center | Self::BottomMiddle => 0.5,
            Self::TopRight | Self::RightMiddle | Self::BottomRight => 1.0,
        };
        let y = match self {
            Self::TopLeft | Self::TopMiddle | Self::TopRight => 0.0,
            Self::LeftMiddle | Self::Center | Self::RightMiddle => 0.5,
            Self::BottomLeft | Self::BottomMiddle | Self::BottomRight => 1.0,
        };
        [x, y]
    }
}

/// How big a node is, one axis at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// This many UI units.
    Px(f32),
    /// This share of the parent, from 0 to 100.
    Percent(f32),
    /// As much as there is: in a stack, a share of what the others leave along
    /// the stack, and otherwise the whole parent.
    Fill,
    /// As big as what's inside: its children as arranged, its picture or its text.
    Content,
}

/// How far a node moves from its anchor, one axis at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Offset {
    Px(f32),
    /// This share of the parent, from 0 to 100.
    #[cfg_attr(not(test), expect(dead_code, reason = "the first screen that uses it is Step 12's inventory"))]
    Percent(f32),
}

/// An axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    #[cfg_attr(not(test), expect(dead_code, reason = "the first column is Step 16's world list"))]
    Y,
}

impl Axis {
    const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
        }
    }
}

/// How a node places its children.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrange {
    /// Each by its own anchors, on top of one another.
    Free,
    /// One after another along an axis. Across it, each goes by its own anchors.
    Stack(Axis),
    /// Into cells of this size, left to right and then down. Inside its cell, each
    /// goes by its own anchors. A child can be larger than its cell, and overlap
    /// the next.
    Grid { columns: u32, cell: [f32; 2] },
}

/// Where a node goes and how big it is, and how it places its children. The
/// default is JSON UI's: the whole of its parent, centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub anchor_from: Anchor,
    pub anchor_to: Anchor,
    pub offset: [Offset; 2],
    pub size: [Size; 2],
    pub arrange: Arrange,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            anchor_from: Anchor::Center,
            anchor_to: Anchor::Center,
            offset: [Offset::Px(0.0); 2],
            size: [Size::Percent(100.0); 2],
            arrange: Arrange::Free,
        }
    }
}

impl Layout {
    /// A node of this many units.
    #[must_use]
    pub fn px(width: f32, height: f32) -> Self {
        Self {
            size: [Size::Px(width), Size::Px(height)],
            ..Self::default()
        }
    }

    /// A node as big as what's inside.
    #[must_use]
    pub fn content() -> Self {
        Self {
            size: [Size::Content; 2],
            ..Self::default()
        }
    }

    /// With this point of it on the same point of its parent.
    #[must_use]
    pub const fn at(self, anchor: Anchor) -> Self {
        Self {
            anchor_from: anchor,
            anchor_to: anchor,
            ..self
        }
    }

    /// Moved by this many units, right and down.
    #[must_use]
    pub const fn moved(self, x: f32, y: f32) -> Self {
        Self {
            offset: [Offset::Px(x), Offset::Px(y)],
            ..self
        }
    }

    /// Placing its children this way.
    #[must_use]
    pub const fn arranged(self, arrange: Arrange) -> Self {
        Self { arrange, ..self }
    }

    /// Where this node goes in `parent`, given its size. In a stack, only the
    /// axis across it counts.
    fn anchored(&self, parent: Rect, size: [f32; 2]) -> [f32; 2] {
        let (from, to) = (self.anchor_from.fraction(), self.anchor_to.fraction());
        let at = |axis: usize, start: f32, room: f32| {
            let moved = match self.offset[axis] {
                Offset::Px(units) => units,
                Offset::Percent(share) => room * share / 100.0,
            };
            start + room * from[axis] - size[axis] * to[axis] + moved
        };
        [at(0, parent.x, parent.width), at(1, parent.y, parent.height)]
    }
}

/// How big the things that have a size of their own are.
pub trait Measure {
    fn image(&self, sprite: SpriteId) -> [f32; 2];
    fn text(&self, text: &str) -> [f32; 2];
}

/// Lays the whole tree out in a screen of this many UI units.
pub fn run(tree: &mut Tree, screen: [f32; 2], measure: &impl Measure) {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: screen[0],
        height: screen[1],
    };
    place(tree, Tree::ROOT, rect, measure);
}

/// Gives a node its rectangle, then places its children in it.
fn place(tree: &mut Tree, id: NodeId, rect: Rect, measure: &impl Measure) {
    tree.set_rect(id, rect);
    let children = shown_children(tree, id);
    match tree.node(id).layout.arrange {
        Arrange::Free => {
            for child in children {
                let size = size_in(tree, child, rect.size(), measure);
                let corner = tree.node(child).layout.anchored(rect, size);
                place(tree, child, at(corner, size), measure);
            }
        }
        Arrange::Stack(axis) => stack(tree, &children, axis, rect, measure),
        Arrange::Grid { columns, cell } => {
            let columns = columns.max(1) as usize;
            for (index, child) in children.into_iter().enumerate() {
                let cell_rect = Rect {
                    x: rect.x + (index % columns) as f32 * cell[0],
                    y: rect.y + (index / columns) as f32 * cell[1],
                    width: cell[0],
                    height: cell[1],
                };
                let size = size_in(tree, child, cell, measure);
                let corner = tree.node(child).layout.anchored(cell_rect, size);
                place(tree, child, at(corner, size), measure);
            }
        }
    }
}

/// One after another along `axis`. Children that `Fill` it share what the others
/// leave.
fn stack(tree: &mut Tree, children: &[NodeId], axis: Axis, rect: Rect, measure: &impl Measure) {
    let along = axis.index();
    let is_fill = |tree: &Tree, child: NodeId| tree.node(child).layout.size[along] == Size::Fill;
    let room = rect.size();
    let (mut taken, mut fills) = (0.0, 0_u32);
    for &child in children {
        if is_fill(tree, child) {
            fills += 1;
        } else {
            taken += size_in(tree, child, room, measure)[along];
        }
    }
    let share = if fills > 0 { (room[along] - taken).max(0.0) / fills as f32 } else { 0.0 };
    let mut cursor = [rect.x, rect.y][along];
    for &child in children {
        let mut size = size_in(tree, child, room, measure);
        if is_fill(tree, child) {
            size[along] = share;
        }
        // Across the stack, its anchors decide. Along it, the cursor does, and
        // its offset moves it from there without moving the rest.
        let mut corner = tree.node(child).layout.anchored(rect, size);
        let moved = match tree.node(child).layout.offset[along] {
            Offset::Px(units) => units,
            Offset::Percent(percent) => room[along] * percent / 100.0,
        };
        corner[along] = cursor + moved;
        place(tree, child, at(corner, size), measure);
        cursor += size[along];
    }
}

const fn at(corner: [f32; 2], size: [f32; 2]) -> Rect {
    Rect {
        x: corner[0],
        y: corner[1],
        width: size[0],
        height: size[1],
    }
}

fn shown_children(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    let children = tree.node(id).children().iter().copied();
    children.filter(|&child| tree.node(child).visible).collect()
}

/// A node's size inside a parent of `parent` units.
fn size_in(tree: &Tree, id: NodeId, parent: [f32; 2], measure: &impl Measure) -> [f32; 2] {
    let size = tree.node(id).layout.size;
    let mut content_size = None;
    let mut axis = |axis: usize| match size[axis] {
        Size::Px(units) => units,
        Size::Percent(share) => parent[axis] * share / 100.0,
        Size::Fill => parent[axis],
        Size::Content => content_size.get_or_insert_with(|| content(tree, id, measure))[axis],
    };
    [axis(0), axis(1)]
}

/// As big as what's inside a node: the larger of its own picture or text and its
/// children as arranged.
fn content(tree: &Tree, id: NodeId, measure: &impl Measure) -> [f32; 2] {
    let node = tree.node(id);
    let own = match &node.kind {
        Kind::Panel => [0.0, 0.0],
        Kind::Image(image) => image.sprite.map_or([0.0, 0.0], |sprite| measure.image(sprite)),
        Kind::Label(label) => measure.text(&label.text),
    };
    let children: Vec<[f32; 2]> = shown_children(tree, id)
        .into_iter()
        .map(|child| own_size(tree, child, measure))
        .collect();
    let arranged = match node.layout.arrange {
        Arrange::Free => [largest(&children, 0), largest(&children, 1)],
        Arrange::Stack(axis) => {
            let along = axis.index();
            let mut size = [largest(&children, 0), largest(&children, 1)];
            size[along] = children.iter().map(|child| child[along]).sum();
            size
        }
        Arrange::Grid { columns, cell } => {
            let (count, columns) = (children.len() as u32, columns.max(1));
            [
                count.min(columns) as f32 * cell[0],
                count.div_ceil(columns) as f32 * cell[1],
            ]
        }
    };
    [own[0].max(arranged[0]), own[1].max(arranged[1])]
}

/// A node's size when its parent is asking: a size that depends on the parent
/// counts for nothing, since the parent is waiting on this answer.
fn own_size(tree: &Tree, id: NodeId, measure: &impl Measure) -> [f32; 2] {
    let size = tree.node(id).layout.size;
    let mut content_size = None;
    let mut axis = |axis: usize| match size[axis] {
        Size::Px(units) => units,
        Size::Percent(_) | Size::Fill => 0.0,
        Size::Content => content_size.get_or_insert_with(|| content(tree, id, measure))[axis],
    };
    [axis(0), axis(1)]
}

fn largest(sizes: &[[f32; 2]], axis: usize) -> f32 {
    sizes.iter().map(|size| size[axis]).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tree::Node;

    /// Text is 6 units a character and 8 tall; there are no pictures.
    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, text: &str) -> [f32; 2] {
            [6.0 * text.len() as f32, 8.0]
        }
    }

    const SCREEN: [f32; 2] = [400.0, 200.0];

    fn laid_out(tree: &mut Tree) {
        run(tree, SCREEN, &Fake);
    }

    fn rect(tree: &Tree, id: NodeId) -> [f32; 4] {
        let rect = tree.node(id).rect;
        [rect.x, rect.y, rect.width, rect.height]
    }

    /// A 20 × 10 node with `from` on its parent and `to` on itself.
    fn anchored(from: Anchor, to: Anchor) -> [f32; 2] {
        let mut tree = Tree::new();
        let layout = Layout {
            anchor_from: from,
            anchor_to: to,
            ..Layout::px(20.0, 10.0)
        };
        let node = tree.add(Tree::ROOT, Node::panel(layout));
        laid_out(&mut tree);
        let [x, y, ..] = rect(&tree, node);
        [x, y]
    }

    #[test]
    fn anchors_put_one_point_on_another() {
        assert_eq!(anchored(Anchor::TopLeft, Anchor::TopLeft), [0.0, 0.0]);
        assert_eq!(anchored(Anchor::TopMiddle, Anchor::TopMiddle), [190.0, 0.0]);
        assert_eq!(anchored(Anchor::TopRight, Anchor::TopRight), [380.0, 0.0]);
        assert_eq!(anchored(Anchor::LeftMiddle, Anchor::LeftMiddle), [0.0, 95.0]);
        assert_eq!(anchored(Anchor::Center, Anchor::Center), [190.0, 95.0]);
        assert_eq!(anchored(Anchor::RightMiddle, Anchor::RightMiddle), [380.0, 95.0]);
        assert_eq!(anchored(Anchor::BottomLeft, Anchor::BottomLeft), [0.0, 190.0]);
        assert_eq!(anchored(Anchor::BottomMiddle, Anchor::BottomMiddle), [190.0, 190.0]);
        assert_eq!(anchored(Anchor::BottomRight, Anchor::BottomRight), [380.0, 190.0]);
        // Two different points: its bottom-right on the parent's centre.
        assert_eq!(anchored(Anchor::Center, Anchor::BottomRight), [180.0, 90.0]);
    }

    #[test]
    fn offsets_move_it_in_units_or_shares_of_the_parent() {
        let mut tree = Tree::new();
        let units = tree.add(Tree::ROOT, Node::panel(Layout::px(20.0, 10.0).at(Anchor::BottomMiddle).moved(4.0, -1.5)));
        let shares = Layout {
            offset: [Offset::Percent(10.0), Offset::Percent(-50.0)],
            ..Layout::px(20.0, 10.0).at(Anchor::TopLeft)
        };
        let shares = tree.add(Tree::ROOT, Node::panel(shares));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, units), [194.0, 188.5, 20.0, 10.0]);
        assert_eq!(rect(&tree, shares), [40.0, -100.0, 20.0, 10.0]);
    }

    #[test]
    fn sizes_are_units_shares_all_or_content() {
        let mut tree = Tree::new();
        let shares = Layout {
            size: [Size::Percent(50.0), Size::Percent(25.0)],
            ..Layout::default().at(Anchor::TopLeft)
        };
        let shares = tree.add(Tree::ROOT, Node::panel(shares));
        let inner = tree.add(shares, Node::panel(Layout { size: [Size::Percent(50.0); 2], ..Layout::default().at(Anchor::TopLeft) }));
        let fill = Layout {
            size: [Size::Fill, Size::Px(5.0)],
            ..Layout::default()
        };
        let fill = tree.add(Tree::ROOT, Node::panel(fill));
        let label = tree.add(Tree::ROOT, Node::label(Layout::content(), "hello"));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, shares)[2..], [200.0, 50.0]);
        assert_eq!(rect(&tree, inner)[2..], [100.0, 25.0], "a share of its parent, not the screen");
        assert_eq!(rect(&tree, fill)[2..], [400.0, 5.0]);
        assert_eq!(rect(&tree, label)[2..], [30.0, 8.0]);
    }

    #[test]
    fn a_stack_goes_one_after_another_and_anchors_across() {
        let mut tree = Tree::new();
        let row = tree.add(Tree::ROOT, Node::panel(Layout::px(100.0, 40.0).at(Anchor::TopLeft).arranged(Arrange::Stack(Axis::X))));
        let a = tree.add(row, Node::panel(Layout::px(10.0, 10.0).at(Anchor::TopLeft)));
        let b = tree.add(row, Node::panel(Layout::px(20.0, 10.0).at(Anchor::Center)));
        let c = tree.add(row, Node::panel(Layout::px(30.0, 10.0).at(Anchor::BottomRight)));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, a), [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(rect(&tree, b), [10.0, 15.0, 20.0, 10.0]);
        assert_eq!(rect(&tree, c), [30.0, 30.0, 30.0, 10.0]);
    }

    #[test]
    fn a_stack_goes_down_as_well() {
        let mut tree = Tree::new();
        let column = tree.add(Tree::ROOT, Node::panel(Layout::px(40.0, 100.0).at(Anchor::TopLeft).arranged(Arrange::Stack(Axis::Y))));
        let a = tree.add(column, Node::panel(Layout::px(10.0, 10.0).at(Anchor::TopRight)));
        let b = tree.add(column, Node::panel(Layout::px(10.0, 20.0).at(Anchor::TopLeft)));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, a), [30.0, 0.0, 10.0, 10.0]);
        assert_eq!(rect(&tree, b), [0.0, 10.0, 10.0, 20.0]);
    }

    #[test]
    fn fill_shares_what_the_others_leave() {
        let mut tree = Tree::new();
        let row = tree.add(Tree::ROOT, Node::panel(Layout::px(100.0, 10.0).at(Anchor::TopLeft).arranged(Arrange::Stack(Axis::X))));
        let wide = Layout {
            size: [Size::Fill, Size::Percent(100.0)],
            ..Layout::default()
        };
        let a = tree.add(row, Node::panel(wide));
        let b = tree.add(row, Node::panel(Layout::px(40.0, 10.0)));
        let c = tree.add(row, Node::panel(wide));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, a), [0.0, 0.0, 30.0, 10.0]);
        assert_eq!(rect(&tree, b), [30.0, 0.0, 40.0, 10.0]);
        assert_eq!(rect(&tree, c), [70.0, 0.0, 30.0, 10.0]);
    }

    #[test]
    fn a_grid_wraps_and_a_child_can_overlap_the_next_cell() {
        let mut tree = Tree::new();
        let grid = Layout::content().at(Anchor::TopLeft).arranged(Arrange::Grid {
            columns: 3,
            cell: [8.0, 9.0],
        });
        let grid = tree.add(Tree::ROOT, Node::panel(grid));
        let cells: Vec<NodeId> = (0..4)
            .map(|_| tree.add(grid, Node::panel(Layout::px(9.0, 9.0).at(Anchor::TopLeft))))
            .collect();
        laid_out(&mut tree);
        assert_eq!(rect(&tree, grid), [0.0, 0.0, 24.0, 18.0], "three cells across, two rows");
        assert_eq!(rect(&tree, cells[0]), [0.0, 0.0, 9.0, 9.0]);
        assert_eq!(rect(&tree, cells[1]), [8.0, 0.0, 9.0, 9.0]);
        assert_eq!(rect(&tree, cells[2]), [16.0, 0.0, 9.0, 9.0]);
        assert_eq!(rect(&tree, cells[3]), [0.0, 9.0, 9.0, 9.0]);
    }

    #[test]
    fn hidden_children_take_no_room() {
        let mut tree = Tree::new();
        let row = tree.add(Tree::ROOT, Node::panel(Layout::content().at(Anchor::TopLeft).arranged(Arrange::Stack(Axis::X))));
        tree.add(row, Node::panel(Layout::px(10.0, 5.0)).hidden());
        let shown = tree.add(row, Node::panel(Layout::px(20.0, 5.0)));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, row)[2], 20.0);
        assert_eq!(rect(&tree, shown)[0], 0.0);
    }

    #[test]
    fn content_adds_up_a_stack_inside_a_stack() {
        let mut tree = Tree::new();
        let outer = tree.add(Tree::ROOT, Node::panel(Layout::content().at(Anchor::BottomMiddle).arranged(Arrange::Stack(Axis::X))));
        let inner = tree.add(outer, Node::panel(Layout::content().arranged(Arrange::Stack(Axis::Y))));
        tree.add(inner, Node::panel(Layout::px(10.0, 5.0)));
        tree.add(inner, Node::panel(Layout::px(4.0, 5.0)));
        tree.add(outer, Node::panel(Layout::px(1.0, 30.0)));
        // Sized by its parent, while its parent is sized by its content: no room.
        tree.add(outer, Node::panel(Layout::default()));
        laid_out(&mut tree);
        assert_eq!(rect(&tree, outer)[2..], [11.0, 30.0]);
        assert_eq!(rect(&tree, outer)[..2], [194.5, 170.0], "and it's anchored by that size");
    }
}
