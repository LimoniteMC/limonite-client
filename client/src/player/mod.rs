//! The local player (m5-plan Decision 2): who the server says it is, what it
//! may do, and where it is, tick by tick, moved by physics (Step 6).

pub mod movement;
pub mod physics;
pub mod terrain;
#[cfg(test)]
mod traces;

use glam::DVec3;
use pumpkin_protocol::bedrock::client::{
    Ability, AttributeData, GameType, PlayerPermissionLevel, SerializedAbilitiesData,
};
use pumpkin_util::GameMode;
use tracing::debug;

use self::{
    movement::{Keys, Movement, Report, Situation},
    physics::{Body, Terrain},
};

pub use self::physics::EYE_HEIGHT;

/// Where the player is, and where it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// The feet: the middle of the bottom of the player's box, in blocks.
    pub feet: DVec3,
    /// Bedrock's angles, in degrees (`render/camera.rs`).
    pub pitch: f32,
    pub yaw: f32,
}

impl Pose {
    /// The pose whose eyes are at `eyes`. Bedrock sends a player's position
    /// at eye height.
    #[must_use]
    pub fn from_eyes(eyes: DVec3, pitch: f32, yaw: f32) -> Self {
        Self {
            feet: eyes - DVec3::Y * EYE_HEIGHT,
            pitch,
            yaw,
        }
    }

    #[must_use]
    pub fn eyes(&self) -> DVec3 {
        self.feet + DVec3::Y * EYE_HEIGHT
    }
}

/// What moves the player for one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// No keys: physics alone, so it stands, falls or drifts to a stop.
    Still,
    /// The keys, and where the camera looks, under physics.
    Keys { keys: Keys, pitch: f32, yaw: f32 },
    /// Straight to a pose, past physics: the bench and the scripted session
    /// move the player themselves.
    To(Pose),
}

/// What the server lets the player do (`UpdateAbilities`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abilities {
    pub build: bool,
    pub mine: bool,
    pub invulnerable: bool,
    pub flying: bool,
    pub may_fly: bool,
    /// Breaks blocks at once, as in creative.
    pub instant_build: bool,
    /// Passes through blocks, as a spectator.
    pub no_clip: bool,
    pub operator_commands: bool,
    /// Bedrock's speeds: 0.05 flying, 1 for flying up and down, 0.1 walking.
    pub fly_speed: f32,
    pub vertical_fly_speed: f32,
    pub walk_speed: f32,
    /// How the server ranks the player: operator, member or visitor.
    pub permissions: PlayerPermissionLevel,
}

impl Default for Abilities {
    /// A survival player's, until the server says.
    fn default() -> Self {
        Self {
            build: true,
            mine: true,
            invulnerable: false,
            flying: false,
            may_fly: false,
            instant_build: false,
            no_clip: false,
            operator_commands: false,
            fly_speed: 0.05,
            vertical_fly_speed: 1.0,
            walk_speed: 0.1,
            permissions: PlayerPermissionLevel::Member,
        }
    }
}

impl Abilities {
    /// Reads the server's ability layers (worth teaching).
    ///
    /// A layer says which abilities it sets (`abilities_set`, one bit each)
    /// and what to (`ability_value`). Each ability takes its value from the
    /// last layer that sets it. Pumpkin sends a base layer that sets every
    /// ability, and for a spectator a second layer that sets flying on.
    #[must_use]
    pub fn from_data(data: &SerializedAbilitiesData) -> Self {
        let bit = |ability: Ability| 1_u32 << ability as u32;
        let mut abilities = Self {
            permissions: data.player_permissions,
            ..Self::default()
        };
        let mut values = 0_u32;
        for layer in &data.layers {
            let set = layer.abilities_set;
            // Keep the bits this layer doesn't set, and take the ones it does.
            values = (values & !set) | (layer.ability_value & set);
            if set & bit(Ability::FlySpeed) != 0 {
                abilities.fly_speed = layer.fly_speed;
            }
            if set & bit(Ability::VerticalFlySpeed) != 0 {
                abilities.vertical_fly_speed = layer.vertical_fly_speed;
            }
            if set & bit(Ability::WalkSpeed) != 0 {
                abilities.walk_speed = layer.walk_speed;
            }
        }
        let has = |ability: Ability| values & bit(ability) != 0;
        Self {
            build: has(Ability::Build),
            mine: has(Ability::Mine),
            invulnerable: has(Ability::Invulnerable),
            flying: has(Ability::Flying),
            may_fly: has(Ability::MayFly),
            instant_build: has(Ability::Instabuild),
            no_clip: has(Ability::NoClip),
            operator_commands: has(Ability::OperatorCommands),
            ..abilities
        }
    }
}

