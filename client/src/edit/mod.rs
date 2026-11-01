//! Changing the world: what the player aims at, and breaking and placing it (m5-plan
//! Decision 9).
//!
//! **Packets as the official client sends them** (m5-plan Results, Step 1). The
//! server trusts them, so their timing is what "breaking a block" is to it:
//! - in creative, a click is a `PlayerAction` of its own, and `StartDestroyBlock`
//!   and `PredictDestroyBlock` in that tick's `PlayerAuthInput`. Held, the same
//!   again every 5 ticks, on whatever is aimed at, with `ContinueDestroyBlock`;
//! - in survival, `StartDestroyBlock` once, then nothing until the block is done:
//!   `ContinueDestroyBlock` and `PredictDestroyBlock` together, on the tick the
//!   client's own count gets there. Moving onto another block sends
//!   `ContinueDestroyBlock` for it, and letting go `AbortDestroyBlock`;
//! - a right click on a block is `StartItemUseOn`, a swing, the use-item
//!   transaction and then one that clicks air, and letting go is `StopItemUseOn`.
//!
//! **Why this module only decides** (worth teaching). [`Editor`] takes what's
//! aimed at and how fast it breaks, and gives back [`Command`]s, touching neither
//! the world nor the network. So its tests can run the capture's timelines tick by
//! tick, and `Session` is left with one job: carrying them out.

pub mod aim;
pub mod predict;

use glam::{DVec3, IVec3};
use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId,
    block_properties::SnowLikeProperties,
    item_stack::ItemStack,
    tag::{self, Taggable},
};
use pumpkin_protocol::bedrock::server::player_action::PlayerActionType;

use crate::{blocks::shapes, player::physics::player_box, world::World};

/// Ticks between creative breaks while the button is held: the capture's 5.
const CREATIVE_DELAY: u32 = 5;
/// Ticks from a survival break to starting on the next block: a break at 4307 and
/// the next start at 4313, in the capture.
const SURVIVAL_DELAY: u32 = 6;
/// Ticks between uses while the button is held.
const USE_DELAY: u32 = 4;
/// Progress this close to whole counts as whole, so that adding a 150th 150 times
/// gets there in `f32`.
const NEARLY_WHOLE: f32 = 1e-4;
/// Survival's crack has this many stages.
pub const CRACK_STAGES: usize = 10;

/// A step of breaking, as `PlayerAuthInput`'s block actions name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Start,
    Continue,
    Predict,
    Abort,
}

impl Step {
    #[must_use]
    pub const fn action(self) -> PlayerActionType {
        match self {
            Self::Start => PlayerActionType::StartDestroyBlock,
            Self::Continue => PlayerActionType::ContinueDestroyBlock,
            Self::Predict => PlayerActionType::PredictDestroyBlock,
            Self::Abort => PlayerActionType::AbortDestroyBlock,
        }
    }
}

/// Something for `Session` to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// `PlayerAction` `CreativeDestroyBlock`, a packet of its own.
    CreativeDestroy { block: IVec3, face: i32 },
    /// A block action in this tick's `PlayerAuthInput`.
    Block { step: Step, block: IVec3, face: i32 },
    /// `Animate`'s swing, source `Mine`: one a tick while the button is held.
    Swing,
    /// The block is gone, as far as the client shows.
    Broke(IVec3),
}

/// What's aimed at, as breaking needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aimed {
    pub block: IVec3,
    pub face: i32,
    /// How much of the block a tick breaks: 1 is the whole of it.
    pub speed: f32,
}

/// What the player's state says about breaking this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub creative: bool,
    /// `UpdateAbilities`' mine: an adventure player can't.
    pub may_mine: bool,
    pub aimed: Option<Aimed>,
}

/// A block being broken in survival.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mining {
    block: IVec3,
    progress: f32,
}

/// The state of breaking, from one tick to the next.
#[derive(Debug, Default)]
pub struct Editor {
    mining: Option<Mining>,
    /// Ticks until the next break may start, without a new press.
    delay: u32,
    /// The button went down since the last tick.
    pressed: bool,
    /// Where the last use went, until the use button comes up.
    using: Option<IVec3>,
    /// Ticks until a held use goes again, and whether it does.
    use_delay: u32,
    use_repeats: bool,
}

