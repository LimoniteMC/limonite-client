//! The host's session with the embedded server.
//!
//! It keeps the conversation: logging in, the stages up to spawning, and one
//! `PlayerAuthInput` every tick. Everything the server says about the game
//! goes on to [`Game`] (m5-plan Decision 2). Everything goes through a
//! `LocalClientEnd`, whose methods never wait, so the loop needs no async
//! runtime.

use std::{fmt, thread, time::Instant};

use bytes::Bytes;
use glam::{DVec3, IVec3};
use pumpkin_core::net::bedrock::{
    level_chunk::CLevelChunk,
    local::{LocalClientEnd, LocalConnector},
};
use pumpkin_protocol::{
    BClientPacket, RawPacket,
    bedrock::{
        client::{
            CChunkRadiusUpdated, CDisconnect, CPlayStatus, CResourcePackStackPacket,
            CResourcePacksInfo, CStartGame, GameType,
        },
        server::{
            block_pick_request::SBlockPickRequest,
            mob_equipment::SMobEquipment,
            player_auth_input::{InputData, SPlayerAuthInput},
            request_chunk_radius::SRequestChunkRadius,
            resource_pack_client_response::SResourcePackClientResponse,
            set_local_player_as_initialized::SSetLocalPlayerAsInitialized,
        },
    },
    codec::{var_int::VarInt, var_uint::VarUInt, var_ulong::VarULong},
    packet::Packet,
    serial::PacketRead,
};
use pumpkin_util::{
    GameMode,
    math::{position::BlockPos, vector2::Vector2, vector3::Vector3},
};
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tracing::{debug, info, warn};

use crate::{
    inventory::{CONTAINER_MAIN, HOTBAR},
    clock::TICK,
    game::{Effect, Game},
    login,
    measure::Stats,
    packets,
    player::{EYE_HEIGHT, Motion, movement::Flags},
    render::camera::look_direction,
    world::{Applied, World},
};

/// Bedrock's input-mode and play-mode values for mouse and keyboard on a
/// screen, as the official client sends them.
const INPUT_MODE_MOUSE: u32 = 1;
const PLAY_MODE_SCREEN: u32 = 2;
/// The `delta.y` the official client reports every tick while standing on the
/// ground: one tick of gravity, cancelled by the collision.
const STANDING_DELTA_Y: f32 = -0.078_4;
/// How far the host walks each tick: about walking speed.
const WALK_STEP: f32 = 0.2;
/// How far the host flies each tick: creative flying speed, 10.9 m/s.
pub const FLY_STEP: f32 = 0.55;
/// The yaw that faces +X (east). Yaw 0 faces +Z, and yaw grows clockwise
/// seen from above.
const YAW_EAST: f32 = -90.0;

/// How far the session has got. Each stage names what it's waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// `SLogin` sent; waiting for `ResourcePacksInfo`.
    LoggingIn,
    /// `HAVE_ALL_PACKS` sent; waiting for `ResourcePackStack`.
    ResourcePacks,
    /// `COMPLETED` sent; waiting for `StartGame`.
    StartingGame,
    /// `StartGame` received; waiting for `PlayStatus::PlayerSpawn`.
    Spawning,
    /// In the world.
    Spawned,
    /// The server ended the connection.
    Disconnected,
}

/// What `StartGame` told us about the player. The game's player has the
/// rest.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    pub runtime_id: u64,
    /// Where the player's eyes are; Bedrock positions are at eye height.
    pub eye_position: Vector3<f32>,
}

/// What the first input after a teleport said (the "teleport" check).
#[derive(Clone, Copy, Debug)]
pub struct TeleportReport {
    /// Where it put the eyes.
    pub eyes: Vector3<f32>,
    pub delta: Vector3<f32>,
    /// The input's tick number.
    pub tick: u64,
}

/// The session timed out before reaching the stage it waited for.
#[derive(Debug)]
pub struct TimedOut {
    pub stage: Stage,
}

