use std::time::{Duration, Instant};

use pumpkin_protocol::{
    bedrock::{
        client::{
            BuildPlatform, CAddActor, CAddItemActor, CAddPlayer, CMoveActorAbsolute, CMoveActorDelta,
            CommandPermissionLevel, GameType, PlayerPermissionLevel, SerializedAbilitiesData, common::ActorLink,
            move_actor_delta::{MOVE_ACTOR_DELTA_FLAG_HAS_X, MOVE_ACTOR_DELTA_FLAG_HAS_Y},
            set_actor_data::{MetadataValue, PropertySyncData, SyncedActorDataList, entity_data_flag, entity_data_key},
        },
        network_item::{ItemStackWrapper, NetworkItemStackDescriptor},
    },
    codec::{var_long::VarLong, var_uint::VarUInt, var_ulong::VarULong},
};
use pumpkin_util::math::{vector2::Vector2, vector3::Vector3};
use uuid::Uuid;

use super::*;

const MS: Duration = Duration::from_millis(1);

fn player(runtime: u64, name: &str, eyes: [f32; 3]) -> CAddPlayer {
    CAddPlayer {
        uuid: Uuid::nil(),
        player_name: name.to_string(),
        target_runtime_id: VarULong(runtime),
        platform_chat_id: String::new(),
        position: Vector3::new(eyes[0], eyes[1], eyes[2]),
        velocity: Vector3::new(0.0, 0.0, 0.0),
        rotation: Vector2::new(0.0, 180.0),
        y_head_rotation: 180.0,
        carried_item: NetworkItemStackDescriptor::default(),
        player_game_type: GameType::Survival,
        entity_data: SyncedActorDataList::default(),
        synced_properties: PropertySyncData::default(),
        abilities_data: SerializedAbilitiesData {
            target_player_raw_id: runtime as i64,
            player_permissions: PlayerPermissionLevel::Member,
            command_permissions: CommandPermissionLevel::Any,
            layers: Vec::new(),
        },
        actor_links: Vec::new(),
        passenger_of_block: None,
        device_id: String::new(),
        build_platform: BuildPlatform::Unknown,
    }
}

fn actor(runtime: u64, identifier: &str, position: [f32; 3]) -> CAddActor {
    CAddActor {
        target_actor_id: VarLong(runtime as i64),
        target_runtime_id: VarULong(runtime),
        actor_type: identifier.to_string(),
        position: Vector3::new(position[0], position[1], position[2]),
        velocity: Vector3::new(0.0, 0.0, 0.0),
        rotation: Vector2::new(10.0, 90.0),
        y_head_rotation: 90.0,
        y_body_rotation: 90.0,
        attributes_list: Vec::new(),
        actor_data: SyncedActorDataList::default(),
        synced_properties: PropertySyncData::default(),
        actor_links: Vec::new(),
        passenger_of_block: None,
    }
}

fn delta(runtime: u64, flags: u16, x: f32, y: f32) -> CMoveActorDelta {
    CMoveActorDelta::new(VarULong(runtime), flags, x, y, 0.0, 0, 0, 0)
}

fn baby_flags(baby: bool) -> SyncedActorDataList {
    let mut data = SyncedActorDataList::default();
    data.0.insert(
        entity_data_key::FLAGS,
        MetadataValue::Int64(if baby { 1 << entity_data_flag::BABY } else { 0 }),
    );
    data
}

fn only(entities: &Entities, now: Instant) -> Drawn {
    let drawn = entities.drawn(now);
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    drawn[0]
}

#[test]
fn an_entity_comes_moves_and_goes() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(7, "minecraft:cow", [10.5, 64.0, 3.5]), start);
    assert_eq!(entities.len(), 1);
    let drawn = only(&entities, start);
    assert_eq!(drawn.group, Group::Animal);
    assert_eq!(drawn.size, [0.9f32, 1.4].map(f64::from));
    assert_eq!((drawn.pose.pitch, drawn.pose.yaw), (10.0, 90.0));

    // Only x changed: y and z keep what they were.
    assert!(entities.move_delta(&delta(7, MOVE_ACTOR_DELTA_FLAG_HAS_X, 11.5, 0.0), start + 50 * MS));
    let (_, _, latest) = entities.latest()[0];
    assert_eq!(latest.feet, DVec3::new(11.5, 64.0, 3.5));
    // The angles weren't sent, so they stay too.
    assert_eq!((latest.pitch, latest.yaw), (10.0, 90.0));

    assert!(entities.remove(7));
    assert!(entities.is_empty());
    assert!(entities.drawn(start).is_empty());
    // Gone, so a second removal or a move finds nothing.
    assert!(!entities.remove(7));
    assert!(!entities.move_delta(&delta(7, MOVE_ACTOR_DELTA_FLAG_HAS_X, 12.5, 0.0), start + 100 * MS));
    assert_eq!(entities.stats.strays, 2);
}