/// What the use button asks of this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseTick {
    Nothing,
    /// It came up: `StopItemUseOn` for here.
    Stop(IVec3),
    /// It's held, and it's time for another.
    Repeat,
}

impl Editor {
    /// The attack button went down. A creative break happens now, so that it shows
    /// in this frame; its block actions go with the next tick's input. Survival
    /// starts on the next tick, which is the first to count towards the block.
    #[must_use]
    pub fn attack_pressed(&mut self, view: &View) -> Vec<Command> {
        self.pressed = true;
        let mut out = Vec::new();
        if view.creative && view.may_mine {
            if let Some(aimed) = view.aimed {
                creative_break(aimed, Step::Start, &mut out);
            }
            // The tick that follows counts down too, so it's one more than the gap.
            self.delay = CREATIVE_DELAY + 1;
        }
        out
    }

    /// One tick, with the attack button `held` or not.
    #[must_use]
    pub fn tick(&mut self, held: bool, view: &View) -> Vec<Command> {
        let pressed = std::mem::take(&mut self.pressed);
        self.delay = self.delay.saturating_sub(1);
        let mut out = Vec::new();
        if !held && !pressed {
            self.abort(&mut out);
            return out;
        }
        out.push(Command::Swing);
        if !view.may_mine {
            self.mining = None;
        } else if view.creative {
            self.mining = None;
            if self.delay == 0
                && let Some(aimed) = view.aimed
            {
                creative_break(aimed, Step::Continue, &mut out);
                self.delay = CREATIVE_DELAY;
            }
        } else {
            self.survival(pressed, view, &mut out);
        }
        out
    }

    fn survival(&mut self, pressed: bool, view: &View, out: &mut Vec<Command>) {
        let Some(aimed) = view.aimed else {
            self.abort(out);
            return;
        };
        match &mut self.mining {
            Some(mining) if mining.block == aimed.block => {
                mining.progress += aimed.speed;
                if mining.progress >= 1.0 - NEARLY_WHOLE {
                    out.push(block(Step::Continue, aimed));
                    self.finish(aimed, out);
                }
            }
            // A new press starts at once. Holding on, it waits out the last break.
            _ if self.delay > 0 && !pressed => {}
            _ => {
                out.push(block(if pressed { Step::Start } else { Step::Continue }, aimed));
                if aimed.speed >= 1.0 {
                    self.finish(aimed, out);
                } else {
                    self.mining = Some(Mining {
                        block: aimed.block,
                        progress: aimed.speed,
                    });
                }
            }
        }
    }

    fn finish(&mut self, aimed: Aimed, out: &mut Vec<Command>) {
        out.push(block(Step::Predict, aimed));
        out.push(Command::Broke(aimed.block));
        self.mining = None;
        self.delay = SURVIVAL_DELAY;
    }

    fn abort(&mut self, out: &mut Vec<Command>) {
        if let Some(mining) = self.mining.take() {
            out.push(Command::Block {
                step: Step::Abort,
                block: mining.block,
                face: 0,
            });
        }
    }

    /// A use has gone out. If there was one before it, this returns where it went, for
    /// `StopItemUseOn`, as when holding the button moves on to the next block.
    pub const fn used(&mut self, plan: &UsePlan, pressed: bool) -> Option<IVec3> {
        // The tick that follows a press counts down too.
        self.use_delay = if pressed { USE_DELAY + 1 } else { USE_DELAY };
        self.use_repeats = plan.repeats;
        self.using.replace(plan.result)
    }

    /// One tick of the use button.
    pub const fn use_tick(&mut self, held: bool) -> UseTick {
        self.use_delay = self.use_delay.saturating_sub(1);
        if !held {
            self.use_repeats = false;
            return match self.using.take() {
                Some(target) => UseTick::Stop(target),
                None => UseTick::Nothing,
            };
        }
        if self.use_repeats && self.use_delay == 0 {
            UseTick::Repeat
        } else {
            UseTick::Nothing
        }
    }

