//! The player's physics (m5-plan Decision 5): one tick at a time, as pure
//! functions over a block lookup, so the tests need no server and no world.
//!
//! **Vanilla's rules, checked against the official client** (worth
//! teaching). Each tick, the keys give the player a push, it moves by its
//! velocity and stops where boxes are in the way, and then its velocity
//! decays: friction along the ground, drag and gravity up and down. Those are
//! Java Edition's rules, which Pumpkin's mob physics follows too. Step 1
//! recorded the official Bedrock client making the same moves, and the
//! tests in `traces.rs` replay each recording tick by tick. Where Bedrock
//! differs, its numbers win (m5-plan Deviations):
//! - the push is `move_vec`, which is already normalised, so a diagonal is no
//!   faster than straight ahead;
//! - the player is in water when its box, less 0.4 at the top and the bottom,
//!   touches water, as in older Minecraft;
//! - a ladder caps a fall at 0.2 a tick, and climbing ends a tick at 0.2 up;
//! - flying, Space adds 0.15, Shift takes 0.22, and neither keeps 0.375 of the
//!   vertical speed; on the tick a flight starts or stops, the drag still
//!   follows the old state.
//!
//! Velocities are in blocks a tick. [`Body::velocity`] is the velocity
//! carried into the next tick, after friction: `PlayerAuthInput`'s `delta`.

use glam::{DVec2, DVec3};

use crate::blocks::shapes::Aabb;

/// Half the player's width: it's 0.6 across.
pub const HALF_WIDTH: f64 = 0.3;
/// The player's height.
pub const HEIGHT: f64 = 1.8;
/// How far the eyes are above the feet, standing: where Bedrock reports a
/// player's position.
pub const EYE_HEIGHT: f64 = 1.62;
/// How far below the feet a player standing still looks for ground.
const GROUND_PROBE: f64 = 1e-4;
/// How high the player steps up without jumping: a slab, a stair.
pub const STEP_HEIGHT: f64 = 0.6;

/// Vertical drag, each tick.
const DRAG: f64 = 0.98;
/// Friction in the air. On the ground it's the block's slipperiness times this.
const AIR_FRICTION: f64 = 0.91;
/// Ground pushes are `movement × 0.216 / slipperiness³`: 0.216 is 0.6³, so on
/// most blocks the push is the movement speed itself.
const GROUND_PUSH: f64 = 0.216;
/// The push in the air, and in water or lava.
const AIR_PUSH: f64 = 0.02;
const FLUID_PUSH: f64 = 0.02;
/// The keys' push is scaled by this, as in vanilla.
const INPUT_SCALE: f64 = 0.98;
const SPRINT: f64 = 1.3;
/// A jump's first upward speed, before the block's jump factor.
const JUMP: f64 = 0.42;
/// A sprinting jump's push forward.
const SPRINT_JUMP_PUSH: f64 = 0.2;
const WATER_FRICTION: f64 = 0.8;
const LAVA_FRICTION: f64 = 0.5;
/// Holding jump in a liquid, each tick.
const SWIM_UP: f64 = 0.04;
/// Climbing out of a liquid against a wall, if there's this much room above.
const LEDGE_CLIMB: f64 = 0.3;
const LEDGE_ROOM: f64 = 0.6;
/// A ladder's climb, and its cap on falling, each tick.
const LADDER_SPEED: f64 = 0.2;
/// Flying: Space and Shift push by these times the flying speed (0.15 and
/// 0.22 at Bedrock's 0.05, measured from `fly.bin`).
const FLY_RISE: f64 = 3.0;
const FLY_SINK: f64 = 4.4;
/// Flying with neither held, this much vertical speed stays before the move.
const FLY_COAST: f64 = 0.375;
/// Flying, the vertical speed is multiplied by this after the move.
const FLY_DRAG: f64 = 0.6;
/// Sprinting doubles the flying speed.
const FLY_SPRINT: f64 = 2.0;
/// How far a sneaking player's step is shortened at a time, to keep it on
/// its edge.
const EDGE_STEP: f64 = 0.05;
/// A collision this much shorter than the move counts, horizontally.
const HORIZONTAL_TOLERANCE: f64 = 1e-5;