/// What the server says about the player's body (`UpdateAttributes`, `SetHealth`,
/// `SetActorData`), with vanilla's values until it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attributes {
    /// `minecraft:movement`: the walking speed.
    pub movement: f32,
    /// `minecraft:underwater_movement`.
    pub underwater_movement: f32,
    /// `minecraft:gravity`, in blocks a tick, each tick.
    pub gravity: f32,
    /// `minecraft:health`, and the most it can be.
    pub health: f32,
    pub max_health: f32,
    /// `minecraft:player.hunger`, 0 to 20, and `minecraft:player.saturation`.
    pub hunger: f32,
    pub saturation: f32,
    /// Breath, in ticks: `AIR_SUPPLY` in `SetActorData`, not an attribute.
    pub air: f32,
    /// `minecraft:player.experience`, how far through this level, 0 to 1, and
    /// `minecraft:player.level`.
    pub experience: f32,
    pub level: f32,
}

impl Default for Attributes {
    fn default() -> Self {
        Self {
            movement: 0.1,
            underwater_movement: 0.02,
            gravity: 0.08,
            health: 20.0,
            max_health: 20.0,
            hunger: 20.0,
            saturation: 5.0,
            air: 300.0,
            experience: 0.0,
            level: 0.0,
        }
    }
}

impl Attributes {
    /// Takes the values in `list`. Ones the client doesn't keep are skipped.
    pub fn update(&mut self, list: &[AttributeData]) {
        for attribute in list {
            let value = attribute.current_value;
            match attribute.name.as_str() {
                "minecraft:movement" => self.movement = value,
                "minecraft:underwater_movement" => self.underwater_movement = value,
                "minecraft:gravity" => self.gravity = value,
                "minecraft:health" => {
                    self.health = value;
                    self.max_health = attribute.max_value;
                }
                "minecraft:player.hunger" => self.hunger = value,
                "minecraft:player.saturation" => self.saturation = value,
                "minecraft:player.experience" => self.experience = value,
                "minecraft:player.level" => self.level = value,
                other => debug!("Attribute {other} = {value}: not kept"),
            }
        }
    }
}

/// The gamemode a `GameType` stands for. `Default` is the world's own.
#[must_use]
pub fn gamemode(game_type: GameType, world: GameType) -> Option<GameMode> {
    match game_type {
        GameType::Survival => Some(GameMode::Survival),
        GameType::Creative => Some(GameMode::Creative),
        GameType::Adventure => Some(GameMode::Adventure),
        GameType::Spectator => Some(GameMode::Spectator),
        GameType::Default if world != GameType::Default => gamemode(world, GameType::Unknown),
        GameType::Default | GameType::Unknown => None,
    }
}

/// The local player.
#[derive(Clone, Debug)]
pub struct Player {
    /// What packets about entities call it.
    pub runtime_id: u64,
    /// What `UpdateAbilities` calls it.
    pub unique_id: i64,
    pub gamemode: GameMode,
    pub abilities: Abilities,
    pub attributes: Attributes,
    /// In blocks a tick, carried into the next tick: physics moves the player
    /// by it, and the server can set it (`SetActorMotion`).
    pub velocity: DVec3,
    /// Resting on something at the end of the last tick.
    pub on_ground: bool,
    /// Flying, sprinting, sneaking, and what the keys were.
    pub movement: Movement,
    /// What the last tick's input reports.
    report: Report,
    pose: Pose,
    /// The pose a tick ago, which frames draw from.
    previous: Pose,
    /// A teleport arrived: the next input says it was handled, and the player
    /// holds still for that tick, as the official client does.
    teleported: bool,
    /// Angles the server turned the player to, for the camera to take.
    turned: Option<(f32, f32)>,
}

impl Player {
    #[must_use]
    pub fn new(runtime_id: u64, unique_id: i64, gamemode: GameMode, pose: Pose) -> Self {
        Self {
            runtime_id,
            unique_id,
            gamemode,
            abilities: Abilities::default(),
            attributes: Attributes::default(),
            velocity: DVec3::ZERO,
            on_ground: false,
            movement: Movement::default(),
            report: Report::default(),
            pose,
            previous: pose,
            teleported: false,
            turned: None,
        }
    }

    /// What the last tick's input reports.
    #[must_use]
    pub const fn report(&self) -> &Report {
        &self.report
    }

    /// This tick's pose.
    #[must_use]
    pub const fn pose(&self) -> Pose {
        self.pose
    }

    /// The eyes, `alpha` of the way from the last tick's pose to this one's
    /// (`clock.rs`): where a frame draws them.
    #[must_use]
    pub fn eyes_at(&self, alpha: f64) -> DVec3 {
        self.previous.eyes().lerp(self.pose.eyes(), alpha)
    }

