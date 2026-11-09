//! What an entity is, from the name the server sends: its size, and which group it's
//! drawn in.
//!
//! **The server's own table, turned round.** Pumpkin names each entity for Bedrock with
//! `entity_type_to_bedrock_identifier`: Java's name, apart from a few that Bedrock calls
//! something else (`xp_orb`, `thrown_trident`, ...). The client links the same server,
//! so it builds its table from that function and `pumpkin-data`'s list of every entity
//! type. A name the server can send can't be missing here, and a sync that renames one
//! changes both sides at once.

use std::{collections::HashMap, sync::LazyLock};

use glam::DVec3;
use pumpkin_core::entity::entity_type_to_bedrock_identifier;
use pumpkin_data::entity::{EntityAttachments, EntityType, MobCategory};

/// How an entity is drawn until milestone 8 gives it a model: its box in one colour per
/// group (m5-plan Decision 13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Monster,
    /// Every other mob: animals, villagers, golems, fish.
    Animal,
    Player,
    /// A dropped item, which is drawn as its icon rather than a box.
    Item,
    /// Everything else: minecarts, boats, arrows, falling blocks, experience orbs.
    Other,
}

impl Group {
    /// The box's colour, sRGB-encoded.
    #[must_use]
    pub const fn colour(self) -> [u8; 4] {
        match self {
            Self::Monster => [196, 64, 56, 255],
            Self::Animal => [96, 176, 72, 255],
            Self::Player => [72, 120, 216, 255],
            Self::Item => [232, 200, 64, 255],
            Self::Other => [176, 176, 176, 255],
        }
    }

    /// The group of a known entity type.
    #[must_use]
    pub fn of(entity_type: &EntityType) -> Self {
        if entity_type.id == EntityType::PLAYER.id {
            Self::Player
        } else if entity_type.id == EntityType::ITEM.id {
            Self::Item
        } else if *entity_type.category == MobCategory::MONSTER || entity_type.is_enemy {
            Self::Monster
        } else if entity_type.mob {
            Self::Animal
        } else {
            Self::Other
        }
    }
}

/// Bedrock's names the server sends that `entity_type_to_bedrock_identifier` doesn't
/// give: a mob can name itself (`Mob::mob_bedrock_identifier`), and only the villager
/// does.
const MOB_NAMES: &[(&str, &EntityType)] = &[("villager_v2", &EntityType::VILLAGER)];

/// Every name the server sends, without its `minecraft:`, to its entity type.
static BY_NAME: LazyLock<HashMap<&'static str, &'static EntityType>> = LazyLock::new(|| {
    let mut names: HashMap<&'static str, &'static EntityType> = EntityType::ALL
        .iter()
        .map(|&entity_type| (short(entity_type_to_bedrock_identifier(entity_type)), entity_type))
        .collect();
    names.extend(MOB_NAMES.iter().copied());
    names
});

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// The entity type a Bedrock name stands for, with or without its `minecraft:`.
#[must_use]
pub fn entity_type(identifier: &str) -> Option<&'static EntityType> {
    BY_NAME.get(short(identifier)).copied()
}

/// How far above its feet Bedrock puts an entity's position, so the client can take the
/// feet back. The same as Pumpkin's `bedrock_y_offset` overrides: a player's eyes, an
/// item's 0.125 and a falling block's 0.49 (`entity/player.rs`, `item.rs`, `falling.rs`).
#[must_use]
pub fn network_height(entity_type: Option<&EntityType>) -> f64 {
    match entity_type {
        Some(entity_type) if entity_type.id == EntityType::PLAYER.id => f64::from(entity_type.eye_height),
        Some(entity_type) if entity_type.id == EntityType::ITEM.id => 0.125,
        Some(entity_type) if entity_type.id == EntityType::FALLING_BLOCK.id => 0.49,
        _ => 0.0,
    }
}

/// Where a rider sits on its vehicle, from the vehicle's feet: the vehicle's seat `index`,
/// turned to its yaw, less the rider's own point that goes on the seat, turned to the
/// rider's yaw. The server's default seating (`position_rider` in Pumpkin's `entity/mod.rs`),
/// from the attachment points `pumpkin-data` has from Java. Pumpkin overrides it for
/// boats, some minecarts and some mobs, which come out near, not exact. `None` when the
/// vehicle has no seats, or either type is unknown.
#[must_use]
pub fn seat(vehicle: Riding, index: usize, rider: Riding) -> Option<DVec3> {
    let vehicle_points = attachments(vehicle.entity_type?, vehicle.baby);
    let seat = *vehicle_points.passenger.get(index).or(vehicle_points.passenger.last())?;
    let own = attachments(rider.entity_type?, rider.baby).vehicle;
    Some(attachment_point(seat, vehicle.scale, vehicle.yaw) - attachment_point(own, rider.scale, rider.yaw))
}

/// What seating needs of an entity: its type, whether it's a baby, its scale and yaw.
#[derive(Clone, Copy, Debug)]
pub struct Riding {
    pub entity_type: Option<&'static EntityType>,
    pub baby: bool,
    pub scale: f32,
    pub yaw: f32,
}