impl fmt::Display for TimedOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timed out while in stage {:?}", self.stage)
    }
}

/// Where this tick's move came from, which decides how the input describes
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// Physics: the player's own report, flag for flag (`player/movement.rs`).
    Physics,
    /// The scripted session's walk (or flight) east, past physics: M4's
    /// flags for walking on the ground.
    ScriptedWalk,
    /// The bench, which places the player where its camera flies: M4's
    /// flags, and however far it moved since the last input.
    Placed,
}

pub struct Session {
    client: LocalClientEnd,
    stage: Stage,
    spawn: Option<Spawn>,
    /// Everything the server has said about the game.
    game: Game,
    /// The eyes the last input reported, for the next one's `delta`.
    reported: Option<DVec3>,
    /// The last input that reported a teleport, until it's taken.
    teleport_report: Option<TeleportReport>,
    /// Ticks of input sent so far; also the input packets' tick number.
    input_tick: u64,
    /// Ticks of moving east still to do, and how far each one goes.
    walk_ticks: u32,
    step: f32,
    /// The view distance the host asks for, in chunks.
    chunk_radius: i32,
    /// `LevelChunk` packets received, decoded or not.
    level_chunks: u32,
    /// `LevelChunk` packets the world decoded and stored.
    chunks_decoded: u32,
    /// Packets the game couldn't apply.
    world_errors: u32,
    /// From `StartGame`: whether block network ids are hashes. Our id table
    /// assumes they are.
    ids_hashed: Option<bool>,
    stats: Stats,
    /// The decoded `StartGame`, kept as an encode-speed yardstick.
    start_game: Option<CStartGame>,
}

impl Session {
    /// Connects to the embedded server and sends the login, as `name` with
    /// this install's `xid`.
    ///
    /// # Errors
    /// If the server isn't accepting local connections.
    pub fn connect(
        connector: &LocalConnector,
        name: &str,
        xid: &str,
        chunk_radius: i32,
    ) -> std::io::Result<Self> {
        let client = connector.connect()?;
        let session = Self {
            client,
            stage: Stage::LoggingIn,
            spawn: None,
            game: Game::default(),
            reported: None,
            teleport_report: None,
            input_tick: 0,
            walk_ticks: 0,
            step: WALK_STEP,
            chunk_radius,
            level_chunks: 0,
            chunks_decoded: 0,
            world_errors: 0,
            ids_hashed: None,
            stats: Stats::default(),
            start_game: None,
        };
        // No `RequestNetworkSettings` first: compression then never turns on,
        // and every batch stays plain bytes (recon §5).
        session.send(&login::host_login(name, xid));
        info!("Logging in as {name}");
        Ok(session)
    }

    pub const fn stage(&self) -> Stage {
        self.stage
    }

    pub const fn spawn(&self) -> Option<Spawn> {
        self.spawn
    }

    pub const fn level_chunks(&self) -> u32 {
        self.level_chunks
    }

    pub const fn stats(&self) -> &Stats {
        &self.stats
    }

    pub const fn game(&self) -> &Game {
        &self.game
    }

    pub const fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// The world: a shortcut, so the checks read as they did before `Game`.
    pub const fn world(&self) -> &World {
        self.game.world()
    }

    /// The world, for a window that tracks its changes.
    pub const fn world_mut(&mut self) -> &mut World {
        self.game.world_mut()
    }

    pub const fn chunks_decoded(&self) -> u32 {
        self.chunks_decoded
    }

    pub const fn world_errors(&self) -> u32 {
        self.world_errors
    }

    pub const fn ids_hashed(&self) -> Option<bool> {
        self.ids_hashed
    }

    /// The eye position the host is at.
    pub fn position(&self) -> Vector3<f32> {
        let eyes = self.game.player().map_or(DVec3::ZERO, |player| player.pose().eyes());
        Vector3::new(eyes.x as f32, eyes.y as f32, eyes.z as f32)
    }

    pub const fn start_game(&self) -> Option<&CStartGame> {
        self.start_game.as_ref()
    }

