//! How the keys move the player (m5-plan Step 6): the modes, the double tap
//! that toggles flight, sprinting and sneaking, and what each tick's
//! `PlayerAuthInput` says about it, flag for flag as the official client
//! sends it (m5-plan Results, Step 1).
//!
//! **Honest reports** (worth teaching). Pumpkin takes a Bedrock player's
//! position as sent, and reads its state from the flags: on the ground from
//! `VerticalCollision` with a falling `delta`, sprinting, sneaking and flying
//! from the start and stop flags (m5-plan Finding 1). So each flag is set from
//! what the keys and the physics really did this tick, never assumed.

use glam::{DVec2, DVec3};
use pumpkin_protocol::{bedrock::server::player_auth_input::InputData, codec::bitset::Bitset};
use pumpkin_util::GameMode;

use super::physics::{self, Body, Intent, Outcome, Rules, Terrain};

/// A second press of jump within this many ticks toggles flight.
const DOUBLE_TAP: u32 = 7;
/// Sneaking scales the keys' direction down to this.
const SNEAK_SCALE: f64 = 0.3;
/// A survival player this hungry or more can't sprint.
const SPRINT_HUNGER: f32 = 6.0;

/// The keys that move the player, as they are this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

impl Keys {
    /// Bedrock's `move_vec` for these keys: x to the left, y forward, made
    /// one long when two are held, so a diagonal is no faster.
    #[must_use]
    pub fn direction(self) -> DVec2 {
        let axis = |plus: bool, minus: bool| f64::from(i8::from(plus) - i8::from(minus));
        let direction = DVec2::new(axis(self.left, self.right), axis(self.forward, self.back));
        if direction.length_squared() > 1.0 {
            direction.normalize()
        } else {
            direction
        }
    }
}

/// `PlayerAuthInput`'s flags, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags(pub u128);

impl Flags {
    pub fn set(&mut self, flag: InputData) {
        self.0 |= 1 << flag as u32;
    }

    #[must_use]
    pub const fn has(self, flag: InputData) -> bool {
        self.0 & (1 << flag as u32) != 0
    }

    #[must_use]
    pub const fn to_bitset(self) -> Bitset<66> {
        Bitset { bits: self.0 }
    }
}

/// What one tick's input reports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    pub flags: Flags,
    /// The keys' direction, scaled down while sneaking.
    pub move_vec: DVec2,
    /// The same, unscaled.
    pub raw_move: DVec2,
}

/// What the player's state says about this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Situation {
    pub gamemode: GameMode,
    /// Creative: a double tap on jump toggles flight.
    pub may_fly: bool,
    pub hunger: f32,
    /// `minecraft:movement`, `minecraft:gravity`, and the flying speed.
    pub movement: f64,
    pub gravity: f64,
    pub fly_speed: f64,
}

/// The movement state carried from tick to tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Movement {
    pub flying: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    /// Last tick's keys, to tell presses and releases from holds.
    pub(crate) last: Keys,
    /// Ticks left in which a second press of jump toggles flight.
    pub(crate) jump_window: u32,
    /// The last tick's collisions, which the tick a teleport is handled in
    /// reports again, as the official client does.
    pub(crate) last_outcome: Outcome,
}

