//! Changing the world: what the player aims at, and breaking it (m5-plan Decision
//! 9).
//!
//! **Packets as the official client sends them** (m5-plan Results, Step 1). The
//! server trusts them, so their timing is what "breaking a block" is to it:
//! - in creative, a click is a `PlayerAction` of its own, and `StartDestroyBlock`
//!   and `PredictDestroyBlock` in that tick's `PlayerAuthInput`. Held, the same
//!   again every 5 ticks, on whatever is aimed at, with `ContinueDestroyBlock`;
//! - in survival, `StartDestroyBlock` once, then nothing until the block is done:
//!   `ContinueDestroyBlock` and `PredictDestroyBlock` together, on the tick the
//!   client's own count gets there. Moving onto another block sends
//!   `ContinueDestroyBlock` for it, and letting go `AbortDestroyBlock`.
//!
//! **Why this module only decides** (worth teaching). [`Editor`] takes what's
//! aimed at and how fast it breaks, and gives back [`Command`]s, touching neither
//! the world nor the network. So its tests can run the capture's timelines tick by
//! tick, and `Session` is left with one job: carrying them out.

pub mod aim;
pub mod predict;

use glam::IVec3;
use pumpkin_data::{Block, BlockStateId, item_stack::ItemStack};
use pumpkin_protocol::bedrock::server::player_action::PlayerActionType;

/// Ticks between creative breaks while the button is held: the capture's 5.
const CREATIVE_DELAY: u32 = 5;
/// Ticks from a survival break to starting on the next block: a break at 4307 and
/// the next start at 4313, in the capture.
const SURVIVAL_DELAY: u32 = 6;
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