    /// The last input that reported a teleport, since
    /// [`Self::forget_teleport_report`].
    pub const fn teleport_report(&self) -> Option<TeleportReport> {
        self.teleport_report
    }

    pub const fn forget_teleport_report(&mut self) {
        self.teleport_report = None;
    }

    /// Walks east (+X) for `ticks` ticks, starting next tick.
    ///
    /// A script, not physics: the player moves by a fixed step each tick, and
    /// the server trusts what the client reports. It tests the transport.
    pub const fn walk(&mut self, ticks: u32) {
        self.walk_ticks = ticks;
        self.step = WALK_STEP;
    }

    /// Flies east (+X) at [`FLY_STEP`] blocks a tick for `ticks` ticks, with
    /// the eyes at `eye_y` from the next tick on.
    ///
    /// The server takes the reported position as it is, with no speed or
    /// collision check, so this is a straight line through the air. Flying
    /// above the terrain keeps the server from suffocating the player in a
    /// hillside.
    pub fn fly_east(&mut self, ticks: u32, eye_y: f32) {
        self.rise_to(eye_y);
        self.walk_ticks = ticks;
        self.step = FLY_STEP;
    }

    /// Puts the eyes at `eye_y`, x and z unchanged, from the next input on.
    ///
    /// The scripted host has no physics: its walk keeps one height, so it can
    /// end up with its head in a hillside, where the server suffocates it.
    /// Up in the air it can stay as long as a measurement needs.
    pub fn rise_to(&mut self, eye_y: f32) {
        if let Some(player) = self.game.player_mut() {
            let mut pose = player.pose();
            pose.feet.y = f64::from(eye_y) - EYE_HEIGHT;
            player.place(pose);
        }
    }

    pub const fn is_walking(&self) -> bool {
        self.walk_ticks > 0
    }

    /// Warns if `StartGame` put the host in another gamemode than `wanted`:
    /// a world that saved the host keeps the host's own mode (m4-plan
    /// Finding 11), whatever new players get.
    pub fn check_gamemode(&self, wanted: GameMode) {
        let wanted = GameType::from(wanted);
        if let Some(start_game) = &self.start_game
            && start_game.player_gamemode != wanted
        {
            warn!(
                "The host is in {:?}, not {wanted:?} as set: this world saved the host's own mode. \
                 Use a new --data-dir for a fresh world",
                start_game.player_gamemode
            );
        }
    }

    /// Runs ticks until `done` returns true or `deadline` passes.
    ///
    /// # Errors
    /// [`TimedOut`] with the stage the session was stuck in.
    pub fn run_until(
        &mut self,
        deadline: Instant,
        mut done: impl FnMut(&Self) -> bool,
    ) -> Result<(), TimedOut> {
        let mut next_tick = Instant::now();
        loop {
            self.tick();
            if done(self) {
                return Ok(());
            }
            if self.stage == Stage::Disconnected || Instant::now() >= deadline {
                return Err(TimedOut { stage: self.stage });
            }
            next_tick += TICK;
            if let Some(wait) = next_tick.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
    }

    /// One scripted tick: handle everything that arrived, step the game with
    /// the script's motion, and send this tick's input.
    ///
    /// Public so long work on the main thread (the measurements) can call it
    /// in between: the server drops a client it hasn't heard from in 10 s.
    pub fn tick(&mut self) {
        self.drain();
        let (motion, source) = self.scripted();
        self.step_with(motion, source);
    }

    /// One tick for the window: steps the game with `motion`, and sends the
    /// input that reports it. One every 50 ms, whatever the frame rate.
    pub fn step(&mut self, motion: Motion) {
        let source = if matches!(motion, Motion::To(_)) {
            Source::Placed
        } else {
            Source::Physics
        };
        self.step_with(motion, source);
    }

    /// Input starts with `StartGame`: the server ignores it until the player
    /// has loaded, but any packet keeps its 10 s timeout away.
    fn step_with(&mut self, motion: Motion, source: Source) {
        if self.stage == Stage::Disconnected || self.game.player().is_none() {
            return;
        }
        self.game.tick(motion);
        if let Some(input) = self.input(source) {
            self.send(&input);
            self.input_tick += 1;
        }
    }

    /// The scripted session's motion this tick: a step east while it walks,
    /// and physics alone otherwise, so it stands, or falls back down.
    fn scripted(&mut self) -> (Motion, Source) {
        let Some(player) = self.game.player() else {
            return (Motion::Still, Source::Physics);
        };
        if self.walk_ticks == 0 {
            return (Motion::Still, Source::Physics);
        }
        self.walk_ticks -= 1;
        let mut pose = player.pose();
        pose.feet.x += f64::from(self.step);
        pose.yaw = YAW_EAST;
        (Motion::To(pose), Source::ScriptedWalk)
    }

    /// Handles everything that has arrived, without sending input. Called on
    /// its own, it times arrivals more finely than a tick.
    pub fn drain(&mut self) {
        loop {
            match self.client.try_recv() {
                Ok(batch) => self.handle_batch(batch),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    if self.stage != Stage::Disconnected {
                        warn!("The server closed the connection");
                        self.stage = Stage::Disconnected;
                    }
                    return;
                }
            }
        }
    }