    /// The block being broken in survival, and which of the crack's stages it's at.
    #[must_use]
    pub fn crack(&self) -> Option<(IVec3, usize)> {
        self.mining
            .map(|mining| (mining.block, ((mining.progress * CRACK_STAGES as f32) as usize).min(CRACK_STAGES - 1)))
    }
}

const fn block(step: Step, aimed: Aimed) -> Command {
    Command::Block {
        step,
        block: aimed.block,
        face: aimed.face,
    }
}

fn creative_break(aimed: Aimed, step: Step, out: &mut Vec<Command>) {
    out.extend([
        Command::CreativeDestroy {
            block: aimed.block,
            face: aimed.face,
        },
        block(step, aimed),
        block(Step::Predict, aimed),
        Command::Broke(aimed.block),
    ]);
}

/// What a right click on `clicked`, through `face`, will do, as far as the client can
/// tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsePlan {
    /// Where the held block would go, which `StartItemUseOn` names.
    pub result: IVec3,
    /// It will go there: the hand loses one, and the transaction says it predicted that.
    pub places: bool,
    /// The block to show there now, if the result is certain.
    pub shows: Option<u32>,
    /// Holding the button does it again.
    pub repeats: bool,
}

/// Works out what a right click will do (worth teaching). These are the server's
/// rules (`BlockRegistry::placement_target`), which the client needs in order to say
/// the same thing ahead of it.
///
/// - **Where:** the clicked block itself if a new block replaces it, like grass, and
///   otherwise the block beyond the face, if that's replaceable. Nothing else.
/// - **Whether:** not when the clicked block reacts to being clicked, not on top of
///   the player, and not outside the world.
/// - **What to show:** only what's certain. A block with one state is that state
///   however it's placed. A log or a stair depends on how it's placed, and the rules
///   for that are the server's, so the player sees those a tick or two later.
#[must_use]
pub fn plan_use(
    world: &World,
    feet: DVec3,
    may_build: bool,
    held: Option<&'static Block>,
    clicked: IVec3,
    face: BlockDirection,
) -> UsePlan {
    let offset = face.to_offset();
    let beyond = clicked + IVec3::new(offset.x, offset.y, offset.z);
    let mut plan = UsePlan {
        result: beyond,
        places: false,
        shows: None,
        repeats: false,
    };
    let state_at = |at: IVec3| world.block(at.x, at.y, at.z, 0).and_then(|id| world.blocks().java_state(id));
    let (Some(block), Some(clicked_state)) = (held, state_at(clicked)) else {
        return plan;
    };
    let clicked_block = Block::from_state_id(clicked_state);
    if reacts_to_use(clicked_block) {
        return plan;
    }
    plan.repeats = true;
    if !may_build {
        return plan;
    }
    // A block onto itself can be a change of state, like a slab into a double slab:
    // that's for the server to say.
    let gives_way = |state: BlockStateId| {
        let other = Block::from_state_id(state);
        other.id != block.id && replaceable(other, state)
    };
    let target = if gives_way(clicked_state) {
        clicked
    } else if state_at(beyond).is_some_and(gives_way) {
        beyond
    } else {
        return plan;
    };
    plan.result = target;
    let in_the_world = world
        .chunk(target.x >> 4, target.z >> 4)
        .is_some_and(|chunk| (chunk.min_y()..chunk.max_y()).contains(&target.y));
    if !in_the_world {
        return plan;
    }
    let id = BlockState::to_be_network_id(block.default_state.id);
    let mut boxes = Vec::new();
    shapes::collision_boxes(world.blocks(), id, target.x, target.y, target.z, &mut boxes);
    let player = player_box(feet);
    if boxes.iter().any(|area| area.intersects(&player)) {
        return plan;
    }
    plan.places = true;
    let certain = block.states.len() == 1 && shapes::is_full_cube(block.default_state.id);
    plan.shows = certain.then_some(id);
    plan
}