fn attachments(entity_type: &'static EntityType, baby: bool) -> &'static EntityAttachments {
    entity_type.baby_attachments.as_ref().filter(|_| baby).unwrap_or(&entity_type.attachments)
}

/// A point on an entity, scaled and turned round y to its yaw, as Pumpkin's
/// `attachment_point` turns it, in `f32` trigonometry to give the same numbers.
fn attachment_point([x, y, z]: [f64; 3], scale: f32, yaw: f32) -> DVec3 {
    let scale = f64::from(scale);
    let (x, y, z) = (x * scale, y * scale, z * scale);
    let angle = -yaw.to_radians();
    let (sin, cos) = (f64::from(angle.sin()), f64::from(angle.cos()));
    DVec3::new(x * cos + z * sin, y, z * cos - x * sin)
}

/// An entity's box, width and height in blocks: its type's, or its type's as a baby,
/// times its scale. An unknown type's is a block.
#[must_use]
pub fn size(entity_type: Option<&EntityType>, baby: bool, scale: f32) -> [f64; 2] {
    let [width, height] = match entity_type {
        Some(entity_type) => match (baby, entity_type.baby_dimensions.as_ref()) {
            (true, Some(baby)) => [baby.width, baby.height],
            _ => entity_type.dimension,
        },
        None => [1.0, 1.0],
    };
    [f64::from(width * scale), f64::from(height * scale)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_bedrocks_with_or_without_the_namespace() {
        assert_eq!(entity_type("minecraft:cow").map(|t| t.id), Some(EntityType::COW.id));
        assert_eq!(entity_type("cow").map(|t| t.id), Some(EntityType::COW.id));
        // Where Bedrock's name isn't Java's.
        assert_eq!(entity_type("minecraft:xp_orb").map(|t| t.id), Some(EntityType::EXPERIENCE_ORB.id));
        assert_eq!(entity_type("minecraft:thrown_trident").map(|t| t.id), Some(EntityType::TRIDENT.id));
        assert_eq!(entity_type("minecraft:villager_v2").map(|t| t.id), Some(EntityType::VILLAGER.id));
        assert_eq!(entity_type("minecraft:zombie_villager_v2").map(|t| t.id), Some(EntityType::ZOMBIE_VILLAGER.id));
        assert!(entity_type("minecraft:no_such_thing").is_none());
    }

    #[test]
    fn every_type_the_server_can_name_is_found() {
        for &entity_type in EntityType::ALL {
            let name = entity_type_to_bedrock_identifier(entity_type);
            assert_eq!(super::entity_type(name).map(|t| t.id), Some(entity_type.id), "{name}");
        }
    }

    #[test]
    fn groups() {
        assert_eq!(Group::of(&EntityType::ZOMBIE), Group::Monster);
        assert_eq!(Group::of(&EntityType::CREEPER), Group::Monster);
        assert_eq!(Group::of(&EntityType::COW), Group::Animal);
        assert_eq!(Group::of(&EntityType::VILLAGER), Group::Animal);
        assert_eq!(Group::of(&EntityType::COD), Group::Animal);
        assert_eq!(Group::of(&EntityType::PLAYER), Group::Player);
        assert_eq!(Group::of(&EntityType::ITEM), Group::Item);
        assert_eq!(Group::of(&EntityType::MINECART), Group::Other);
        assert_eq!(Group::of(&EntityType::ARROW), Group::Other);
    }

    #[test]
    fn sizes() {
        assert_eq!(size(Some(&EntityType::COW), false, 1.0), [0.9f32, 1.4].map(f64::from));
        let [width, height] = size(Some(&EntityType::COW), true, 1.0);
        assert!(width < 0.9 && height < 1.4, "a calf is smaller: {width} × {height}");
        // A baby of a type with no baby size is its usual size.
        assert_eq!(size(Some(&EntityType::CREEPER), true, 1.0), size(Some(&EntityType::CREEPER), false, 1.0));
        assert_eq!(size(None, false, 1.0), [1.0, 1.0]);
        assert_eq!(size(None, false, 2.0), [2.0, 2.0]);
    }

    #[test]
    fn a_rider_sits_on_its_vehicles_seat() {
        let on = |entity_type: &'static EntityType, baby: bool, yaw: f32| Riding {
            entity_type: Some(entity_type),
            baby,
            scale: 1.0,
            yaw,
        };
        // A baby zombie on a chicken, the chicken jockey: the chicken's seat, less the
        // point of the zombie that sits on it.
        let chicken = on(&EntityType::CHICKEN, false, 0.0);
        let zombie = on(&EntityType::ZOMBIE, true, 0.0);
        let points = &EntityType::CHICKEN.attachments;
        let own = EntityType::ZOMBIE.baby_attachments.as_ref().unwrap().vehicle;
        let seat = seat(chicken, 0, zombie).unwrap();
        assert!((seat.y - (points.passenger[0][1] - own[1])).abs() < 1e-9, "{seat}");
        // A seat off the middle turns with the vehicle: a quarter turn takes +z to -x.
        let turned = attachment_point([0.0, 1.0, 1.0], 1.0, 90.0);
        assert!((turned - DVec3::new(-1.0, 1.0, 0.0)).length() < 1e-6, "{turned}");
        // No seat, or an unknown type: not seated.
        let unknown = Riding {
            entity_type: None,
            baby: false,
            scale: 1.0,
            yaw: 0.0,
        };
        assert!(super::seat(unknown, 0, zombie).is_none());
        assert!(super::seat(chicken, 0, unknown).is_none());
    }

    #[test]
    fn network_heights_are_the_servers() {
        assert!((network_height(Some(&EntityType::PLAYER)) - 1.62).abs() < 1e-6);
        assert!((network_height(Some(&EntityType::ITEM)) - 0.125).abs() < 1e-9);
        assert!((network_height(Some(&EntityType::FALLING_BLOCK)) - 0.49).abs() < 1e-9);
        assert_eq!(network_height(Some(&EntityType::COW)), 0.0);
        assert_eq!(network_height(None), 0.0);
    }
}