    fn handle_batch(&mut self, batch: Bytes) {
        let len = batch.len();
        let start = Instant::now();
        let split = packets::split_batch(batch);
        self.stats.record_batch(len, start.elapsed());
        match split {
            Ok(packets) => {
                for packet in packets {
                    self.stats.record_packet(packet.id, packet.payload.len());
                    self.handle_packet(&packet);
                }
            }
            Err(err) => warn!("Couldn't split a batch from the server: {err}"),
        }
    }

    fn handle_packet(&mut self, packet: &RawPacket) {
        debug!("<- {}", packets::PacketName(packet.id));
        match packet.id {
            CPlayStatus::PACKET_ID => match self.decode::<CPlayStatus>(packet) {
                Ok(CPlayStatus::LoginSuccess) => info!("Login accepted"),
                Ok(CPlayStatus::PlayerSpawn) => self.on_player_spawn(),
                Ok(status) => warn!("PlayStatus {}", status as i32),
                Err(err) => warn!("Bad PlayStatus: {err}"),
            },
            CResourcePacksInfo::PACKET_ID => {
                self.send_pack_response(SResourcePackClientResponse::STATUS_HAVE_ALL_PACKS);
                self.stage = Stage::ResourcePacks;
            }
            CResourcePackStackPacket::PACKET_ID => {
                self.send_pack_response(SResourcePackClientResponse::STATUS_COMPLETED);
                self.stage = Stage::StartingGame;
            }
            CStartGame::PACKET_ID => match self.decode::<CStartGame>(packet) {
                Ok(start_game) => self.on_start_game(start_game),
                Err(err) => warn!("Bad StartGame: {err}"),
            },
            CChunkRadiusUpdated::PACKET_ID => match self.decode::<CChunkRadiusUpdated>(packet) {
                Ok(update) => info!("Chunk radius is {}", update.chunk_radius.0),
                Err(err) => warn!("Bad ChunkRadiusUpdated: {err}"),
            },
            CDisconnect::PACKET_ID => {
                match self.decode::<CDisconnect>(packet) {
                    Ok(disconnect) => warn!(
                        "Disconnected by the server (reason {}): {}",
                        disconnect.reason.0, disconnect.message
                    ),
                    Err(err) => warn!("Disconnected by the server (unreadable: {err})"),
                }
                self.stage = Stage::Disconnected;
            }
            id => {
                if id == CLevelChunk::PACKET_ID {
                    self.level_chunks += 1;
                }
                self.apply(packet);
            }
        }
    }