/// A liquid the player can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    Water,
    Lava,
}

/// How a block treats what touches it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    /// 0.6 for most blocks; ice 0.98, slime 0.8.
    pub slipperiness: f64,
    /// Soul sand and honey slow walking to 0.4.
    pub speed_factor: f64,
    /// Honey halves a jump.
    pub jump_factor: f64,
    /// Ladders, vines and the like.
    pub climbable: bool,
    pub fluid: Option<Fluid>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            slipperiness: 0.6,
            speed_factor: 1.0,
            jump_factor: 1.0,
            climbable: false,
            fluid: None,
        }
    }
}

/// What physics asks of the world around the player.
pub trait Terrain {
    /// Adds the collision boxes of the block at `(x, y, z)`, in world blocks.
    fn collision_boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>);
    /// How the block at `(x, y, z)` treats the player.
    fn surface(&self, x: i32, y: i32, z: i32) -> Surface;
    /// Whether the blocks around `feet` have arrived. Until they have, the
    /// player waits, so a join can't fall through the world.
    fn ready(&self, feet: DVec3) -> bool;
}

/// The player's body, as physics moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// The middle of the bottom of the box.
    pub feet: DVec3,
    /// Carried into the next tick: what `PlayerAuthInput` reports as `delta`.
    pub velocity: DVec3,
    /// Resting on something at the end of the last tick.
    pub on_ground: bool,
}

/// What the player asks of this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// Bedrock's `move_vec`: x to the left, y forward, at most 1 long, and
    /// already scaled down while sneaking.
    pub move_vec: DVec2,
    pub yaw: f32,
    /// Jump held: jumps from the ground, swims up, climbs, and flies up.
    pub jump: bool,
    /// Shift held while flying: flies down.
    pub descend: bool,
    /// Sneaking: stops at edges.
    pub sneaking: bool,
    pub sprinting: bool,
}

/// What the player's state says about how it moves this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// Flying now: this tick's push follows it.
    pub flying: bool,
    /// Flying when the tick began: its drag follows that. The two differ on
    /// the tick a flight starts or stops.
    pub was_flying: bool,
    /// A spectator, which passes through everything.
    pub no_clip: bool,
    /// `minecraft:movement`: walking speed.
    pub movement: f64,
    /// `minecraft:gravity`.
    pub gravity: f64,
    /// The abilities' flying speed.
    pub fly_speed: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            flying: false,
            was_flying: false,
            no_clip: false,
            movement: 0.1,
            gravity: 0.08,
            fly_speed: 0.05,
        }
    }
}

/// What a tick did, for the input's flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Something stopped the move along x or z.
    pub horizontal_collision: bool,
    /// Something stopped it along y, or the player stood still on something.
    pub vertical_collision: bool,
    /// It jumped from the ground.
    pub jumped: bool,
}

/// The player's box, with its feet at `feet`.
#[must_use]
pub fn player_box(feet: DVec3) -> Aabb {
    Aabb::new(
        feet - DVec3::new(HALF_WIDTH, 0.0, HALF_WIDTH),
        feet + DVec3::new(HALF_WIDTH, HEIGHT, HALF_WIDTH),
    )
}

