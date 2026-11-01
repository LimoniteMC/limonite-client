//! The client's whole picture of the game (m5-plan Decision 2; worth
//! teaching).
//!
//! **One owner for what the server says.** `Game` holds the world, the local
//! player and the time of day, and [`Game::apply`] is the only way a packet
//! changes any of them. `Session` keeps the conversation (logging in, the
//! stages, sending input) and hands every other packet over; the renderer,
//! and from Step 8 the UI, only read `Game`. So the wall PLAN.md puts between
//! networking and rendering ("UI reads world state; it never reads packets
//! directly") now holds for the player and the clock too, not just chunks.
//! Entities and the inventory join it in later steps.

use std::fmt;

use glam::DVec3;
use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
use pumpkin_protocol::{
    RawPacket,
    bedrock::client::{
        CCorrectPlayerMovePrediction, CMovePlayer, CSetActorMotion, CSetHealth, CSetPlayerGameType,
        CSetTime, CStartGame, CUpdateAbilities, CUpdateAttributes, GameType,
        inventory_content::CInventoryContent,
        inventory_slot::CInventorySlot,
        network_chunk_publisher_update::CNetworkChunkPublisherUpdate,
        player_hotbar::CPlayerHotbar,
        set_actor_data::{CSetActorData, MetadataValue, entity_data_key},
        update_block::CUpdateBlock,
    },
    packet::Packet,
    serial::PacketRead,
};
use pumpkin_util::{GameMode, math::vector3::Vector3};
use tracing::warn;

use crate::{
    clock::TimeOfDay,
    inventory::{CONTAINER_MAIN, Inventory},
    packets::PacketName,
    player::{self, Abilities, Motion, Player, Pose, terrain::WorldTerrain},
    world::{Applied, ApplyError, World},
};

/// What a packet changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// The world's chunks or blocks.
    World(Applied),
    /// The local player.
    Player,
    /// The time of day.
    Time,
    /// The player's inventory.
    Inventory,
    /// Nothing the client keeps yet, or something about someone else.
    Ignored,
}

/// A packet the game couldn't apply.
#[derive(Debug)]
pub enum GameError {
    World(ApplyError),
    /// A packet that didn't decode.
    Packet { id: PacketName, err: std::io::Error },
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::World(err) => err.fmt(f),
            Self::Packet { id, err } => write!(f, "bad {id}: {err}"),
        }
    }
}

pub struct Game {
    world: World,
    /// From `StartGame` on.
    player: Option<Player>,
    inventory: Inventory,
    time: TimeOfDay,
    /// Ticks stepped since `StartGame`.
    ticks: u64,
    /// The world's own gamemode, for a player whose game type is `Default`.
    world_gamemode: GameType,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            world: World::new(),
            player: None,
            inventory: Inventory::default(),
            time: TimeOfDay::default(),
            ticks: 0,
            world_gamemode: GameType::Default,
        }
    }
}

impl Game {
    pub const fn world(&self) -> &World {
        &self.world
    }

    pub const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The local player, from `StartGame` on.
    pub const fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    pub const fn player_mut(&mut self) -> Option<&mut Player> {
        self.player.as_mut()
    }

    pub const fn time(&self) -> &TimeOfDay {
        &self.time
    }

    pub const fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    pub const fn inventory_mut(&mut self) -> &mut Inventory {
        &mut self.inventory
    }

    /// Ticks stepped since `StartGame`.
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Takes the player from `StartGame`: its ids, its gamemode, and where it
    /// starts.
    pub fn start(&mut self, start_game: &CStartGame) {
        let pose = Pose::from_eyes(vec3(start_game.position), start_game.pitch, start_game.yaw);
        self.start_player(
            Player::new(
                start_game.runtime_entity_id.0,
                start_game.entity_id.0,
                GameMode::Survival,
                pose,
            ),
            start_game.player_gamemode,
            start_game.level_settings.world_gamemode,
        );
    }