/// Whether a new block takes the place of this one: the server's
/// `can_replace_with_other_block`.
fn replaceable(block: &Block, state: BlockStateId) -> bool {
    if block.id == Block::SNOW.id {
        return SnowLikeProperties::from_state_id(state).layers == 1;
    }
    block.id == Block::SCULK_VEIN.id || state.to_state().replaceable()
}

/// Blocks that do something when clicked, instead of having a block put against them.
/// The server's blocks say this in code, so here it's a list, and one that's missing
/// only costs a block that shows for half a second.
const REACTING: [&str; 41] = [
    "barrel", "beacon", "bell", "blast_furnace", "brewing_stand", "cake", "cartography_table",
    "chain_command_block", "chest", "chiseled_bookshelf", "command_block", "comparator", "composter",
    "crafter", "crafting_table", "daylight_detector", "decorated_pot", "dispenser", "dragon_egg", "dropper",
    "enchanting_table", "ender_chest", "furnace", "grindstone", "hopper", "jigsaw", "jukebox", "lectern",
    "lever", "lodestone", "loom", "note_block", "redstone_wire", "repeater", "repeating_command_block",
    "respawn_anchor", "smithing_table", "smoker", "stonecutter", "structure_block", "trapped_chest",
];

fn reacts_to_use(block: &Block) -> bool {
    REACTING.contains(&block.name)
        || [
            &tag::Block::MINECRAFT_ALL_SIGNS,
            &tag::Block::MINECRAFT_ANVIL,
            &tag::Block::MINECRAFT_BEDS,
            &tag::Block::MINECRAFT_BUTTONS,
            &tag::Block::MINECRAFT_CAMPFIRES,
            &tag::Block::MINECRAFT_CANDLE_CAKES,
            &tag::Block::MINECRAFT_CAULDRONS,
            &tag::Block::MINECRAFT_DOORS,
            &tag::Block::MINECRAFT_FENCE_GATES,
            &tag::Block::MINECRAFT_FLOWER_POTS,
            &tag::Block::MINECRAFT_SHULKER_BOXES,
            &tag::Block::MINECRAFT_TRAPDOORS,
        ]
        .iter()
        .any(|&tag| block.has_tag(tag))
}