/// One tick of physics: moves `body` as `intent` and `rules` say.
pub fn step(body: &mut Body, intent: &Intent, rules: &Rules, terrain: &impl Terrain) -> Outcome {
    let start = *body;
    let below = surface_below(terrain, start.feet);
    let fluid = if rules.no_clip {
        None
    } else {
        fluid_at(terrain, player_box(start.feet))
    };
    let climbable = !rules.flying && !rules.no_clip && {
        let at = block_at(start.feet);
        terrain.surface(at.0, at.1, at.2).climbable
    };
    let mut velocity = start.velocity;
    let mut jumped = false;

    // Up and down.
    if rules.flying {
        let fly = rules.fly_speed;
        velocity.y = match (intent.jump, intent.descend) {
            (true, false) => FLY_RISE.mul_add(fly, velocity.y),
            (false, true) => FLY_SINK.mul_add(-fly, velocity.y),
            _ => velocity.y * FLY_COAST,
        };
    } else if fluid.is_some() {
        if intent.jump {
            velocity.y += SWIM_UP;
        }
    } else if start.on_ground && intent.jump {
        velocity.y = JUMP * below.jump_factor;
        if intent.sprinting {
            velocity += facing(intent.yaw) * SPRINT_JUMP_PUSH;
        }
        jumped = true;
    }

    // Along: the keys' push.
    let sprint = if intent.sprinting { SPRINT } else { 1.0 };
    let speed = if rules.flying {
        rules.fly_speed * if intent.sprinting { FLY_SPRINT } else { 1.0 }
    } else if fluid.is_some() {
        FLUID_PUSH
    } else if start.on_ground {
        rules.movement * sprint * GROUND_PUSH / below.slipperiness.powi(3)
    } else {
        AIR_PUSH * sprint
    };
    velocity += push(intent.move_vec, intent.yaw) * (INPUT_SCALE * speed);

    if climbable {
        velocity.y = velocity.y.max(-LADDER_SPEED);
    }

    // The move.
    let from = player_box(start.feet);
    let mut motion = velocity;
    if intent.sneaking && start.on_ground && !rules.flying && !rules.no_clip && motion.y <= 0.0 {
        motion = back_off_from_edge(terrain, from, motion);
    }
    let moved = if rules.no_clip {
        motion
    } else {
        move_box(terrain, from, motion, start.on_ground)
    };
    body.feet += moved;

    let stopped_x = (moved.x - motion.x).abs() > HORIZONTAL_TOLERANCE;
    let stopped_z = (moved.z - motion.z).abs() > HORIZONTAL_TOLERANCE;
    let stopped_y = moved.y != motion.y;
    // Standing still on something counts as a vertical collision too: the
    // official client reports one on the first tick after a teleport onto
    // the ground, when nothing has moved yet.
    let resting = motion.y == 0.0
        && !rules.flying
        && !rules.no_clip
        && touches_anything(terrain, player_box(body.feet).offset(DVec3::new(0.0, -GROUND_PROBE, 0.0)));
    body.on_ground = (stopped_y && motion.y < 0.0) || resting;
    if stopped_x {
        velocity.x = 0.0;
    }
    if stopped_z {
        velocity.z = 0.0;
    }
    if stopped_y {
        velocity.y = 0.0;
    }

    // Soul sand and honey slow the walk.
    let factor = speed_factor(terrain, body.feet);
    velocity.x *= factor;
    velocity.z *= factor;

    // The velocity decays, by the state the tick began in.
    let ground_friction = if start.on_ground {
        below.slipperiness * AIR_FRICTION
    } else {
        AIR_FRICTION
    };
    match fluid {
        Some(fluid) if !rules.was_flying => {
            let (friction, sink) = match fluid {
                Fluid::Water => (WATER_FRICTION, rules.gravity / 16.0),
                Fluid::Lava => (LAVA_FRICTION, rules.gravity / 4.0),
            };
            velocity *= friction;
            velocity.y -= sink;
            // Against a wall, with room 0.6 higher, the player climbs out:
            // vanilla checks the box where the move ended, carried on by the
            // new velocity, and raised 0.6 above where the tick began.
            let room = DVec3::new(moved.x + velocity.x, velocity.y + LEDGE_ROOM, moved.z + velocity.z);
            if (stopped_x || stopped_z) && is_free(terrain, from.offset(room)) {
                velocity.y = LEDGE_CLIMB;
            }
        }
        _ if rules.was_flying => {
            velocity.x *= ground_friction;
            velocity.z *= ground_friction;
            velocity.y *= FLY_DRAG;
        }
        _ => {
            velocity.y = (velocity.y - rules.gravity) * DRAG;
            velocity.x *= ground_friction;
            velocity.z *= ground_friction;
        }
    }
    if climbable && fluid.is_none() && (stopped_x || stopped_z || intent.jump) {
        velocity.y = LADDER_SPEED;
    }
    body.velocity = velocity;
    single_precision(body);

    Outcome {
        horizontal_collision: stopped_x || stopped_z,
        vertical_collision: stopped_y || resting,
        jumped,
    }
}