#[test]
fn an_unknown_identifier_is_a_block_and_is_counted() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(1, "minecraft:no_such_mob", [0.0, 70.0, 0.0]), start);
    entities.add_actor(&actor(2, "minecraft:no_such_mob", [4.0, 70.0, 0.0]), start);
    let drawn = entities.drawn(start);
    assert!(drawn.iter().all(|d| d.size == [1.0, 1.0] && d.group == Group::Other));
    assert_eq!(entities.stats.unknown.get("minecraft:no_such_mob"), Some(&2));
    // Known ones aren't counted.
    entities.add_actor(&actor(3, "minecraft:zombie", [8.0, 70.0, 0.0]), start);
    assert_eq!(entities.stats.unknown.len(), 1);
}

#[test]
fn items_and_players_stand_on_their_feet() {
    let start = Instant::now();
    let mut entities = Entities::default();
    // Bedrock puts an item's position 0.125 above its feet...
    entities.add_item(
        &CAddItemActor {
            target_actor_id: VarLong(5),
            target_runtime_id: VarULong(5),
            item: ItemStackWrapper {
                id: 1,
                stack_size: 3,
                aux_value: VarUInt(0),
                ..ItemStackWrapper::default()
            },
            position: Vector3::new(0.5, 64.125, 0.5),
            velocity: Vector3::new(0.0, 0.0, 0.0),
            entity_data: SyncedActorDataList::default(),
            is_from_fishing: false,
        },
        start,
    );
    let item = only(&entities, start);
    assert_eq!(item.pose.feet, DVec3::new(0.5, 64.0, 0.5));
    assert_eq!(item.group, Group::Item);
    assert_eq!(item.dropped, Some(Dropped { item: 1, aux: 0, count: 3 }));
    // ...and every move of it too.
    assert!(entities.move_delta(&delta(5, MOVE_ACTOR_DELTA_FLAG_HAS_Y, 0.0, 63.125), start + 50 * MS));
    assert_eq!(entities.latest()[0].2.feet.y, 63.0);
    assert!(entities.remove(5));

    // ...and a player's at its eyes.
    entities.add_player(&player(9, "Guest", [0.5, 65.62, 0.5]), start);
    let player = only(&entities, start);
    assert!((player.pose.feet.y - 64.0).abs() < 1e-5, "{:?}", player.pose.feet);
    assert_eq!(player.group, Group::Player);
    let entity = entities.by_runtime[&9];
    assert_eq!(entities.world.get::<&Name>(entity).unwrap().0, "Guest");
}

#[test]
fn a_baby_is_smaller_until_it_grows() {
    let start = Instant::now();
    let mut entities = Entities::default();
    let mut add = actor(3, "minecraft:cow", [0.0, 64.0, 0.0]);
    add.actor_data = baby_flags(true);
    entities.add_actor(&add, start);
    let [width, height] = only(&entities, start).size;
    assert!(width < 0.9 && height < 1.4, "{width} × {height}");
    assert!(entities.set_data(3, &baby_flags(false)));
    assert_eq!(only(&entities, start).size, [0.9f32, 1.4].map(f64::from));
    // Data that says nothing about either leaves the look alone.
    assert!(entities.set_data(3, &SyncedActorDataList::default()));
    assert_eq!(only(&entities, start).size, [0.9f32, 1.4].map(f64::from));
}

#[test]
fn a_frame_draws_entities_a_delay_late() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(1, "minecraft:pig", [0.0, 64.0, 0.0]), start);
    entities.move_delta(&delta(1, MOVE_ACTOR_DELTA_FLAG_HAS_X, 1.0, 0.0), start + 50 * MS);
    entities.move_delta(&delta(1, MOVE_ACTOR_DELTA_FLAG_HAS_X, 2.0, 0.0), start + 100 * MS);
    // 75 ms behind: at 25 ms, halfway to the first move.
    assert!((only(&entities, start + 100 * MS).pose.feet.x - 0.5).abs() < 1e-9);
    entities.forget_old(start + 150 * MS);
    assert!((only(&entities, start + 150 * MS).pose.feet.x - 1.5).abs() < 1e-9);
    // Two moves a tick apart: two gaps of 50 ms for the log.
    assert_eq!(entities.stats.gaps.count(50..=50), 2);
    assert_eq!(entities.stats.gaps.count(0..=1000), 2);
}

#[test]
fn a_teleport_draws_at_once() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(1, "minecraft:pig", [0.0, 64.0, 0.0]), start);
    let teleport = CMoveActorAbsolute {
        actor_runtime_id: VarULong(1),
        header: CMoveActorAbsolute::FLAG_TELEPORT,
        position: Vector3::new(100.0, 70.0, 0.0),
        rotation_x: 0,
        rotation_y: 64,
        rotation_y_head: 64,
    };
    assert!(entities.move_absolute(&teleport, start + 50 * MS));
    let drawn = only(&entities, start + 60 * MS);
    assert_eq!(drawn.pose.feet, DVec3::new(100.0, 70.0, 0.0));
    // 64 of 256 is a quarter turn.
    assert_eq!(drawn.pose.yaw, 90.0);
}

