//! Everything in the world besides blocks and the local player: mobs, other players,
//! dropped items, minecarts and arrows (m5-plan Decision 13; worth teaching).
//!
//! **An entity-component system, a small one.** Entities don't share one shape. A dropped
//! item has a stack, a player has a name, a calf is a baby, and milestone 8 adds models
//! and animations. So each entity is an id in a `hecs::World`, with the components it
//! has: always its [`Ids`], [`Kind`] and [`Track`]; a [`Dropped`] stack for an item; a
//! [`Name`] for a player; and a [`Look`] once its data says anything. Drawing asks for the
//! entities with the components it needs, and the world keeps each kind of component in
//! its own tightly packed array, which is quick to walk. `hecs` is the small ECS: no
//! scheduler or systems, which suits the client's own loop (`bevy_ecs` would bring both,
//! and much more to compile).
//!
//! **Two ids.** Bedrock names an entity by its runtime id in every move and data packet,
//! and by its unique id when it goes (`RemoveActor`). Pumpkin uses one number for both,
//! but the client keeps a map for each, as Bedrock's protocol has them.
//!
//! **Drawn in the past** (`track.rs`): each frame draws every entity where it was a tick
//! and a margin ago, between two of the server's moves.

pub mod kinds;
pub mod track;

use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use glam::DVec3;
use pumpkin_data::entity::EntityType;
use pumpkin_protocol::bedrock::client::{
    CAddActor, CAddItemActor, CAddPlayer, CMoveActorAbsolute, CMoveActorDelta, CMovePlayer,
    common::ActorLink,
    move_actor_delta::{
        MOVE_ACTOR_DELTA_FLAG_HAS_HEAD_YAW, MOVE_ACTOR_DELTA_FLAG_HAS_PITCH, MOVE_ACTOR_DELTA_FLAG_HAS_X,
        MOVE_ACTOR_DELTA_FLAG_HAS_Y, MOVE_ACTOR_DELTA_FLAG_HAS_YAW, MOVE_ACTOR_DELTA_FLAG_HAS_Z,
    },
    set_actor_data::{MetadataValue, SyncedActorDataList, entity_data_flag, entity_data_key},
};
use pumpkin_util::math::vector3::Vector3;
use tracing::{debug, info};

use self::{
    kinds::Group,
    track::{Pose, STILL, Track},
};

/// The server's two names for an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ids {
    pub runtime: u64,
    pub unique: i64,
}

/// What an entity is.
#[derive(Clone, Copy, Debug)]
pub struct Kind {
    /// `None` for a name the client doesn't know: it's drawn as a block-sized box, and
    /// the log names it.
    pub entity_type: Option<&'static EntityType>,
    pub group: Group,
}

/// What its data says about how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub baby: bool,
    pub scale: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self { baby: false, scale: 1.0 }
    }
}

/// A dropped item's stack: the item, its data value, and how many.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub item: i16,
    pub aux: u32,
    pub count: u16,
}

/// A player's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name(pub String);

/// When the client first heard of it, for an item's spin.
#[derive(Clone, Copy, Debug)]
struct Born(Instant);

/// What it rides, by unique id, as `SetActorLink` names it. The server sends a rider no
/// position of its own, only which way it looks, so it's drawn on its vehicle's seat.
#[derive(Clone, Copy, Debug)]
struct Riding(i64);

/// Its riders by unique id, in the order they got on, which is their seats' order.
#[derive(Clone, Debug, Default)]
struct Riders(Vec<i64>);

/// Which of an entity's poses to seat a rider by.
#[derive(Clone, Copy)]
enum When {
    /// As a frame at this moment draws it.
    Drawn(Instant),
    /// The newest the server sent.
    Latest,
}

/// How deep riders can stack, a jockey on a jockey, before the client stops looking.
const MOST_STACKED: u8 = 4;