/// Rounds the body as the official client keeps it (worth teaching).
///
/// Bedrock stores the player's position, at eye height, and its velocity in
/// single precision, so each tick's move lands on an `f32`. Near y 200 those
/// are 0.000015 apart, and the steps add up: the official client climbs a
/// ladder 0.000035 short of its top over 15 ticks, so it's still touching
/// the ladder there, and it sinks through water a little slower than exact
/// arithmetic would. Rounding the same way here, the replays of its traces
/// land and let go on the same ticks it does.
///
/// Standing on something, the height rounds up rather than to the nearest:
/// the official client rests at 201.62001 on a floor whose top is 200, the
/// next `f32` above 201.62, never just inside the floor.
fn single_precision(body: &mut Body) {
    let exact = body.feet.y + EYE_HEIGHT;
    let mut eyes_y = exact as f32;
    if body.on_ground && f64::from(eyes_y) < exact {
        eyes_y = eyes_y.next_up();
    }
    body.feet = DVec3::new(
        f64::from(body.feet.x as f32),
        f64::from(eyes_y) - EYE_HEIGHT,
        f64::from(body.feet.z as f32),
    );
    body.velocity = body.velocity.as_vec3().as_dvec3();
}

/// The keys' push for `move_vec`, turned to face `yaw`: forward goes along
/// the yaw, and left is to its left.
fn push(move_vec: DVec2, yaw: f32) -> DVec3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    let (left, forward) = (move_vec.x, move_vec.y);
    DVec3::new(left * cos - forward * sin, 0.0, forward * cos + left * sin)
}

/// Forward along `yaw`, along the ground.
fn facing(yaw: f32) -> DVec3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    DVec3::new(-sin, 0.0, cos)
}

/// The block a position is in.
fn block_at(position: DVec3) -> (i32, i32, i32) {
    let block = position.floor();
    (block.x as i32, block.y as i32, block.z as i32)
}

/// The block under the feet that sets friction and jumping: half a block
/// down, as in vanilla, so a slab counts for a player standing on it.
fn surface_below(terrain: &impl Terrain, feet: DVec3) -> Surface {
    let (x, y, z) = block_at(feet - DVec3::new(0.0, 0.500_000_1, 0.0));
    terrain.surface(x, y, z)
}

/// The speed factor where the player stands: the block it's in, or else the
/// one under it.
fn speed_factor(terrain: &impl Terrain, feet: DVec3) -> f64 {
    let (x, y, z) = block_at(feet);
    let at = terrain.surface(x, y, z);
    if at.speed_factor != 1.0 || at.fluid.is_some() {
        at.speed_factor
    } else {
        surface_below(terrain, feet).speed_factor
    }
}

/// The blocks a box touches, as `(x, y, z)` ranges, inclusive.
fn cells(area: Aabb) -> impl Iterator<Item = (i32, i32, i32)> {
    let low = area.min.floor();
    // A box ending exactly on a block's edge doesn't reach into it.
    let high = (area.max - DVec3::splat(1e-9)).floor();
    let (x0, y0, z0) = (low.x as i32, low.y as i32, low.z as i32);
    let (x1, y1, z1) = (high.x as i32, high.y as i32, high.z as i32);
    (x0..=x1).flat_map(move |x| (y0..=y1).flat_map(move |y| (z0..=z1).map(move |z| (x, y, z))))
}

/// The liquid the player is in, if any (worth teaching).
///
/// Bedrock decides this the way older Minecraft did: the player's box,
/// 0.4 shorter at both ends, must touch a liquid block. So a player floats
/// out of water once its feet are about 0.6 below the surface, and the
/// `water_exit` trace leaves the water between feet heights 203.51 and
/// 203.63 below a surface at 204. Lava also narrows the box by 0.1 at the
/// sides.
fn fluid_at(terrain: &impl Terrain, player: Aabb) -> Option<Fluid> {
    let water = player.shrunk(DVec3::new(0.001, 0.401, 0.001));
    if cells(water).any(|(x, y, z)| terrain.surface(x, y, z).fluid == Some(Fluid::Water)) {
        return Some(Fluid::Water);
    }
    let lava = player.shrunk(DVec3::new(0.1, 0.4, 0.1));
    cells(lava)
        .any(|(x, y, z)| terrain.surface(x, y, z).fluid == Some(Fluid::Lava))
        .then_some(Fluid::Lava)
}