impl Movement {
    /// One tick: the keys change the state, physics moves `body`, and the
    /// report says what happened.
    ///
    /// On the tick a teleport is handled the player doesn't move, and its
    /// velocity is gone: the report carries `HandledTeleport`.
    pub fn tick(
        &mut self,
        body: &mut Body,
        keys: Keys,
        yaw: f32,
        situation: &Situation,
        terrain: &impl Terrain,
        teleported: bool,
    ) -> Report {
        use InputData as F;
        let mut flags = Flags::default();
        flags.set(F::BlockBreakingDelayEnabled);
        let jump_pressed = keys.jump && !self.last.jump;
        let sneak_pressed = keys.sneak && !self.last.sneak;

        // Flight: always for a spectator; a double tap on jump in creative.
        let was_flying = self.flying;
        let spectator = situation.gamemode == GameMode::Spectator;
        self.jump_window = self.jump_window.saturating_sub(1);
        if spectator {
            self.flying = true;
        } else if !situation.may_fly {
            self.flying = false;
        } else if jump_pressed {
            if self.jump_window > 0 {
                self.flying = !self.flying;
                flags.set(if self.flying { F::StartFlying } else { F::StopFlying });
                self.jump_window = 0;
            } else {
                self.jump_window = DOUBLE_TAP;
            }
        }

        // Shift sneaks, except while flying, where it sinks.
        let sneaking = keys.sneak && !self.flying;
        if sneaking != self.sneaking {
            flags.set(if sneaking { F::StartSneaking } else { F::StopSneaking });
            self.sneaking = sneaking;
        }
        let raw_move = keys.direction();
        let move_vec = if sneaking { raw_move * SNEAK_SCALE } else { raw_move };

        // Sprinting starts with the sprint key while moving forward, and
        // lasts while forward is held.
        let can_sprint = keys.forward
            && !keys.back
            && !sneaking
            && (situation.hunger > SPRINT_HUNGER || situation.may_fly);
        let sprinting = can_sprint && (self.sprinting || keys.sprint);
        // `Sprinting` isn't the state: the official client sends it while
        // the sprint key is down and it is, or just was, sprinting. With the
        // key up it keeps sprinting, unflagged, until forward is let go
        // (`sprint.bin`), and the tick it stops with the key down carries
        // both `Sprinting` and `StopSprinting` (`sprint_jump.bin`).
        if keys.sprint && (sprinting || self.sprinting) {
            flags.set(F::Sprinting);
        }
        if sprinting != self.sprinting {
            flags.set(if sprinting { F::StartSprinting } else { F::StopSprinting });
            self.sprinting = sprinting;
        }
        if keys.sprint {
            flags.set(F::SprintDown);
        }

        // The keys themselves.
        for (held, flag) in [
            (keys.forward, F::Up),
            (keys.back, F::Down),
            (keys.left, F::Left),
            (keys.right, F::Right),
        ] {
            if held {
                flags.set(flag);
            }
        }
        if keys.jump {
            for flag in [F::JumpDown, F::Jumping, F::WantUp, F::JumpCurrentRaw] {
                flags.set(flag);
            }
        }
        if jump_pressed {
            flags.set(F::JumpPressedRaw);
        }
        if !keys.jump && self.last.jump {
            flags.set(F::JumpReleasedRaw);
        }
        if keys.sneak {
            for flag in [F::Sneaking, F::SneakDown, F::WantDown, F::SneakCurrentRaw] {
                flags.set(flag);
            }
        }
        if sneak_pressed {
            flags.set(F::SneakPressedRaw);
        }
        if !keys.sneak && self.last.sneak {
            flags.set(F::SneakReleasedRaw);
        }
        self.last = keys;

        let outcome = if teleported {
            flags.set(F::HandledTeleport);
            body.velocity = DVec3::ZERO;
            Outcome {
                jumped: false,
                ..self.last_outcome
            }
        } else if terrain.ready(body.feet) {
            let intent = Intent {
                move_vec,
                yaw,
                jump: keys.jump,
                descend: keys.sneak && self.flying,
                sneaking,
                sprinting,
            };
            let rules = Rules {
                flying: self.flying,
                was_flying,
                no_clip: spectator,
                movement: situation.movement,
                gravity: situation.gravity,
                fly_speed: situation.fly_speed,
            };
            physics::step(body, &intent, &rules, terrain)
        } else {
            // The chunks around the player haven't arrived: it waits.
            Outcome::default()
        };
        if outcome.horizontal_collision {
            flags.set(F::HorizontalCollision);
        }
        if outcome.vertical_collision {
            flags.set(F::VerticalCollision);
        }
        if outcome.jumped {
            flags.set(F::StartJumping);
        }
        self.last_outcome = outcome;
        Report {
            flags,
            move_vec,
            raw_move,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::physics::tests::{Kind, World};

    fn flat(_: i32, y: i32, _: i32) -> Kind {
        if y < 0 { Kind::Stone } else { Kind::Air }
    }

    fn situation(gamemode: GameMode) -> Situation {
        Situation {
            gamemode,
            may_fly: matches!(gamemode, GameMode::Creative | GameMode::Spectator),
            hunger: 20.0,
            movement: 0.1,
            gravity: 0.08,
            fly_speed: 0.05,
        }
    }

    fn standing() -> Body {
        Body {
            feet: DVec3::new(0.5, 0.0, 0.5),
            velocity: DVec3::ZERO,
            on_ground: true,
        }
    }

    const JUMP: Keys = Keys {
        forward: false,
        back: false,
        left: false,
        right: false,
        jump: true,
        sneak: false,
        sprint: false,
    };

    /// Taps jump twice, two ticks apart, and returns the flags of each tick.
    fn double_tap(gamemode: GameMode) -> (Movement, Vec<Flags>) {
        let world = World(flat);
        let mut body = standing();
        let mut movement = Movement::default();
        let situation = situation(gamemode);
        let mut flags = Vec::new();
        for keys in [JUMP, Keys::default(), JUMP, Keys::default()] {
            flags.push(movement.tick(&mut body, keys, 0.0, &situation, &world, false).flags);
        }
        (movement, flags)
    }

    #[test]
    fn a_double_tap_toggles_flight_in_creative_only() {
        let (movement, flags) = double_tap(GameMode::Creative);
        assert!(movement.flying);
        assert!(flags[0].has(InputData::StartJumping), "the first tap jumps");
        assert!(flags[2].has(InputData::StartFlying), "the second flies");

        let (movement, flags) = double_tap(GameMode::Survival);
        assert!(!movement.flying);
        assert!(flags.iter().all(|f| !f.has(InputData::StartFlying)));
    }

    #[test]
    fn sprint_and_sneak_report_their_start_and_stop() {
        let world = World(flat);
        let mut body = standing();
        let mut movement = Movement::default();
        let situation = situation(GameMode::Survival);
        let run = Keys {
            forward: true,
            sprint: true,
            ..Keys::default()
        };
        let first = movement.tick(&mut body, run, 0.0, &situation, &world, false).flags;
        for flag in [InputData::StartSprinting, InputData::SprintDown, InputData::Sprinting, InputData::Up] {
            assert!(first.has(flag), "{flag:?}");
        }
        let stop = movement.tick(&mut body, Keys::default(), 0.0, &situation, &world, false).flags;
        assert!(stop.has(InputData::StopSprinting) && !stop.has(InputData::Sprinting));

        let sneak = Keys {
            forward: true,
            sneak: true,
            ..Keys::default()
        };
        let report = movement.tick(&mut body, sneak, 0.0, &situation, &world, false);
        assert!(report.flags.has(InputData::StartSneaking) && report.flags.has(InputData::SneakPressedRaw));
        assert_eq!(report.move_vec, DVec2::new(0.0, 0.3));
        assert_eq!(report.raw_move, DVec2::new(0.0, 1.0));
        let released = movement.tick(&mut body, Keys::default(), 0.0, &situation, &world, false).flags;
        assert!(released.has(InputData::StopSneaking) && released.has(InputData::SneakReleasedRaw));
    }

    #[test]
    fn a_handled_teleport_holds_still_and_repeats_the_last_collisions() {
        let world = World(flat);
        let mut body = standing();
        let mut movement = Movement::default();
        let situation = situation(GameMode::Survival);
        movement.tick(&mut body, Keys::default(), 0.0, &situation, &world, false);
        body.velocity = DVec3::new(0.3, 0.0, 0.0);
        let before = body.feet;
        let flags = movement.tick(&mut body, Keys::default(), 0.0, &situation, &world, true).flags;
        assert!(flags.has(InputData::HandledTeleport) && flags.has(InputData::VerticalCollision));
        assert_eq!((body.feet, body.velocity), (before, DVec3::ZERO));
    }

    #[test]
    fn a_diagonal_is_no_faster() {
        let keys = Keys {
            forward: true,
            left: true,
            ..Keys::default()
        };
        assert!((keys.direction().length() - 1.0).abs() < 1e-12);
        assert_eq!(Keys { back: true, ..Keys::default() }.direction(), DVec2::new(0.0, -1.0));
    }
}