/// How much of a block in `state` a tick breaks, with `held` in the hand (worth
/// teaching): the server's sum (`calc_block_breaking`).
///
/// A tool's speed for the block, over its hardness, and over 30, or over 100 if
/// the block needs a tool and this isn't one. Not standing on anything takes it
/// to a fifth, and so does having the eyes in water, which Bedrock and Java both
/// do, and Pumpkin doesn't yet. That only makes the client later than the server
/// would allow. It doesn't know about Efficiency or Haste yet, which makes it later
/// too.
#[must_use]
pub fn breaking_speed(state: BlockStateId, held: &ItemStack, on_ground: bool, eyes_in_water: bool) -> f32 {
    let java = state.to_state();
    let block = Block::from_state_id(state);
    #[expect(clippy::float_cmp, reason = "the data says -1, exactly, for what can't be broken")]
    if java.hardness == -1.0 {
        return 0.0;
    }
    let mut speed = held.get_speed(block);
    if !on_ground {
        speed /= 5.0;
    }
    if eyes_in_water {
        speed /= 5.0;
    }
    let right_tool = !java.tool_required() || held.is_correct_for_drops(block);
    speed / java.hardness / if right_tool { 30.0 } else { 100.0 }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::item::Item;

    use super::*;

    /// A chunk of air around the origin, with these blocks in it.
    fn world_of(blocks: &[(IVec3, &Block)]) -> World {
        let mut world = World::new();
        world.add_empty_chunk(0, 0);
        for &(at, block) in blocks {
            assert!(world.set_block(at.x, at.y, at.z, 0, id_of(block)));
        }
        world
    }

    fn id_of(block: &Block) -> u32 {
        BlockState::to_be_network_id(block.default_state.id)
    }

    const FLOOR: IVec3 = IVec3::new(4, 64, 4);
    const ABOVE: IVec3 = IVec3::new(4, 65, 4);
    /// Standing well away from it.
    const AWAY: DVec3 = DVec3::new(10.5, 65.0, 10.5);

    fn plan_on(world: &World, held: &'static Block) -> UsePlan {
        plan_use(world, AWAY, true, Some(held), FLOOR, BlockDirection::Up)
    }

    #[test]
    fn a_block_goes_against_the_face_clicked() {
        let world = world_of(&[(FLOOR, &Block::STONE)]);
        assert_eq!(
            plan_on(&world, &Block::DIRT),
            UsePlan {
                result: ABOVE,
                places: true,
                shows: Some(id_of(&Block::DIRT)),
                repeats: true
            }
        );
        // And round the side.
        let side = plan_use(&world, AWAY, true, Some(&Block::DIRT), FLOOR, BlockDirection::West);
        assert_eq!(side.result, IVec3::new(3, 64, 4));
    }

    #[test]
    fn a_block_goes_into_grass_when_grass_is_clicked_or_is_in_the_way() {
        let world = world_of(&[(FLOOR, &Block::STONE), (ABOVE, &Block::SHORT_GRASS)]);
        let through = plan_on(&world, &Block::DIRT);
        assert_eq!((through.result, through.places), (ABOVE, true));
        // Clicking the grass itself replaces it, whichever face the click is on.
        let on_it = plan_use(&world, AWAY, true, Some(&Block::DIRT), ABOVE, BlockDirection::North);
        assert_eq!((on_it.result, on_it.places), (ABOVE, true));
    }

    #[test]
    fn a_block_goes_into_water_and_a_thin_layer_of_snow_but_not_a_deep_one() {
        let world = world_of(&[(FLOOR, &Block::STONE), (ABOVE, &Block::WATER)]);
        assert_eq!(plan_on(&world, &Block::DIRT).shows, Some(id_of(&Block::DIRT)));
        let world = world_of(&[(FLOOR, &Block::STONE), (ABOVE, &Block::SNOW)]);
        assert_eq!(plan_on(&world, &Block::DIRT).result, ABOVE);
        assert!(plan_on(&world, &Block::DIRT).places, "one layer gives way");
    }

    #[test]
    fn nothing_is_placed_where_something_solid_is() {
        let world = world_of(&[(FLOOR, &Block::STONE), (ABOVE, &Block::OAK_PLANKS)]);
        let plan = plan_on(&world, &Block::DIRT);
        assert!(!plan.places && plan.shows.is_none(), "{plan:?}");
        assert!(plan.repeats, "holding on keeps trying, as the aim moves");
    }

    #[test]
    fn a_block_isnt_placed_on_top_of_the_player() {
        let world = world_of(&[(FLOOR, &Block::STONE)]);
        let feet_there = ABOVE.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        let plan = plan_use(&world, feet_there, true, Some(&Block::DIRT), FLOOR, BlockDirection::Up);
        assert!(!plan.places && plan.shows.is_none(), "{plan:?}");
        // A block over its head is out of the way, and so is one beside it.
        let beside = feet_there + DVec3::new(1.0, 0.0, 0.0);
        assert!(plan_use(&world, beside, true, Some(&Block::DIRT), FLOOR, BlockDirection::Up).places);
    }

    #[test]
    fn only_a_block_with_one_state_shows_at_once() {
        let world = world_of(&[(FLOOR, &Block::STONE)]);
        for certain in [&Block::DIRT, &Block::STONE, &Block::OAK_PLANKS, &Block::SAND] {
            assert_eq!(plan_on(&world, certain).shows, Some(id_of(certain)), "{}", certain.name);
        }
        for depends in [&Block::OAK_LOG, &Block::OAK_STAIRS, &Block::FURNACE, &Block::TORCH, &Block::OAK_SLAB, &Block::POPPY] {
            let plan = plan_on(&world, depends);
            assert!(plan.places && plan.shows.is_none(), "{}: {plan:?}", depends.name);
        }
    }

    #[test]
    fn a_block_that_reacts_to_clicks_takes_the_click() {
        for reacting in [&Block::CHEST, &Block::OAK_DOOR, &Block::CRAFTING_TABLE, &Block::LEVER, &Block::OAK_TRAPDOOR] {
            let world = world_of(&[(FLOOR, reacting)]);
            let plan = plan_on(&world, &Block::DIRT);
            assert!(!plan.places && plan.shows.is_none() && !plan.repeats, "{}: {plan:?}", reacting.name);
        }
    }

    #[test]
    fn the_same_block_is_for_the_server_to_settle() {
        let world = world_of(&[(FLOOR, &Block::OAK_SLAB)]);
        let plan = plan_use(&world, AWAY, true, Some(&Block::OAK_SLAB), FLOOR, BlockDirection::Up);
        // The block beyond gives way, so it's that for all the client knows; not the slab.
        assert_eq!(plan.result, ABOVE);
    }

    #[test]
    fn an_empty_hand_or_no_right_to_build_places_nothing() {
        let world = world_of(&[(FLOOR, &Block::STONE)]);
        let empty = plan_use(&world, AWAY, true, None, FLOOR, BlockDirection::Up);
        assert!(!empty.places && !empty.repeats && empty.result == ABOVE, "{empty:?}");
        let forbidden = plan_use(&world, AWAY, false, Some(&Block::DIRT), FLOOR, BlockDirection::Up);
        assert!(!forbidden.places && forbidden.shows.is_none(), "{forbidden:?}");
    }

    #[test]
    fn nothing_is_placed_outside_the_world() {
        let top = IVec3::new(4, 319, 4);
        let world = world_of(&[(top, &Block::STONE)]);
        let plan = plan_use(&world, AWAY, true, Some(&Block::DIRT), top, BlockDirection::Up);
        assert!(!plan.places, "over the top: {plan:?}");
        let edge = IVec3::new(15, 64, 4);
        let world = world_of(&[(edge, &Block::STONE)]);
        let plan = plan_use(&world, AWAY, true, Some(&Block::DIRT), edge, BlockDirection::East);
        assert!(!plan.places, "a chunk that isn't here: {plan:?}");
    }

    #[test]
    fn holding_the_button_repeats_every_four_ticks_and_letting_go_stops() {
        let mut editor = Editor::default();
        let plan = UsePlan {
            result: ABOVE,
            places: true,
            shows: None,
            repeats: true,
        };
        assert_eq!(editor.used(&plan, true), None);
        // The tick the click went with, then the wait.
        for tick in 0..4 {
            assert_eq!(editor.use_tick(true), UseTick::Nothing, "{tick}");
        }
        assert_eq!(editor.use_tick(true), UseTick::Repeat);
        let next = UsePlan { result: FLOOR, ..plan };
        assert_eq!(editor.used(&next, false), Some(ABOVE), "the last one to stop");
        for tick in 0..3 {
            assert_eq!(editor.use_tick(true), UseTick::Nothing, "{tick}");
        }
        assert_eq!(editor.use_tick(true), UseTick::Repeat);
        assert_eq!(editor.use_tick(false), UseTick::Stop(FLOOR));
        assert_eq!(editor.use_tick(false), UseTick::Nothing, "once");
        assert_eq!(editor.use_tick(true), UseTick::Nothing, "not until it's pressed again");
    }

    #[test]
    fn what_doesnt_repeat_waits_for_the_button_to_come_up() {
        let mut editor = Editor::default();
        let plan = UsePlan {
            result: ABOVE,
            places: false,
            shows: None,
            repeats: false,
        };
        let _ = editor.used(&plan, true);
        for _ in 0..20 {
            assert_eq!(editor.use_tick(true), UseTick::Nothing);
        }
        assert_eq!(editor.use_tick(false), UseTick::Stop(ABOVE));
    }

    const HERE: IVec3 = IVec3::new(0, 200, 29);
    const THERE: IVec3 = IVec3::new(0, 199, 30);

    fn aim(block: IVec3, face: i32, speed: f32) -> View {
        View {
            creative: false,
            may_mine: true,
            aimed: Some(Aimed { block, face, speed }),
        }
    }

    fn creative(block: IVec3, face: i32) -> View {
        View {
            creative: true,
            ..aim(block, face, 0.0)
        }
    }

    /// The commands of a tick, without the swing.
    fn acted(commands: Vec<Command>) -> Vec<Command> {
        commands.into_iter().filter(|command| *command != Command::Swing).collect()
    }

    fn step(step: Step, block: IVec3, face: i32) -> Command {
        Command::Block { step, block, face }
    }

    /// How many ticks a block takes, the first included.
    fn ticks_to_break(speed: f32) -> u32 {
        let mut editor = Editor::default();
        assert!(editor.attack_pressed(&aim(HERE, 1, speed)).is_empty(), "survival waits for the tick");
        for tick in 1..1000 {
            if acted(editor.tick(true, &aim(HERE, 1, speed))).contains(&Command::Broke(HERE)) {
                return tick;
            }
        }
        panic!("it never broke");
    }

    fn speed_of(block: &Block, held: &ItemStack) -> f32 {
        breaking_speed(block.default_state.id, held, true, false)
    }

    #[test]
    fn survival_takes_the_servers_time() {
        let hand = ItemStack::EMPTY;
        let wooden_pickaxe = ItemStack::new(1, &Item::WOODEN_PICKAXE);
        assert_eq!(ticks_to_break(speed_of(&Block::DIRT, hand)), 15);
        assert_eq!(ticks_to_break(speed_of(&Block::STONE, hand)), 150);
        assert_eq!(ticks_to_break(speed_of(&Block::STONE, &wooden_pickaxe)), 23);
    }

    #[test]
    fn being_off_the_ground_or_under_water_is_five_times_slower() {
        let stone = Block::DIRT.default_state.id;
        let hand = ItemStack::EMPTY;
        let walking = breaking_speed(stone, hand, true, false);
        assert!((breaking_speed(stone, hand, false, false) - walking / 5.0).abs() < 1e-6);
        assert!((breaking_speed(stone, hand, true, true) - walking / 5.0).abs() < 1e-6);
        assert!((breaking_speed(stone, hand, false, true) - walking / 25.0).abs() < 1e-6);
    }

    #[test]
    fn what_cannot_be_broken_never_is() {
        assert_eq!(speed_of(&Block::BEDROCK, ItemStack::EMPTY), 0.0);
    }

    /// The capture's survival run: a start at tick 4293, the dirt done at 4307,
    /// another block aimed at 4313, and the button up at 4314.
    #[test]
    fn survival_goes_as_the_capture_does() {
        let mut editor = Editor::default();
        let dirt = speed_of(&Block::DIRT, ItemStack::EMPTY);
        let here = aim(HERE, 4, dirt);
        let _ = editor.attack_pressed(&here);
        assert_eq!(acted(editor.tick(true, &here)), [step(Step::Start, HERE, 4)], "4293");
        for tick in 4294..4307 {
            assert_eq!(acted(editor.tick(true, &here)), [], "{tick}: nothing per tick");
        }
        assert_eq!(
            acted(editor.tick(true, &here)),
            [step(Step::Continue, HERE, 4), step(Step::Predict, HERE, 4), Command::Broke(HERE)],
            "4307"
        );
        let there = aim(THERE, 1, dirt);
        for tick in 4308..4313 {
            assert_eq!(acted(editor.tick(true, &there)), [], "{tick}: the wait");
        }
        assert_eq!(acted(editor.tick(true, &there)), [step(Step::Continue, THERE, 1)], "4313");
        assert_eq!(acted(editor.tick(false, &there)), [step(Step::Abort, THERE, 0)], "4314");
        assert_eq!(acted(editor.tick(false, &there)), [], "once");
    }

    /// The capture's creative run: a block every 5 ticks while the button is held.
    #[test]
    fn creative_goes_as_the_capture_does() {
        let mut editor = Editor::default();
        assert_eq!(
            editor.attack_pressed(&creative(HERE, 1)),
            [
                Command::CreativeDestroy { block: HERE, face: 1 },
                step(Step::Start, HERE, 1),
                step(Step::Predict, HERE, 1),
                Command::Broke(HERE)
            ]
        );
        let there = creative(THERE, 2);
        // The tick that carries the click's own block actions.
        assert_eq!(acted(editor.tick(true, &there)), []);
        for round in 0..3 {
            for tick in 0..5 {
                let commands = acted(editor.tick(true, &there));
                if tick < 4 {
                    assert_eq!(commands, [], "round {round}, tick {tick}");
                } else {
                    assert_eq!(
                        commands,
                        [
                            Command::CreativeDestroy { block: THERE, face: 2 },
                            step(Step::Continue, THERE, 2),
                            step(Step::Predict, THERE, 2),
                            Command::Broke(THERE)
                        ],
                        "round {round}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_new_click_doesnt_wait_for_the_last_break() {
        let mut editor = Editor::default();
        let _ = editor.attack_pressed(&creative(HERE, 1));
        let _ = editor.tick(false, &creative(HERE, 1));
        let again = editor.attack_pressed(&creative(THERE, 1));
        assert!(again.contains(&Command::Broke(THERE)), "{again:?}");

        let mut editor = Editor::default();
        let plant = aim(HERE, 1, f32::INFINITY);
        let _ = editor.attack_pressed(&plant);
        assert!(acted(editor.tick(true, &plant)).contains(&Command::Broke(HERE)));
        let _ = editor.tick(false, &plant);
        let _ = editor.attack_pressed(&plant);
        assert!(acted(editor.tick(true, &plant)).contains(&Command::Broke(HERE)));
    }

    #[test]
    fn what_breaks_at_once_says_so_on_the_first_tick() {
        let mut editor = Editor::default();
        let plant = aim(HERE, 1, speed_of(&Block::SHORT_GRASS, ItemStack::EMPTY));
        let _ = editor.attack_pressed(&plant);
        assert_eq!(
            acted(editor.tick(true, &plant)),
            [step(Step::Start, HERE, 1), step(Step::Predict, HERE, 1), Command::Broke(HERE)]
        );
    }

    #[test]
    fn a_click_shorter_than_a_tick_still_starts_and_aborts() {
        let mut editor = Editor::default();
        let here = aim(HERE, 4, 0.1);
        let _ = editor.attack_pressed(&here);
        assert_eq!(acted(editor.tick(false, &here)), [step(Step::Start, HERE, 4)]);
        assert_eq!(acted(editor.tick(false, &here)), [step(Step::Abort, HERE, 0)]);
    }

    #[test]
    fn looking_away_aborts_and_looking_back_starts_over() {
        let mut editor = Editor::default();
        let here = aim(HERE, 4, 0.1);
        let _ = editor.attack_pressed(&here);
        for _ in 0..5 {
            let _ = editor.tick(true, &here);
        }
        let away = View { aimed: None, ..here };
        assert_eq!(acted(editor.tick(true, &away)), [step(Step::Abort, HERE, 0)]);
        assert_eq!(editor.crack(), None);
        assert_eq!(acted(editor.tick(true, &here)), [step(Step::Continue, HERE, 4)]);
        assert_eq!(editor.crack(), Some((HERE, 1)), "from the start again");
    }

    #[test]
    fn an_adventure_player_only_swings() {
        let mut editor = Editor::default();
        let here = View {
            may_mine: false,
            ..aim(HERE, 4, 0.1)
        };
        assert_eq!(editor.attack_pressed(&here), []);
        assert_eq!(editor.tick(true, &here), [Command::Swing]);
    }

    #[test]
    fn the_crack_grows_through_its_stages() {
        let mut editor = Editor::default();
        let here = aim(HERE, 4, 0.1);
        let _ = editor.attack_pressed(&here);
        let mut stages = Vec::new();
        for _ in 0..9 {
            let _ = editor.tick(true, &here);
            stages.push(editor.crack().unwrap().1);
        }
        assert_eq!(stages, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