    /// Decodes `packet` as a `P`, timing it.
    fn decode<P: PacketRead>(&mut self, packet: &RawPacket) -> std::io::Result<P> {
        let mut body = packet.payload.as_ref();
        let start = Instant::now();
        let decoded = P::read(&mut body);
        self.stats.record_decode(packet.id, start.elapsed());
        decoded
    }

    /// Hands a packet to the game, timing the ones it uses into the stats.
    fn apply(&mut self, packet: &RawPacket) {
        let start = Instant::now();
        let applied = self.game.apply(packet);
        let took = start.elapsed();
        match applied {
            Ok(Effect::Ignored) => {}
            Ok(effect) => {
                self.stats.record_decode(packet.id, took);
                if let Effect::World(Applied::Chunk { .. }) = effect {
                    self.chunks_decoded += 1;
                }
            }
            Err(err) => {
                self.stats.record_decode(packet.id, took);
                self.world_errors += 1;
                warn!("{err}");
            }
        }
    }

    fn on_start_game(&mut self, start_game: CStartGame) {
        let spawn = Spawn {
            runtime_id: start_game.runtime_entity_id.0,
            eye_position: start_game.position,
        };
        let settings = &start_game.level_settings;
        let block = settings.spawn_position.0;
        info!(
            "StartGame: runtime id {}, seed {}, world spawn ({}, {}, {}), player eyes at ({:.2}, {:.2}, {:.2}), \
             gamemode {:?}",
            spawn.runtime_id,
            settings.seed,
            block.x,
            block.y,
            block.z,
            spawn.eye_position.x,
            spawn.eye_position.y,
            spawn.eye_position.z,
            start_game.player_gamemode,
        );
        if !start_game.blocknetwork_ids_are_hashed {
            warn!("StartGame says block ids aren't hashed; the world's id table expects hashes");
        }
        self.ids_hashed = Some(start_game.blocknetwork_ids_are_hashed);
        self.game.start(&start_game);
        self.spawn = Some(spawn);
        self.start_game = Some(start_game);
        self.stage = Stage::Spawning;
        self.send(&SRequestChunkRadius {
            chunk_radius: VarInt(self.chunk_radius),
            max_chunk_radius: self.chunk_radius as u8,
        });
    }

    fn on_player_spawn(&mut self) {
        let Some(spawn) = self.spawn else {
            warn!("PlayerSpawn arrived before StartGame");
            return;
        };
        self.send(&SSetLocalPlayerAsInitialized {
            player_id: VarULong(spawn.runtime_id),
        });
        self.stage = Stage::Spawned;
        // The server sends PlayerSpawn once more than 4 chunks are on their way;
        // it can arrive before they do.
        info!("Spawned ({} LevelChunk packets so far)", self.level_chunks);
    }

    fn send_pack_response(&self, response: u8) {
        self.send(&SResourcePackClientResponse {
            response,
            download_size: 0,
            pack_ids: Vec::new(),
        });
    }