    fn start_player(&mut self, mut player: Player, game_type: GameType, world_gamemode: GameType) {
        self.world_gamemode = world_gamemode;
        match player::gamemode(game_type, world_gamemode) {
            Some(mode) => player.gamemode = mode,
            None => warn!("StartGame gives the player game type {game_type:?}; taking survival"),
        }
        self.player = Some(player);
    }

    /// One tick: the time of day moves on, and the player moves by `motion`
    /// through the world's blocks.
    pub fn tick(&mut self, motion: Motion) {
        self.ticks += 1;
        self.time.tick();
        let terrain = WorldTerrain::new(&self.world);
        if let Some(player) = &mut self.player {
            player.step(motion, &terrain);
        }
    }

    /// Applies one packet from the server.
    ///
    /// # Errors
    /// If the packet doesn't decode, or the world can't take it.
    pub fn apply(&mut self, packet: &RawPacket) -> Result<Effect, GameError> {
        match packet.id {
            CLevelChunk::PACKET_ID | CUpdateBlock::PACKET_ID | CNetworkChunkPublisherUpdate::PACKET_ID => {
                self.world.apply(packet).map(Effect::World).map_err(GameError::World)
            }
            CSetTime::PACKET_ID => {
                let set: CSetTime = read(packet)?;
                self.time.set(i64::from(set.time.0), self.ticks);
                Ok(Effect::Time)
            }
            CInventoryContent::PACKET_ID => {
                let content: CInventoryContent = read(packet)?;
                if content.container_id.0 != CONTAINER_MAIN {
                    return Ok(Effect::Ignored);
                }
                self.inventory.set_main(content.slots);
                Ok(Effect::Inventory)
            }
            CInventorySlot::PACKET_ID => {
                let slot: CInventorySlot = read(packet)?;
                if slot.container_id.0 != CONTAINER_MAIN {
                    return Ok(Effect::Ignored);
                }
                self.inventory.set_slot(slot.slot.0 as usize, slot.item);
                Ok(Effect::Inventory)
            }
            CPlayerHotbar::PACKET_ID => {
                let hotbar: CPlayerHotbar = read(packet)?;
                if hotbar.should_select_slot && hotbar.selected_slot.0 < 9 {
                    self.inventory.select(hotbar.selected_slot.0 as u8);
                }
                Ok(Effect::Inventory)
            }
            _ => self.apply_to_player(packet),
        }
    }

    /// The packets about the local player (m5-plan Decision 4: the server's
    /// word wins over the client's prediction).
    fn apply_to_player(&mut self, packet: &RawPacket) -> Result<Effect, GameError> {
        let world_gamemode = self.world_gamemode;
        let Some(player) = &mut self.player else {
            return Ok(Effect::Ignored);
        };
        match packet.id {
            CMovePlayer::PACKET_ID => {
                let moved: CMovePlayer = read(packet)?;
                if moved.player_runtime_id.0 != player.runtime_id {
                    return Ok(Effect::Ignored);
                }
                if moved.mode == CMovePlayer::MODE_ROTATION {
                    player.turn(moved.pitch, moved.yaw);
                } else {
                    player.teleport(Pose::from_eyes(vec3(moved.position), moved.pitch, moved.yaw));
                }
            }
            CSetActorMotion::PACKET_ID => {
                let motion: CSetActorMotion = read(packet)?;
                if motion.target_runtime_id.0 != player.runtime_id {
                    return Ok(Effect::Ignored);
                }
                player.velocity = vec3(motion.motion);
            }
            CCorrectPlayerMovePrediction::PACKET_ID => {
                let correction: CCorrectPlayerMovePrediction = read(packet)?;
                // At eye height, as every player position Bedrock sends.
                let feet = vec3(correction.pos) - DVec3::Y * player::EYE_HEIGHT;
                player.correct(feet, vec3(correction.pos_delta));
            }
            CUpdateAbilities::PACKET_ID => {
                let update: CUpdateAbilities = read(packet)?;
                if update.data.target_player_raw_id != player.unique_id {
                    return Ok(Effect::Ignored);
                }
                player.set_abilities(Abilities::from_data(&update.data));
            }
            CSetPlayerGameType::PACKET_ID => {
                let set: CSetPlayerGameType = read(packet)?;
                match player::gamemode(set.player_game_type, world_gamemode) {
                    Some(mode) => player.gamemode = mode,
                    None => warn!("SetPlayerGameType to {:?}: kept {:?}", set.player_game_type, player.gamemode),
                }
            }
            CUpdateAttributes::PACKET_ID => {
                let update: CUpdateAttributes = read(packet)?;
                if update.target_runtime_id.0 != player.runtime_id {
                    return Ok(Effect::Ignored);
                }
                player.attributes.update(&update.attribute_list);
            }
            CSetHealth::PACKET_ID => {
                let set: CSetHealth = read(packet)?;
                player.attributes.health = set.health.0 as f32;
            }
            // Breath isn't an attribute on Bedrock, but one of the actor's data.
            CSetActorData::PACKET_ID => {
                let data: CSetActorData = read(packet)?;
                if data.target_runtime_id.0 != player.runtime_id {
                    return Ok(Effect::Ignored);
                }
                if let Some(MetadataValue::Short(air)) = data.actor_data.0.get(&entity_data_key::AIR_SUPPLY) {
                    player.attributes.air = f32::from(*air);
                }
            }
            _ => return Ok(Effect::Ignored),
        }
        Ok(Effect::Player)
    }
}