/// One entity as a frame draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub runtime_id: u64,
    pub group: Group,
    pub pose: Pose,
    /// Its box's width and height, in blocks.
    pub size: [f64; 2],
    /// A dropped item's stack, which is drawn as its icon.
    pub dropped: Option<Dropped>,
    /// How long the client has known it.
    pub age: Duration,
}

/// What the log reports about entities, to measure the drawing delay by.
#[derive(Debug, Default)]
struct Stats {
    added: u32,
    removed: u32,
    /// Names the client doesn't know, with how many of each came.
    unknown: BTreeMap<String, u32>,
    /// How far apart each entity's moves came.
    gaps: Gaps,
    /// Moves, removals and data for entities the client doesn't have.
    strays: u32,
}

/// How far apart moves came, counted by the millisecond up to [`LONGEST_GAP`], and the
/// longer ones together: every gap of a session, in a fixed space.
#[derive(Debug)]
struct Gaps {
    counts: Vec<u64>,
}

/// The longest gap counted on its own, in milliseconds.
const LONGEST_GAP: usize = 250;
/// A tick and a half, in milliseconds: a gap up to this is a moving entity's next move.
/// Up to [`LATE`], it's most likely the next move come late; up to [`STOOD`], two and a
/// half ticks, most likely one tick standing still, which sends nothing, so the next move
/// comes about 100 ms on.
const MOVING: usize = STILL.as_millis() as usize;
const LATE: usize = 90;
const STOOD: usize = 125;

impl Default for Gaps {
    fn default() -> Self {
        Self {
            counts: vec![0; LONGEST_GAP + 2],
        }
    }
}

impl Gaps {
    fn add(&mut self, gap: Duration) {
        let ms = usize::try_from(gap.as_millis()).unwrap_or(usize::MAX).min(LONGEST_GAP + 1);
        self.counts[ms] += 1;
    }

    /// How many came this many milliseconds apart; past [`LONGEST_GAP`], all the longer.
    fn count(&self, ms: std::ops::RangeInclusive<usize>) -> u64 {
        let (start, end) = (*ms.start(), (*ms.end()).min(LONGEST_GAP + 1));
        self.counts.get(start..=end).map_or(0, |counts| counts.iter().sum())
    }

    /// The gap that a fraction `p` of those up to `up_to` ms are no longer than.
    fn percentile(&self, up_to: usize, p: f64) -> Option<usize> {
        let total = self.count(0..=up_to);
        if total == 0 {
            return None;
        }
        let wanted = ((total as f64 * p).ceil() as u64).max(1);
        let mut seen = 0;
        (0..=up_to).find(|&ms| {
            seen += self.counts[ms];
            seen >= wanted
        })
    }
}

/// Every entity the server has told the client about.
pub struct Entities {
    world: hecs::World,
    by_runtime: HashMap<u64, hecs::Entity>,
    by_unique: HashMap<i64, hecs::Entity>,
    stats: Stats,
}

impl Default for Entities {
    fn default() -> Self {
        Self {
            world: hecs::World::new(),
            by_runtime: HashMap::new(),
            by_unique: HashMap::new(),
            stats: Stats::default(),
        }
    }
}

/// An angle Bedrock packs into a byte, a 256th of a turn each.
fn unpack_degrees(packed: u8) -> f32 {
    f32::from(packed) * 360.0 / 256.0
}

fn dvec(v: Vector3<f32>) -> DVec3 {
    DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
}