    /// One tick of `motion`, through `terrain`.
    ///
    /// Under physics, the tick a teleport is reported in holds the player
    /// still; the report says it was handled, and the flag is used up. A
    /// scripted move leaves the flag for the script's own report.
    pub fn step(&mut self, motion: Motion, terrain: &impl Terrain) {
        self.previous = self.pose;
        match motion {
            Motion::Still => self.move_by_keys(Keys::default(), self.pose.pitch, self.pose.yaw, terrain),
            Motion::Keys { keys, pitch, yaw } => self.move_by_keys(keys, pitch, yaw, terrain),
            Motion::To(pose) if !self.teleported => self.pose = pose,
            Motion::To(pose) => {
                self.pose.pitch = pose.pitch;
                self.pose.yaw = pose.yaw;
            }
        }
    }

    fn move_by_keys(&mut self, keys: Keys, pitch: f32, yaw: f32, terrain: &impl Terrain) {
        self.pose.pitch = pitch;
        self.pose.yaw = yaw;
        let mut body = Body {
            feet: self.pose.feet,
            velocity: self.velocity,
            on_ground: self.on_ground,
        };
        let situation = Situation {
            gamemode: self.gamemode,
            may_fly: self.abilities.may_fly,
            hunger: self.attributes.hunger,
            movement: f64::from(self.attributes.movement),
            gravity: f64::from(self.attributes.gravity),
            fly_speed: f64::from(self.abilities.fly_speed),
        };
        let teleported = std::mem::take(&mut self.teleported);
        self.report = self.movement.tick(&mut body, keys, yaw, &situation, terrain, teleported);
        self.pose.feet = body.feet;
        self.velocity = body.velocity;
        self.on_ground = body.on_ground;
    }

    /// The server moved the player: a teleport, a respawn, or a reset. It's
    /// there at once, with nothing to draw in between, and stopped.
    pub fn teleport(&mut self, pose: Pose) {
        self.pose = pose;
        self.previous = pose;
        self.velocity = DVec3::ZERO;
        self.on_ground = false;
        self.teleported = true;
        self.turned = Some((pose.pitch, pose.yaw));
    }

    /// The server's abilities: they say whether the player is flying, too.
    pub const fn set_abilities(&mut self, abilities: Abilities) {
        self.abilities = abilities;
        self.movement.flying = abilities.flying;
    }

    /// The server turned the player, without moving it.
    pub fn turn(&mut self, pitch: f32, yaw: f32) {
        self.pose.pitch = pitch;
        self.pose.yaw = yaw;
        self.turned = Some((pitch, yaw));
    }

    /// The server corrected a prediction: the player is put back where it
    /// says, moving as it says. Pumpkin never sends one (m5-plan Finding 1).
    pub fn correct(&mut self, feet: DVec3, velocity: DVec3) {
        self.pose.feet = feet;
        self.previous = self.pose;
        self.velocity = velocity;
    }

    /// Puts the player at `pose` for scripts, as if it had moved there.
    pub const fn place(&mut self, pose: Pose) {
        self.pose = pose;
    }

    /// Whether a teleport is still to be reported. Asking clears it.
    pub const fn take_teleported(&mut self) -> bool {
        std::mem::replace(&mut self.teleported, false)
    }