/// The collision boxes that could stop a box in `area`: the blocks it
/// touches and one more all round, since fences and walls reach 0.5 above
/// their block.
fn boxes_near(terrain: &impl Terrain, area: Aabb, out: &mut Vec<Aabb>) {
    out.clear();
    let grown = Aabb::new(area.min - DVec3::ONE, area.max + DVec3::ONE);
    let mut found = Vec::new();
    for (x, y, z) in cells(grown) {
        terrain.collision_boxes(x, y, z, &mut found);
    }
    out.extend(found.into_iter().filter(|candidate| candidate.intersects(&area.shrunk(DVec3::splat(-1e-6)))));
}

/// Whether any collision box overlaps `area`.
fn touches_anything(terrain: &impl Terrain, area: Aabb) -> bool {
    let mut boxes = Vec::new();
    boxes_near(terrain, area, &mut boxes);
    boxes.iter().any(|candidate| candidate.intersects(&area))
}

/// Whether `area` holds no collision box and no liquid.
fn is_free(terrain: &impl Terrain, area: Aabb) -> bool {
    !touches_anything(terrain, area) && cells(area).all(|(x, y, z)| terrain.surface(x, y, z).fluid.is_none())
}

/// How far a box gets on a move by `motion`, axis by axis: y first, then
/// the larger of x and z, then the other, as vanilla does.
fn collide(terrain: &impl Terrain, from: Aabb, motion: DVec3) -> DVec3 {
    if motion == DVec3::ZERO {
        return motion;
    }
    let mut boxes = Vec::new();
    boxes_near(terrain, from.expanded_towards(motion), &mut boxes);
    let order = if motion.x.abs() < motion.z.abs() { [1, 2, 0] } else { [1, 0, 2] };
    let mut moving = from;
    let mut moved = DVec3::ZERO;
    for axis in order {
        let mut distance = motion[axis];
        if distance == 0.0 {
            continue;
        }
        for other in &boxes {
            // As vanilla: a move this short next to a box is no move.
            if distance.abs() < 1e-7 {
                distance = 0.0;
                break;
            }
            distance = moving.clip(other, axis, distance);
        }
        let mut step = DVec3::ZERO;
        step[axis] = distance;
        moving = moving.offset(step);
        moved[axis] = distance;
    }
    moved
}

fn horizontal_length_squared(v: DVec3) -> f64 {
    v.x.mul_add(v.x, v.z * v.z)
}

/// The move, stepping up onto anything [`STEP_HEIGHT`] high or lower when
/// walking into it (vanilla's `Entity.collide`).
///
/// If the plain move was stopped sideways, it tries again 0.6 higher: up and
/// across together, and up first then across. If either gets further along
/// the ground, it comes back down as far as it can.
fn move_box(terrain: &impl Terrain, from: Aabb, motion: DVec3, on_ground: bool) -> DVec3 {
    let moved = collide(terrain, from, motion);
    let stopped_sideways = moved.x != motion.x || moved.z != motion.z;
    let grounded = on_ground || (moved.y != motion.y && motion.y < 0.0);
    if !(grounded && stopped_sideways) {
        return moved;
    }
    let across = DVec3::new(motion.x, 0.0, motion.z);
    let mut stepped = collide(terrain, from, DVec3::new(motion.x, STEP_HEIGHT, motion.z));
    let up = collide(terrain, from.expanded_towards(across), DVec3::new(0.0, STEP_HEIGHT, 0.0));
    if up.y < STEP_HEIGHT {
        let over = collide(terrain, from.offset(up), across) + up;
        if horizontal_length_squared(over) > horizontal_length_squared(stepped) {
            stepped = over;
        }
    }
    if horizontal_length_squared(stepped) > horizontal_length_squared(moved) {
        let down = collide(terrain, from.offset(stepped), DVec3::new(0.0, motion.y - stepped.y, 0.0));
        return stepped + down;
    }
    moved
}