impl Entities {
    /// How many entities there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_runtime.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_runtime.is_empty()
    }

    /// A mob, a minecart, an arrow: anything but a player or an item.
    pub fn add_actor(&mut self, add: &CAddActor, at: Instant) {
        debug!("AddActor {} as {}, with {} links", add.actor_type, add.target_runtime_id.0, add.actor_links.len());
        let kind = self.kind(&add.actor_type);
        let feet = dvec(add.position) - DVec3::Y * kinds::network_height(kind.entity_type);
        let pose = Pose {
            feet,
            pitch: add.rotation.x,
            yaw: add.rotation.y,
            head_yaw: add.y_head_rotation,
        };
        let ids = Ids {
            runtime: add.target_runtime_id.0,
            unique: add.target_actor_id.0,
        };
        let entity = self.spawn(ids, kind, pose, at);
        self.apply_data(entity, &add.actor_data);
        for link in &add.actor_links {
            self.link(link, at);
        }
    }

    /// Another player. The local one is the game's `Player`, which never comes this way.
    pub fn add_player(&mut self, add: &CAddPlayer, at: Instant) {
        let kind = self.kind("minecraft:player");
        let feet = dvec(add.position) - DVec3::Y * kinds::network_height(kind.entity_type);
        let pose = Pose {
            feet,
            pitch: add.rotation.x,
            yaw: add.rotation.y,
            head_yaw: add.y_head_rotation,
        };
        // A player's unique id isn't in `AddPlayer`: Pumpkin's is its runtime id.
        let ids = Ids {
            runtime: add.target_runtime_id.0,
            unique: add.target_runtime_id.0 as i64,
        };
        let entity = self.spawn(ids, kind, pose, at);
        let _ = self.world.insert_one(entity, Name(add.player_name.clone()));
        self.apply_data(entity, &add.entity_data);
    }

    /// A dropped item.
    pub fn add_item(&mut self, add: &CAddItemActor, at: Instant) {
        let kind = self.kind("minecraft:item");
        let feet = dvec(add.position) - DVec3::Y * kinds::network_height(kind.entity_type);
        let pose = Pose {
            feet,
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
        };
        let ids = Ids {
            runtime: add.target_runtime_id.0,
            unique: add.target_actor_id.0,
        };
        let entity = self.spawn(ids, kind, pose, at);
        let stack = &add.item;
        let _ = self.world.insert_one(
            entity,
            Dropped {
                item: stack.id,
                aux: stack.aux_value.0,
                count: stack.stack_size,
            },
        );
    }

    fn kind(&mut self, identifier: &str) -> Kind {
        let entity_type = kinds::entity_type(identifier);
        if entity_type.is_none() {
            *self.stats.unknown.entry(identifier.to_string()).or_default() += 1;
        }
        Kind {
            entity_type,
            group: entity_type.map_or(Group::Other, Group::of),
        }
    }

    fn spawn(&mut self, ids: Ids, kind: Kind, pose: Pose, at: Instant) -> hecs::Entity {
        // The same id again means the server sent it anew: the old one is gone.
        if let Some(old) = self.by_runtime.get(&ids.runtime).copied() {
            self.despawn(old);
        }
        let entity = self.world.spawn((ids, kind, Track::new(at, pose), Born(at)));
        self.by_runtime.insert(ids.runtime, entity);
        self.by_unique.insert(ids.unique, entity);
        self.stats.added += 1;
        entity
    }

    fn despawn(&mut self, entity: hecs::Entity) {
        if let Ok(ids) = self.world.get::<&Ids>(entity).map(|ids| *ids) {
            self.get_off(ids.unique);
            self.by_runtime.remove(&ids.runtime);
            self.by_unique.remove(&ids.unique);
        }
        let _ = self.world.despawn(entity);
    }

    /// `SetActorLink`: a rider gets on (type 1, the driver, or 2, a passenger) or off
    /// (type 0). Getting off starts its own track where it sat, so it doesn't flash back
    /// to where it got on.
    pub fn link(&mut self, link: &ActorLink, at: Instant) {
        let (vehicle, rider) = (link.ridden_unique_id.0, link.rider_unique_id.0);
        if link.link_type == 0 {
            if let Some(&entity) = self.by_unique.get(&rider)
                && let Some(seated) = self.pose_of(entity, When::Latest, 0)
                && let Ok(mut track) = self.world.get::<&mut Track>(entity)
            {
                track.jumped(at, seated);
            }
            self.get_off(rider);
            return;
        }
        self.get_off(rider);
        let (Some(&on), Some(&rides)) = (self.by_unique.get(&vehicle), self.by_unique.get(&rider)) else {
            self.stats.strays += 1;
            debug!("SetActorLink of {rider} on {vehicle}, one of which the client doesn't have");
            return;
        };
        if let Ok(mut riders) = self.world.get::<&mut Riders>(on) {
            riders.0.push(rider);
        } else {
            let _ = self.world.insert_one(on, Riders(vec![rider]));
        }
        let _ = self.world.insert_one(rides, Riding(vehicle));
    }

    /// Takes a rider off whatever it rides, and its riders off it.
    fn get_off(&mut self, unique: i64) {
        let Some(&entity) = self.by_unique.get(&unique) else {
            return;
        };
        if let Ok(Riding(vehicle)) = self.world.remove_one::<Riding>(entity)
            && let Some(&on) = self.by_unique.get(&vehicle)
            && let Ok(mut riders) = self.world.get::<&mut Riders>(on)
        {
            riders.0.retain(|&other| other != unique);
        }
        if let Ok(Riders(riders)) = self.world.remove_one::<Riders>(entity) {
            for rider in riders {
                if let Some(&rides) = self.by_unique.get(&rider) {
                    let _ = self.world.remove_one::<Riding>(rides);
                }
            }
        }
    }

    /// An entity's pose: its own, or on its vehicle's seat if it rides one the client has.
    fn pose_of(&self, entity: hecs::Entity, when: When, depth: u8) -> Option<Pose> {
        let own = {
            let track = self.world.get::<&Track>(entity).ok()?;
            match when {
                When::Drawn(now) => track.drawn(now),
                When::Latest => track.latest(),
            }
        };
        let Ok(Riding(vehicle)) = self.world.get::<&Riding>(entity).map(|riding| *riding) else {
            return Some(own);
        };
        let seated = || {
            if depth >= MOST_STACKED {
                return None;
            }
            let on = *self.by_unique.get(&vehicle)?;
            let under = self.pose_of(on, when, depth + 1)?;
            let unique = self.world.get::<&Ids>(entity).ok()?.unique;
            let index = self.world.get::<&Riders>(on).ok()?.0.iter().position(|&rider| rider == unique)?;
            let seat = kinds::seat(self.riding(on, under.yaw)?, index, self.riding(entity, own.yaw)?)?;
            Some(Pose {
                feet: under.feet + seat,
                ..own
            })
        };
        Some(seated().unwrap_or(own))
    }

    /// What seating needs of an entity, turned to `yaw`.
    fn riding(&self, entity: hecs::Entity, yaw: f32) -> Option<kinds::Riding> {
        let kind = *self.world.get::<&Kind>(entity).ok()?;
        let look = self.world.get::<&Look>(entity).map_or_else(|_| Look::default(), |look| *look);
        Some(kinds::Riding {
            entity_type: kind.entity_type,
            baby: look.baby,
            scale: look.scale,
            yaw,
        })
    }

    /// `RemoveActor`, which names the unique id. Returns whether it was there.
    pub fn remove(&mut self, unique: i64) -> bool {
        let Some(entity) = self.by_unique.get(&unique).copied() else {
            self.stats.strays += 1;
            debug!("RemoveActor for {unique}, which the client doesn't have");
            return false;
        };
        self.despawn(entity);
        self.stats.removed += 1;
        true
    }

    /// Forgets every entity, as a change of dimension does.
    pub fn clear(&mut self) {
        self.world.clear();
        self.by_runtime.clear();
        self.by_unique.clear();
    }

    /// The track of the entity with this runtime id, and what it is.
    fn track_mut(&mut self, runtime: u64) -> Option<(hecs::RefMut<'_, Track>, Option<&'static EntityType>)> {
        let Some(&entity) = self.by_runtime.get(&runtime) else {
            self.stats.strays += 1;
            debug!("A move for {runtime}, which the client doesn't have");
            return None;
        };
        let entity_type = self.world.get::<&Kind>(entity).ok()?.entity_type;
        Some((self.world.get::<&mut Track>(entity).ok()?, entity_type))
    }

    fn note_gap(&mut self, gap: Duration) {
        self.stats.gaps.add(gap);
    }

    /// `MoveActorDelta`: the axes and angles that changed, each as its new value. Returns
    /// whether the entity was there.
    pub fn move_delta(&mut self, moved: &CMoveActorDelta, at: Instant) -> bool {
        let Some((mut track, entity_type)) = self.track_mut(moved.entity_runtime_id.0) else {
            return false;
        };
        let has = |flag: u16| moved.flags & flag != 0;
        let mut pose = track.latest();
        let height = kinds::network_height(entity_type);
        if has(MOVE_ACTOR_DELTA_FLAG_HAS_X) {
            pose.feet.x = f64::from(moved.x);
        }
        if has(MOVE_ACTOR_DELTA_FLAG_HAS_Y) {
            pose.feet.y = f64::from(moved.y) - height;
        }
        if has(MOVE_ACTOR_DELTA_FLAG_HAS_Z) {
            pose.feet.z = f64::from(moved.z);
        }
        if has(MOVE_ACTOR_DELTA_FLAG_HAS_PITCH) {
            pose.pitch = unpack_degrees(moved.pitch);
        }
        if has(MOVE_ACTOR_DELTA_FLAG_HAS_YAW) {
            pose.yaw = unpack_degrees(moved.yaw);
        }
        if has(MOVE_ACTOR_DELTA_FLAG_HAS_HEAD_YAW) {
            pose.head_yaw = unpack_degrees(moved.head_yaw);
        }
        let gap = track.moved(at, pose);
        drop(track);
        self.note_gap(gap);
        true
    }

    /// `MoveActorAbsolute`: the whole pose, and a teleport if it says so.
    pub fn move_absolute(&mut self, moved: &CMoveActorAbsolute, at: Instant) -> bool {
        let Some((mut track, entity_type)) = self.track_mut(moved.actor_runtime_id.0) else {
            return false;
        };
        let pose = Pose {
            feet: dvec(moved.position) - DVec3::Y * kinds::network_height(entity_type),
            pitch: unpack_degrees(moved.rotation_x),
            yaw: unpack_degrees(moved.rotation_y),
            head_yaw: unpack_degrees(moved.rotation_y_head),
        };
        if moved.header & CMoveActorAbsolute::FLAG_TELEPORT != 0 {
            track.jumped(at, pose);
        } else {
            let gap = track.moved(at, pose);
            drop(track);
            self.note_gap(gap);
        }
        true
    }

    /// `MovePlayer` about another player.
    pub fn move_player(&mut self, moved: &CMovePlayer, at: Instant) -> bool {
        let Some((mut track, entity_type)) = self.track_mut(moved.player_runtime_id.0) else {
            return false;
        };
        let pose = Pose {
            feet: dvec(moved.position) - DVec3::Y * kinds::network_height(entity_type),
            pitch: moved.pitch,
            yaw: moved.yaw,
            head_yaw: moved.head_yaw,
        };
        if moved.mode == CMovePlayer::MODE_NORMAL {
            let gap = track.moved(at, pose);
            drop(track);
            self.note_gap(gap);
        } else {
            track.jumped(at, pose);
        }
        true
    }

    /// `SetActorData` about an entity: whether it's a baby, and its scale. Returns whether
    /// the entity was there.
    pub fn set_data(&mut self, runtime: u64, data: &SyncedActorDataList) -> bool {
        let Some(&entity) = self.by_runtime.get(&runtime) else {
            self.stats.strays += 1;
            debug!("SetActorData for {runtime}, which the client doesn't have");
            return false;
        };
        self.apply_data(entity, data);
        true
    }

    fn apply_data(&mut self, entity: hecs::Entity, data: &SyncedActorDataList) {
        let flags = data.0.get(&entity_data_key::FLAGS);
        let scale = data.0.get(&entity_data_key::SCALE);
        if flags.is_none() && scale.is_none() {
            return;
        }
        let mut look = self.world.get::<&Look>(entity).map_or_else(|_| Look::default(), |look| *look);
        if let Some(MetadataValue::Int64(flags)) = flags {
            look.baby = flags & (1 << entity_data_flag::BABY) != 0;
        }
        if let Some(&MetadataValue::Float(scale)) = scale {
            look.scale = scale;
        }
        let _ = self.world.insert_one(entity, look);
    }

    /// Every entity as a frame at `now` draws it.
    #[must_use]
    pub fn drawn(&self, now: Instant) -> Vec<Drawn> {
        let mut query = self
            .world
            .query::<(hecs::Entity, &Ids, &Kind, &Track, &Born, Option<&Look>, Option<&Dropped>)>();
        query
            .iter()
            .map(|(entity, ids, kind, track, born, look, dropped)| {
                let look = look.copied().unwrap_or_default();
                Drawn {
                    runtime_id: ids.runtime,
                    group: kind.group,
                    pose: self.pose_of(entity, When::Drawn(now), 0).unwrap_or_else(|| track.drawn(now)),
                    size: kinds::size(kind.entity_type, look.baby, look.scale),
                    dropped: dropped.copied(),
                    age: now.saturating_duration_since(born.0),
                }
            })
            .collect()
    }

    /// Forgets the poses no frame will want again.
    pub fn forget_old(&mut self, now: Instant) {
        for track in self.world.query_mut::<&mut Track>() {
            track.forget_old(now);
        }
    }

    /// Each entity's runtime id, type and newest pose, for the headless check.
    #[must_use]
    pub fn latest(&self) -> Vec<(u64, Option<&'static EntityType>, Pose)> {
        let mut query = self.world.query::<(hecs::Entity, &Ids, &Kind, &Track)>();
        query
            .iter()
            .map(|(entity, ids, kind, track)| {
                let pose = self.pose_of(entity, When::Latest, 0).unwrap_or_else(|| track.latest());
                (ids.runtime, kind.entity_type, pose)
            })
            .collect()
    }

    /// Logs what came: how many, the names the client doesn't know, and how far apart
    /// moves arrive, which is what the drawing delay has to cover.
    pub fn log(&self) {
        let stats = &self.stats;
        if self.is_empty() && stats.added == 0 {
            info!("Entities: none came");
            return;
        }
        info!(
            "Entities: {} now, {} added and {} removed; {} packets about entities the client didn't have",
            self.len(),
            stats.added,
            stats.removed,
            stats.strays
        );
        if !stats.unknown.is_empty() {
            let names: Vec<String> = stats.unknown.iter().map(|(name, count)| format!("{name} ({count})")).collect();
            info!("Entities the client doesn't know, drawn as a block: {}", names.join(", "));
        }
        let gaps = &stats.gaps;
        if let (Some(median), Some(p95), Some(p99)) =
            (gaps.percentile(MOVING, 0.5), gaps.percentile(MOVING, 0.95), gaps.percentile(MOVING, 0.99))
        {
            let moving = gaps.count(0..=MOVING);
            let late = gaps.count(MOVING + 1..=LATE);
            let stood = gaps.count(LATE + 1..=STOOD);
            let still = gaps.count(STOOD + 1..=LONGEST_GAP + 1);
            info!(
                "Moves while moving came {median} ms apart at the median, {p95} at the 95th percentile and {p99} at the \
                 99th, over {moving} moves; {late} more came {MOVING} to {LATE} ms apart, late, which is {:.2}% of \
                 them, against a drawing delay of {} ms. {stood} came {LATE} to {STOOD} ms apart, after a tick standing \
                 still, and {still} after standing still longer",
                100.0 * late as f64 / (moving + late) as f64,
                track::DELAY.as_millis(),
            );
        }
    }
}

#[cfg(test)]
mod tests;