/// Decodes a packet's body as a `P`.
fn read<P: PacketRead>(packet: &RawPacket) -> Result<P, GameError> {
    let mut body = packet.payload.as_ref();
    P::read(&mut body).map_err(|err| GameError::Packet {
        id: PacketName(packet.id),
        err,
    })
}

fn vec3(v: Vector3<f32>) -> DVec3 {
    DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::{
        BClientPacket,
        bedrock::client::{
            AttributeData, CommandPermissionLevel, PlayerPermissionLevel, SerializedAbilitiesData,
            SerializedAbilitiesDataSerializedLayer,
            set_actor_data::{PropertySyncData, SyncedActorDataList},
            update_abilities::Ability,
        },
        bedrock::network_item::{ContainerName, FullContainerName, NetworkItemStackDescriptor},
        codec::{var_int::VarInt, var_uint::VarUInt, var_ulong::VarULong},
    };
    use pumpkin_util::math::vector2::Vector2;

    use super::*;
    use crate::packets;

    const ID: u64 = 7;
    const OTHER: u64 = 8;

    fn game() -> Game {
        let mut game = Game::default();
        let pose = Pose::from_eyes(DVec3::new(0.5, 65.62, 0.5), 0.0, 0.0);
        game.start_player(
            Player::new(ID, ID as i64, GameMode::Survival, pose),
            GameType::Default,
            GameType::Creative,
        );
        game
    }

    /// Frames `packet` and splits it again, as `Session` receives it.
    fn apply<P: BClientPacket>(game: &mut Game, packet: &P) -> Effect {
        let batch = packets::frame(packet).unwrap();
        let raw = packets::split_batch(batch).unwrap().remove(0);
        game.apply(&raw).unwrap()
    }

    fn player(game: &Game) -> &Player {
        game.player().unwrap()
    }

    fn move_player(id: u64, mode: u8) -> CMovePlayer {
        CMovePlayer::new(
            VarULong(id),
            Vector3::new(10.5, 71.62, -4.5),
            15.0,
            90.0,
            90.0,
            mode,
            false,
            VarULong(0),
            0,
            0,
            VarULong(3),
        )
    }

    #[test]
    fn a_default_game_type_takes_the_worlds_mode() {
        assert_eq!(player(&game()).gamemode, GameMode::Creative);
    }

    #[test]
    fn a_teleport_moves_our_player_and_no_one_else_moves_it() {
        let mut game = game();
        assert_eq!(apply(&mut game, &move_player(OTHER, CMovePlayer::MODE_TELEPORT)), Effect::Ignored);
        assert_eq!(apply(&mut game, &move_player(ID, CMovePlayer::MODE_TELEPORT)), Effect::Player);
        let pose = player(&game).pose();
        assert!((pose.feet - DVec3::new(10.5, 70.0, -4.5)).length() < 1e-5);
        assert_eq!((pose.pitch, pose.yaw), (15.0, 90.0));
        assert!(game.player_mut().unwrap().take_teleported());

        // A rotation turns it where it stands.
        let mut turn = move_player(ID, CMovePlayer::MODE_ROTATION);
        turn.pitch = -30.0;
        apply(&mut game, &turn);
        let turned = player(&game).pose();
        assert_eq!((turned.feet, turned.pitch), (pose.feet, -30.0));
        assert!(!game.player_mut().unwrap().take_teleported());
    }

    #[test]
    fn motion_and_corrections_set_the_velocity() {
        let mut game = game();
        let knock = |id| CSetActorMotion {
            target_runtime_id: VarULong(id),
            motion: Vector3::new(0.4, 0.36, 0.0),
            tick: VarULong(0),
        };
        assert_eq!(apply(&mut game, &knock(OTHER)), Effect::Ignored);
        apply(&mut game, &knock(ID));
        assert!((player(&game).velocity - DVec3::new(0.4, 0.36, 0.0)).length() < 1e-6);

        apply(
            &mut game,
            &CCorrectPlayerMovePrediction {
                prediction_type: 0,
                pos: Vector3::new(1.5, 66.62, 2.5),
                pos_delta: Vector3::new(0.0, -0.0784, 0.0),
                rotation: Vector2::new(0.0, 0.0),
                vehicle_angular_velocity: None,
                on_ground: true,
                tick: VarULong(9),
            },
        );
        assert!((player(&game).pose().feet - DVec3::new(1.5, 65.0, 2.5)).length() < 1e-5);
        assert!((player(&game).velocity.y + 0.0784).abs() < 1e-6);
    }

    #[test]
    fn abilities_gamemode_attributes_and_health() {
        let mut game = game();
        let all = (1_u32 << Ability::AbilityCount as u32) - 1;
        let flying = (1_u32 << Ability::MayFly as u32) | (1 << Ability::Flying as u32);
        let abilities = |target| CUpdateAbilities {
            data: SerializedAbilitiesData {
                target_player_raw_id: target,
                player_permissions: PlayerPermissionLevel::Operator,
                command_permissions: CommandPermissionLevel::GameDirectors,
                layers: vec![SerializedAbilitiesDataSerializedLayer {
                    serialized_layer: 0,
                    abilities_set: all,
                    ability_value: flying,
                    fly_speed: 0.05,
                    vertical_fly_speed: 1.0,
                    walk_speed: 0.1,
                }],
            },
        };
        assert_eq!(apply(&mut game, &abilities(OTHER as i64)), Effect::Ignored);
        apply(&mut game, &abilities(ID as i64));
        let got = player(&game).abilities;
        assert!(got.may_fly && got.flying && !got.build);
        assert_eq!(got.permissions, PlayerPermissionLevel::Operator);

        apply(
            &mut game,
            &CSetPlayerGameType {
                player_game_type: GameType::Spectator,
            },
        );
        assert_eq!(player(&game).gamemode, GameMode::Spectator);

        apply(
            &mut game,
            &CUpdateAttributes {
                target_runtime_id: VarULong(ID),
                attribute_list: vec![AttributeData {
                    min_value: 0.0,
                    max_value: 20.0,
                    current_value: 12.0,
                    default_min_value: 0.0,
                    default_max_value: 20.0,
                    default_value: 20.0,
                    name: "minecraft:player.hunger".to_string(),
                    modifiers: Vec::new(),
                }],
                tick: VarULong(0),
            },
        );
        assert_eq!(player(&game).attributes.hunger, 12.0);

        apply(&mut game, &CSetHealth { health: VarInt(9) });
        assert_eq!(player(&game).attributes.health, 9.0);
    }

    fn stack(id: i16, count: u16) -> NetworkItemStackDescriptor {
        NetworkItemStackDescriptor {
            id,
            stack_size: count,
            ..NetworkItemStackDescriptor::default()
        }
    }

    fn inventory_name() -> FullContainerName {
        FullContainerName {
            container_name: ContainerName::Inventory,
            dynamic_id: None,
        }
    }

    #[test]
    fn the_server_says_what_the_player_carries() {
        let mut game = game();
        let content = |container| CInventoryContent {
            container_id: VarUInt(container),
            slots: vec![stack(5, 64), stack(6, 1)],
            full_container_name: inventory_name(),
            storage_item: NetworkItemStackDescriptor::default(),
        };
        assert_eq!(apply(&mut game, &content(7)), Effect::Ignored, "someone else's chest");
        assert_eq!(game.inventory().hotbar()[0], None);
        assert_eq!(apply(&mut game, &content(0)), Effect::Inventory);
        assert_eq!(game.inventory().hotbar()[0].map(|item| (item.id, item.count)), Some((5, 64)));

        apply(
            &mut game,
            &CInventorySlot {
                container_id: VarUInt(0),
                slot: VarUInt(3),
                full_container_name: Some(inventory_name()),
                storage_item: None,
                item: stack(9, 2),
            },
        );
        assert_eq!(game.inventory().hotbar()[3].map(|item| (item.id, item.count)), Some((9, 2)));
    }

    #[test]
    fn the_server_can_select_a_slot() {
        let mut game = game();
        let hotbar = |slot, select| CPlayerHotbar {
            selected_slot: VarUInt(slot),
            container_id: 0,
            should_select_slot: select,
        };
        apply(&mut game, &hotbar(4, true));
        assert_eq!(game.inventory().selected(), 4);
        apply(&mut game, &hotbar(6, false));
        assert_eq!(game.inventory().selected(), 4, "it only says what's selected");
        apply(&mut game, &hotbar(20, true));
        assert_eq!(game.inventory().selected(), 4, "no such slot");
    }

    #[test]
    fn breath_comes_from_our_actors_data() {
        let mut game = game();
        let data = |id, air| {
            let mut actor_data = SyncedActorDataList::new();
            actor_data.set(entity_data_key::AIR_SUPPLY, MetadataValue::Short(air));
            CSetActorData {
                target_runtime_id: VarULong(id),
                actor_data,
                synced_properties: PropertySyncData::default(),
                tick: VarULong(0),
            }
        };
        assert_eq!(apply(&mut game, &data(OTHER, 10)), Effect::Ignored, "a fish's breath isn't ours");
        assert_eq!(player(&game).attributes.air, 300.0);
        assert_eq!(apply(&mut game, &data(ID, 120)), Effect::Player);
        assert_eq!(player(&game).attributes.air, 120.0);
    }

    #[test]
    fn set_time_sets_the_clock_and_ticks_move_it() {
        let mut game = game();
        assert_eq!(apply(&mut game, &CSetTime::new(6_000)), Effect::Time);
        game.tick(Motion::Still);
        game.tick(Motion::Still);
        assert_eq!(game.time().get(), Some(6_002));
    }

    #[test]
    fn packets_before_start_game_leave_no_player() {
        let mut game = Game::default();
        assert_eq!(apply(&mut game, &CSetHealth { health: VarInt(9) }), Effect::Ignored);
        assert!(game.player().is_none());
        // A tick with no player only moves the clock.
        game.tick(Motion::Still);
    }

    #[test]
    fn a_bad_packet_says_which_it_was() {
        let mut game = game();
        let raw = RawPacket {
            id: CSetHealth::PACKET_ID,
            payload: bytes::Bytes::new(),
        };
        let err = game.apply(&raw).unwrap_err();
        assert!(err.to_string().starts_with("bad CSetHealth"), "{err}");
    }
}