    /// This tick's `PlayerAuthInput`, from the player as the game has it.
    ///
    /// Under physics it's the player's own report: the flags, `move_vec` and
    /// `raw_move` as `player/movement.rs` set them, and the velocity as
    /// `delta`, as the official client sends them (m5-plan Results, Step 1).
    ///
    /// A scripted walk keeps M4's report of walking on the ground: the
    /// position, `delta` and `move_vec` move together, as they would from the
    /// official client holding "forward" facing east. The bench reports how
    /// far the player moved since the last input. Either way, the first input
    /// after a teleport says it was handled, with no movement.
    fn input(&mut self, source: Source) -> Option<SPlayerAuthInput> {
        let player = self.game.player_mut()?;
        let pose = player.pose();
        let eyes = pose.eyes();
        let position = Vector3::new(eyes.x as f32, eyes.y as f32, eyes.z as f32);
        let still = Vector2::new(0.0, 0.0);
        let (flags, move_vec, raw_move, delta) = if source == Source::Physics {
            let report = *player.report();
            let v = player.velocity;
            (
                report.flags,
                vec2(report.move_vec),
                vec2(report.raw_move),
                Vector3::new(v.x as f32, v.y as f32, v.z as f32),
            )
        } else {
            // The flags the official client sets while on the ground.
            let mut flags = Flags::default();
            flags.set(InputData::BlockBreakingDelayEnabled);
            flags.set(InputData::VerticalCollision);
            let (move_vec, delta) = if player.take_teleported() {
                flags.set(InputData::HandledTeleport);
                (still, Vector3::new(0.0, 0.0, 0.0))
            } else if source == Source::ScriptedWalk {
                flags.set(InputData::Up);
                (Vector2::new(0.0, 1.0), Vector3::new(self.step, STANDING_DELTA_Y, 0.0))
            } else {
                let moved = eyes - self.reported.unwrap_or(eyes);
                (still, Vector3::new(moved.x as f32, moved.y as f32, moved.z as f32))
            };
            (flags, move_vec, move_vec, delta)
        };
        self.reported = Some(eyes);
        if flags.has(InputData::HandledTeleport) {
            self.teleport_report = Some(TeleportReport {
                eyes: position,
                delta,
                tick: self.input_tick,
            });
        }
        let (pitch, yaw) = (pose.pitch, pose.yaw);
        let forward = look_direction(pitch, yaw);
        Some(SPlayerAuthInput {
            pitch,
            yaw,
            position,
            move_vec,
            head_yaw: yaw,
            input_data: flags.to_bitset(),
            input_mode: VarUInt(INPUT_MODE_MOUSE),
            play_mode: VarUInt(PLAY_MODE_SCREEN),
            interaction_model: VarInt(0),
            interact_pitch: pitch,
            interact_yaw: yaw,
            tick: VarULong(self.input_tick),
            delta,
            block_actions: None,
            item_interaction: None,
            item_stack_request: None,
            vehicle_rotation: None,
            vehicle_unique_id: None,
            analog_move: Vector2::new(0.0, 0.0),
            camera_orientation: Vector3::new(forward.x, forward.y, forward.z),
            raw_move,
        })
    }

    /// Frames and queues one packet for the server.
    /// Selects a hotbar slot, and tells the server, as the official client does on every
    /// change. The packet names the item now in hand.
    pub fn select_slot(&mut self, slot: u8) {
        if !self.game.inventory_mut().select(slot) {
            return;
        }
        let Some(player) = self.game.player() else {
            return;
        };
        self.send(&SMobEquipment {
            entity_runtime_id: VarULong(player.runtime_id),
            item: self.game.inventory().held().clone(),
            slot,
            selected_slot: slot,
            container_id: CONTAINER_MAIN as u8,
        });
    }

    /// Asks the server for the block at `block` in the hotbar, as a middle click
    /// does. Pumpkin ignores this until upstream fixes it (m5-plan Deviation 8):
    /// it takes `max_slots` for the slot to fill, and it's always 9.
    pub fn pick_block(&mut self, block: IVec3) {
        self.send(&SBlockPickRequest {
            position: BlockPos::new(block.x, block.y, block.z),
            with_data: false,
            max_slots: HOTBAR as u8,
        });
    }

    /// Drops one of the held item, if there is one.
    pub fn drop_held(&mut self) {
        if let Some(transaction) = self.game.inventory_mut().drop_one() {
            self.send(&transaction);
        }
    }

    fn send<P: BClientPacket>(&self, packet: &P) {
        let batch = match packets::frame(packet) {
            Ok(batch) => batch,
            Err(err) => {
                warn!("Couldn't encode packet {}: {err}", P::PACKET_ID);
                return;
            }
        };
        match self.client.try_send(batch) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                warn!(
                    "The server's queue is full; dropped packet {}",
                    P::PACKET_ID
                );
            }
            Err(TrySendError::Closed(_)) => {
                debug!("Not sending packet {}: connection closed", P::PACKET_ID);
            }
        }
    }
}

fn vec2(v: glam::DVec2) -> Vector2<f32> {
    Vector2::new(v.x as f32, v.y as f32)
}