/// Shortens a sneaking player's move so it doesn't step off an edge it
/// couldn't step back up (vanilla's `maybeBackOffFromEdge`): while the box,
/// moved and lowered by a step, would touch nothing, the move shrinks by
/// 0.05.
fn back_off_from_edge(terrain: &impl Terrain, from: Aabb, motion: DVec3) -> DVec3 {
    let free = |dx: f64, dz: f64| !touches_anything(terrain, from.offset(DVec3::new(dx, -STEP_HEIGHT, dz)));
    let shorter = |d: f64| {
        if d.abs() < EDGE_STEP {
            0.0
        } else {
            d - EDGE_STEP.copysign(d)
        }
    };
    let (mut x, mut z) = (motion.x, motion.z);
    while x != 0.0 && free(x, 0.0) {
        x = shorter(x);
    }
    while z != 0.0 && free(0.0, z) {
        z = shorter(z);
    }
    while x != 0.0 && z != 0.0 && free(x, z) {
        x = shorter(x);
        z = shorter(z);
    }
    DVec3::new(x, motion.y, z)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A block in a test world.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Kind {
        Air,
        Stone,
        Ice,
        /// A bottom slab.
        Slab,
        Water,
        /// A ladder hanging on the north face of the block south of it.
        LadderNorth,
    }

    /// A world built from a function of the block position.
    pub struct World<F: Fn(i32, i32, i32) -> Kind>(pub F);

    impl<F: Fn(i32, i32, i32) -> Kind> Terrain for World<F> {
        fn collision_boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>) {
            let corner = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
            match (self.0)(x, y, z) {
                Kind::Stone | Kind::Ice => out.push(Aabb::cube(x, y, z)),
                Kind::Slab => out.push(Aabb::new(corner, corner + DVec3::new(1.0, 0.5, 1.0))),
                Kind::LadderNorth => out.push(Aabb::new(
                    corner + DVec3::new(0.0, 0.0, 13.0 / 16.0),
                    corner + DVec3::ONE,
                )),
                Kind::Air | Kind::Water => {}
            }
        }

        fn surface(&self, x: i32, y: i32, z: i32) -> Surface {
            match (self.0)(x, y, z) {
                Kind::Ice => Surface {
                    slipperiness: 0.98,
                    ..Surface::default()
                },
                Kind::Water => Surface {
                    fluid: Some(Fluid::Water),
                    ..Surface::default()
                },
                Kind::LadderNorth => Surface {
                    climbable: true,
                    ..Surface::default()
                },
                _ => Surface::default(),
            }
        }

        fn ready(&self, _: DVec3) -> bool {
            true
        }
    }

    /// Flat stone, with its top at y 0.
    fn flat(_: i32, y: i32, _: i32) -> Kind {
        if y < 0 { Kind::Stone } else { Kind::Air }
    }

    fn standing(feet: DVec3) -> Body {
        Body {
            feet,
            velocity: DVec3::ZERO,
            on_ground: true,
        }
    }

    fn walk(yaw: f32) -> Intent {
        Intent {
            move_vec: DVec2::new(0.0, 1.0),
            yaw,
            ..Intent::default()
        }
    }

    /// Runs `ticks` ticks of the same intent, returning the last outcome.
    fn run(body: &mut Body, intent: &Intent, ticks: usize, terrain: &impl Terrain) -> Outcome {
        let mut outcome = Outcome::default();
        for _ in 0..ticks {
            outcome = step(body, intent, &Rules::default(), terrain);
        }
        outcome
    }

    #[test]
    fn a_fall_lands_on_stone_and_says_so() {
        let world = World(flat);
        let mut body = Body {
            feet: DVec3::new(0.5, 10.0, 0.5),
            velocity: DVec3::ZERO,
            on_ground: false,
        };
        let mut landed_after = None;
        for tick in 1..=40 {
            let outcome = step(&mut body, &Intent::default(), &Rules::default(), &world);
            if outcome.vertical_collision {
                landed_after = Some(tick);
                break;
            }
        }
        // Ten blocks: the first tick only gains speed, and the 18th lands,
        // as in `fall.bin`.
        assert_eq!(landed_after, Some(18));
        assert!(body.on_ground && body.feet.y.abs() < 1e-5, "{body:?}");
        assert!((body.velocity.y + 0.0784).abs() < 1e-5, "one tick of gravity, stopped by the ground");
    }

    #[test]
    fn a_wall_stops_the_player_and_says_so() {
        let world = World(|x, y, z| if x == 2 && y >= 0 { Kind::Stone } else { flat(x, y, z) });
        let mut body = standing(DVec3::new(0.5, 0.0, 0.5));
        let outcome = run(&mut body, &walk(-90.0), 20, &world);
        assert!(outcome.horizontal_collision);
        assert!((body.feet.x - 1.7).abs() < 1e-5, "against the wall: {:?}", body.feet);
        assert_eq!(body.velocity.x, 0.0);
    }

    #[test]
    fn a_slab_is_stepped_onto_and_a_full_block_isnt() {
        // Slabs from x 2 on.
        let slab = World(|x, y, z| if x >= 2 && y == 0 { Kind::Slab } else { flat(x, y, z) });
        let mut body = standing(DVec3::new(0.5, 0.0, 0.5));
        run(&mut body, &walk(-90.0), 30, &slab);
        assert!(body.feet.x > 2.5 && (body.feet.y - 0.5).abs() < 1e-5, "on the slab: {:?}", body.feet);

        let block = World(|x, y, z| if x == 2 && y == 0 { Kind::Stone } else { flat(x, y, z) });
        let mut body = standing(DVec3::new(0.5, 0.0, 0.5));
        run(&mut body, &walk(-90.0), 30, &block);
        assert!((body.feet.x - 1.7).abs() < 1e-5 && body.feet.y.abs() < 1e-5, "{:?}", body.feet);
    }

    #[test]
    fn a_jump_peaks_about_a_block_and_a_quarter_up() {
        let world = World(flat);
        let mut body = standing(DVec3::new(0.5, 0.0, 0.5));
        let jump = Intent {
            jump: true,
            ..Intent::default()
        };
        assert!(step(&mut body, &jump, &Rules::default(), &world).jumped);
        let mut peak = body.feet.y;
        for _ in 0..12 {
            step(&mut body, &Intent::default(), &Rules::default(), &world);
            peak = peak.max(body.feet.y);
        }
        assert!((peak - 1.252_24).abs() < 1e-4, "peak {peak}");
        assert!(body.on_ground);
    }

    #[test]
    fn sprinting_is_a_third_faster_than_walking() {
        let world = World(flat);
        let top_speed = |sprinting| {
            let mut body = standing(DVec3::ZERO);
            let intent = Intent { sprinting, ..walk(0.0) };
            run(&mut body, &intent, 60, &world);
            let before = body.feet.z;
            run(&mut body, &intent, 1, &world);
            body.feet.z - before
        };
        let (walking, sprinting) = (top_speed(false), top_speed(true));
        assert!((walking - 0.215_86).abs() < 1e-5, "walking {walking}");
        assert!((sprinting / walking - 1.3).abs() < 1e-4, "sprinting {sprinting}");
    }

    #[test]
    fn sneaking_stops_at_an_edge() {
        // A platform from z 0 to 3, then a drop.
        let world = World(|x, y, z| if y < 0 && z < 3 { Kind::Stone } else { flat(x, y + 10, z) });
        let mut body = standing(DVec3::new(0.5, 0.0, 0.5));
        let sneak = Intent {
            move_vec: DVec2::new(0.0, 0.3),
            sneaking: true,
            ..Intent::default()
        };
        run(&mut body, &sneak, 100, &world);
        assert!(body.on_ground && body.feet.z < 3.3 && body.feet.z > 3.0, "{:?}", body.feet);
        // Not sneaking, the same walk goes over.
        let mut body = standing(DVec3::new(0.5, 0.0, 0.5));
        run(&mut body, &walk(0.0), 40, &world);
        assert!(body.feet.y < -1.0, "{:?}", body.feet);
    }

    /// How far the player glides after letting go of the keys at full
    /// walking speed.
    fn glide(terrain: &impl Terrain) -> f64 {
        let mut body = standing(DVec3::ZERO);
        run(&mut body, &walk(0.0), 100, terrain);
        let let_go = body.feet.z;
        run(&mut body, &Intent::default(), 40, terrain);
        body.feet.z - let_go
    }

    #[test]
    fn ice_slides() {
        // Ice pushes less, so it takes longer to reach speed, but then
        // glides on: friction 0.98 × 0.91 a tick instead of 0.6 × 0.91.
        let on_ice = glide(&World(|x, y, z| if y == -1 { Kind::Ice } else { flat(x, y, z) }));
        let on_stone = glide(&World(flat));
        assert!(on_stone < 0.3 && on_ice > 1.5, "ice {on_ice}, stone {on_stone}");
    }

    #[test]
    fn water_sinks_and_jump_swims_up() {
        let pool = World(|x, y, z| if (0..10).contains(&y) { Kind::Water } else { flat(x, y, z) });
        let mut body = Body {
            feet: DVec3::new(0.5, 5.0, 0.5),
            velocity: DVec3::ZERO,
            on_ground: false,
        };
        run(&mut body, &Intent::default(), 60, &pool);
        assert!((body.velocity.y + 0.025).abs() < 1e-4, "sinks at 0.025 a tick: {:?}", body.velocity);
        let low = body.feet.y;
        run(
            &mut body,
            &Intent {
                jump: true,
                ..Intent::default()
            },
            20,
            &pool,
        );
        assert!(body.feet.y > low + 1.0, "swam up from {low} to {}", body.feet.y);
    }

    #[test]
    fn a_ladder_climbs() {
        // Ladders on the north face of a column at z 1.
        let column = World(|x, y, z| match (x, z) {
            (0, 1) if y >= 0 => Kind::Stone,
            (0, 0) if (0..5).contains(&y) => Kind::LadderNorth,
            _ => flat(x, y, z),
        });
        let mut body = standing(DVec3::new(0.5, 0.0, 0.3));
        let outcome = run(&mut body, &walk(0.0), 10, &column);
        assert!(outcome.horizontal_collision);
        assert!((body.velocity.y - 0.2).abs() < 1e-5 && body.feet.y > 1.0, "{body:?}");
    }

    #[test]
    fn flying_rises_sinks_and_coasts() {
        let world = World(flat);
        let mut body = Body {
            feet: DVec3::new(0.5, 5.0, 0.5),
            velocity: DVec3::ZERO,
            on_ground: false,
        };
        let rules = Rules {
            flying: true,
            was_flying: true,
            ..Rules::default()
        };
        let up = Intent {
            jump: true,
            ..Intent::default()
        };
        step(&mut body, &up, &rules, &world);
        assert!((body.velocity.y - 0.09).abs() < 1e-5, "{body:?}");
        step(&mut body, &up, &rules, &world);
        assert!((body.velocity.y - 0.144).abs() < 1e-5, "{body:?}");
        step(&mut body, &Intent::default(), &rules, &world);
        assert!((body.velocity.y - 0.144 * 0.375 * 0.6).abs() < 1e-5, "{body:?}");
        // No gravity: it stays up.
        let height = body.feet.y;
        run_rules(&mut body, &Intent::default(), &rules, 40, &world);
        assert!((body.feet.y - height).abs() < 0.05);
    }

    fn run_rules(body: &mut Body, intent: &Intent, rules: &Rules, ticks: usize, terrain: &impl Terrain) {
        for _ in 0..ticks {
            step(body, intent, rules, terrain);
        }
    }

    #[test]
    fn a_spectator_passes_through_stone() {
        let solid = World(|_, _, _| Kind::Stone);
        let mut body = Body {
            feet: DVec3::new(0.5, 0.0, 0.5),
            velocity: DVec3::ZERO,
            on_ground: false,
        };
        let rules = Rules {
            flying: true,
            was_flying: true,
            no_clip: true,
            ..Rules::default()
        };
        run_rules(&mut body, &walk(-90.0), &rules, 20, &solid);
        assert!(body.feet.x > 5.0, "{:?}", body.feet);
    }
}