    /// Angles the server set since the last call, for the camera.
    pub const fn take_turned(&mut self) -> Option<(f32, f32)> {
        self.turned.take()
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::bedrock::{
        client::SerializedAbilitiesDataSerializedLayer, server::player_auth_input::InputData,
    };

    use super::{
        physics::tests::{Kind, World},
        *,
    };

    /// Flat stone with its top at y 64.
    fn flat(_: i32, y: i32, _: i32) -> Kind {
        if y < 64 { Kind::Stone } else { Kind::Air }
    }

    fn player() -> Player {
        let pose = Pose::from_eyes(DVec3::new(0.5, 65.62, 0.5), 0.0, 0.0);
        Player::new(7, 7, GameMode::Survival, pose)
    }

    fn forward(pitch: f32, yaw: f32) -> Motion {
        let keys = Keys {
            forward: true,
            ..Keys::default()
        };
        Motion::Keys { keys, pitch, yaw }
    }

    #[test]
    fn the_keys_walk_a_tick_at_a_time_and_frames_draw_in_between() {
        let world = World(flat);
        let mut player = player();
        // A tick standing still finds the ground under it.
        player.step(Motion::Still, &world);
        assert!(player.on_ground);
        let start = player.pose().eyes();
        // Facing east (yaw -90): the first tick's push is 0.98 × 0.1.
        player.step(forward(10.0, -90.0), &world);
        let step = 0.098;
        assert!((player.pose().eyes() - (start + DVec3::X * step)).length() < 1e-5);
        assert_eq!((player.pose().pitch, player.pose().yaw), (10.0, -90.0));
        assert!(player.report().flags.has(InputData::Up));
        // Halfway through the next tick, a frame draws it halfway along.
        assert!((player.eyes_at(0.5) - (start + DVec3::X * step / 2.0)).length() < 1e-5);
        assert!((player.eyes_at(0.0) - start).length() < 1e-9);
        assert!((player.eyes_at(1.0) - player.pose().eyes()).length() < 1e-9);
    }

    #[test]
    fn a_teleport_jumps_holds_a_tick_and_is_reported_once() {
        let world = World(flat);
        let mut player = player();
        player.step(forward(0.0, 0.0), &world);
        let there = Pose::from_eyes(DVec3::new(100.5, 80.62, -3.5), 20.0, 45.0);
        player.teleport(there);
        // No sliding across the jump.
        assert_eq!(player.eyes_at(0.5), there.eyes());
        assert_eq!(player.take_turned(), Some((20.0, 45.0)));
        assert_eq!(player.take_turned(), None);
        // The next tick holds still, though the keys still turn it, and says
        // the teleport was handled.
        player.step(forward(5.0, 50.0), &world);
        assert_eq!(player.pose().feet, there.feet);
        assert_eq!(player.pose().yaw, 50.0);
        assert!(player.report().flags.has(InputData::HandledTeleport));
        assert!(!player.take_teleported(), "used up by that tick");
        // Then it moves again: up in the air, it falls.
        player.step(forward(5.0, 50.0), &world);
        assert!(!player.report().flags.has(InputData::HandledTeleport));
        player.step(forward(5.0, 50.0), &world);
        assert!(player.pose().feet.y < there.feet.y);
    }

    #[test]
    fn the_servers_abilities_say_whether_it_flies() {
        let mut player = player();
        player.set_abilities(Abilities {
            may_fly: true,
            flying: true,
            ..Abilities::default()
        });
        assert!(player.movement.flying);
    }

    fn layer(set: u32, values: u32) -> SerializedAbilitiesDataSerializedLayer {
        SerializedAbilitiesDataSerializedLayer {
            serialized_layer: 0,
            abilities_set: set,
            ability_value: values,
            fly_speed: 0.05,
            vertical_fly_speed: 1.0,
            walk_speed: 0.1,
        }
    }

    #[test]
    fn each_ability_comes_from_the_last_layer_that_sets_it() {
        let bit = |ability: Ability| 1_u32 << ability as u32;
        let all = (1 << Ability::AbilityCount as u32) - 1;
        // Pumpkin's spectator: a base layer, then flying on.
        let mut base = layer(all, bit(Ability::MayFly) | bit(Ability::NoClip));
        base.fly_speed = 0.1;
        let data = SerializedAbilitiesData {
            target_player_raw_id: 7,
            player_permissions: PlayerPermissionLevel::Operator,
            command_permissions: pumpkin_protocol::bedrock::client::CommandPermissionLevel::GameDirectors,
            layers: vec![base, layer(bit(Ability::Flying), bit(Ability::Flying))],
        };
        let abilities = Abilities::from_data(&data);
        assert!(abilities.flying && abilities.may_fly && abilities.no_clip);
        assert!(!abilities.build && !abilities.mine && !abilities.instant_build);
        assert_eq!(abilities.permissions, PlayerPermissionLevel::Operator);
        assert!((abilities.fly_speed - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn attributes_take_the_values_the_server_sends() {
        let attribute = |name: &str, value: f32, max: f32| AttributeData {
            min_value: 0.0,
            max_value: max,
            current_value: value,
            default_min_value: 0.0,
            default_max_value: max,
            default_value: value,
            name: name.to_string(),
            modifiers: Vec::new(),
        };
        let mut attributes = Attributes::default();
        attributes.update(&[
            attribute("minecraft:movement", 0.13, 3.4e38),
            attribute("minecraft:health", 7.0, 24.0),
            attribute("minecraft:player.hunger", 15.0, 20.0),
            attribute("minecraft:player.experience", 0.25, 1.0),
            attribute("minecraft:player.level", 7.0, 24_791.0),
            attribute("minecraft:luck", 1.0, 1024.0),
        ]);
        assert_eq!((attributes.experience, attributes.level), (0.25, 7.0));
        assert!((attributes.movement - 0.13).abs() < f32::EPSILON);
        assert_eq!((attributes.health, attributes.max_health), (7.0, 24.0));
        assert_eq!(attributes.hunger, 15.0);
        assert_eq!(attributes.gravity, 0.08, "untouched");
    }

    #[test]
    fn a_default_game_type_is_the_worlds() {
        assert_eq!(gamemode(GameType::Default, GameType::Creative), Some(GameMode::Creative));
        assert_eq!(gamemode(GameType::Spectator, GameType::Creative), Some(GameMode::Spectator));
        assert_eq!(gamemode(GameType::Default, GameType::Default), None);
        assert_eq!(gamemode(GameType::Unknown, GameType::Survival), None);
    }
}