#[test]
fn the_same_id_again_replaces_the_entity() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(4, "minecraft:sheep", [0.0, 64.0, 0.0]), start);
    entities.add_actor(&actor(4, "minecraft:zombie", [5.0, 64.0, 0.0]), start + 10 * MS);
    assert_eq!(entities.len(), 1);
    let drawn = only(&entities, start + 10 * MS);
    assert_eq!(drawn.group, Group::Monster);
    assert_eq!(drawn.pose.feet.x, 5.0);
    assert!(entities.remove(4));
    assert!(entities.is_empty());
}

fn link(vehicle: i64, rider: i64, link_type: u8) -> ActorLink {
    ActorLink {
        ridden_unique_id: VarLong(vehicle),
        rider_unique_id: VarLong(rider),
        link_type,
        ..ActorLink::default()
    }
}

#[test]
fn a_rider_sits_on_its_vehicle_and_moves_with_it() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(1, "minecraft:chicken", [0.0, 64.0, 0.0]), start);
    let mut zombie = actor(2, "minecraft:zombie", [10.0, 64.0, 10.0]);
    zombie.actor_data = baby_flags(true);
    entities.add_actor(&zombie, start);
    entities.link(&link(1, 2, 1), start);

    let rider = |entities: &Entities| entities.latest().into_iter().find(|(id, _, _)| *id == 2).unwrap().2;
    let chicken = |entities: &Entities| entities.latest().into_iter().find(|(id, _, _)| *id == 1).unwrap().2;
    let seated = rider(&entities);
    let seat = seated.feet - chicken(&entities).feet;
    assert!(seat.y > 0.0 && seat.y < 1.0, "the jockey sits just above the chicken's feet: {seat}");

    // The chicken walks, and its rider goes with it, on the same seat.
    entities.move_delta(&delta(1, MOVE_ACTOR_DELTA_FLAG_HAS_X, 3.0, 0.0), start + 50 * MS);
    assert!((rider(&entities).feet - chicken(&entities).feet - seat).length() < 1e-9);
    assert_eq!(chicken(&entities).feet.x, 3.0);
    // And so does the rider's frame, a delay late.
    let drawn = entities.drawn(start + 100 * MS);
    let at = |id: u64| drawn.iter().find(|d| d.runtime_id == id).unwrap().pose.feet;
    assert!((at(2) - at(1) - seat).length() < 1e-9);

    // Off again: it stays where it sat, and doesn't flash back to (10, 64, 10).
    let sat = rider(&entities);
    entities.link(&link(1, 2, 0), start + 100 * MS);
    assert_eq!(rider(&entities).feet, sat.feet);
    assert!((only_of(&entities, 2, start + 110 * MS).feet - sat.feet).length() < 1e-9);
}

#[test]
fn a_rider_whose_vehicle_goes_is_drawn_on_its_own() {
    let start = Instant::now();
    let mut entities = Entities::default();
    entities.add_actor(&actor(1, "minecraft:chicken", [0.0, 64.0, 0.0]), start);
    entities.add_actor(&actor(2, "minecraft:zombie", [10.0, 64.0, 10.0]), start);
    entities.link(&link(1, 2, 1), start);
    assert!(entities.remove(1));
    let (_, _, pose) = entities.latest()[0];
    assert_eq!(pose.feet, DVec3::new(10.0, 64.0, 10.0));
    // A link to something the client doesn't have is a stray.
    entities.link(&link(99, 2, 1), start);
    assert_eq!(entities.stats.strays, 1);
}

fn only_of(entities: &Entities, id: u64, now: Instant) -> Pose {
    entities.drawn(now).into_iter().find(|d| d.runtime_id == id).unwrap().pose
}

#[test]
fn gaps_count_every_move_by_the_millisecond() {
    let mut gaps = Gaps::default();
    assert_eq!(gaps.percentile(75, 0.5), None);
    for ms in 1..=100 {
        gaps.add(Duration::from_millis(ms));
    }
    gaps.add(Duration::from_secs(10));
    // Of the 75 up to 75 ms: the median is the 38th, and the 95th percentile the 72nd.
    assert_eq!(gaps.percentile(75, 0.5), Some(38));
    assert_eq!(gaps.percentile(75, 0.95), Some(72));
    assert_eq!(gaps.count(0..=75), 75);
    assert_eq!(gaps.count(76..=125), 25);
    // Ten seconds counts among the longest.
    assert_eq!(gaps.count(126..=LONGEST_GAP + 1), 1);
    assert_eq!(gaps.count(0..=usize::MAX), 101);
}
