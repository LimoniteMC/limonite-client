//! Where items can be: the player's slots, and how the server names them (m5-plan
//! Step 12).
//!
//! **Two numberings** (worth teaching). Bedrock sends the player's slots in containers
//! by number: container 0 is the 36 slots, the hotbar first; 120 is the armour, head to
//! feet; 119 the off hand. But an item stack request names a slot by what it's for: the
//! hotbar is `HotBar` slots 0 to 8, the 27 above it are `Inventory` slots 9 to 35, and the
//! stack on the pointer is `Cursor` slot 0. [`SlotRef`] is one slot, whichever way it's
//! named.

use std::num::NonZero;

use pumpkin_data::{
    attributes::Attributes,
    data_component_impl::{AttributeModifiersImpl, EquipmentSlot, EquippableImpl, Operation},
};
use pumpkin_protocol::bedrock::network_item::{ContainerName, NetworkItemStackDescriptor};

use super::{Item, MAIN, items};

/// A slot's stack, as the server sends it.
pub type Stack = NetworkItemStackDescriptor;

/// The containers the player's slots are sent in.
pub const CONTAINER_MAIN: u32 = 0;
pub const CONTAINER_ARMOUR: u32 = 120;
pub const CONTAINER_OFFHAND: u32 = 119;
/// Pumpkin's number for the cursor, which it empties when the screen closes.
pub const CONTAINER_CURSOR: u32 = 59;
/// How many armour slots there are.
pub const ARMOUR: usize = 4;
/// The first of the 36 that isn't on the hotbar.
const FIRST_ABOVE: u8 = 9;

/// A slot the player can see.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotRef {
    /// One of the 36: 0 to 8 are the hotbar, and 9 to 35 the rows above it.
    Main(u8),
    /// Head, chest, legs and feet: 0 to 3.
    Armour(u8),
    Offhand,
    /// What the pointer carries while a screen is open.
    Cursor,
}

impl SlotRef {
    /// How a request names this slot.
    #[must_use]
    pub const fn container(self) -> (ContainerName, u8) {
        match self {
            Self::Main(slot) if slot < FIRST_ABOVE => (ContainerName::HotBar, slot),
            Self::Main(slot) => (ContainerName::Inventory, slot),
            Self::Armour(slot) => (ContainerName::Armor, slot),
            Self::Offhand => (ContainerName::Offhand, 0),
            Self::Cursor => (ContainerName::Cursor, 0),
        }
    }

    /// The slot a response names, if it's one the client keeps.
    #[must_use]
    pub fn from_container(name: ContainerName, slot: u8) -> Option<Self> {
        let main = usize::from(slot) < MAIN;
        match name {
            ContainerName::HotBar if slot < FIRST_ABOVE => Some(Self::Main(slot)),
            ContainerName::Inventory | ContainerName::CombinedHotBarAndInventory if main => Some(Self::Main(slot)),
            ContainerName::Armor if usize::from(slot) < ARMOUR => Some(Self::Armour(slot)),
            ContainerName::Offhand if slot == 0 => Some(Self::Offhand),
            ContainerName::Cursor if slot == 0 => Some(Self::Cursor),
            _ => None,
        }
    }

    /// Whether this slot is on the hotbar.
    #[must_use]
    pub const fn on_hotbar(self) -> bool {
        matches!(self, Self::Main(slot) if slot < FIRST_ABOVE)
    }
}

/// Everything the player carries.
#[derive(Clone, Debug)]
pub struct Slots {
    pub main: Vec<Stack>,
    pub armour: [Stack; ARMOUR],
    pub offhand: Stack,
    pub cursor: Stack,
}

impl Default for Slots {
    fn default() -> Self {
        Self {
            main: vec![Stack::default(); MAIN],
            armour: std::array::from_fn(|_| Stack::default()),
            offhand: Stack::default(),
            cursor: Stack::default(),
        }
    }
}

impl Slots {
    #[must_use]
    pub fn get(&self, slot: SlotRef) -> &Stack {
        match slot {
            SlotRef::Main(slot) => &self.main[usize::from(slot)],
            SlotRef::Armour(slot) => &self.armour[usize::from(slot)],
            SlotRef::Offhand => &self.offhand,
            SlotRef::Cursor => &self.cursor,
        }
    }

    pub fn get_mut(&mut self, slot: SlotRef) -> &mut Stack {
        match slot {
            SlotRef::Main(slot) => &mut self.main[usize::from(slot)],
            SlotRef::Armour(slot) => &mut self.armour[usize::from(slot)],
            SlotRef::Offhand => &mut self.offhand,
            SlotRef::Cursor => &mut self.cursor,
        }
    }

    /// What a slot shows, unless it's empty.
    #[must_use]
    pub fn item(&self, slot: SlotRef) -> Option<Item> {
        super::item_of(self.get(slot))
    }
}

/// Whether a stack holds nothing.
#[must_use]
pub const fn is_empty(stack: &Stack) -> bool {
    stack.id == 0 || stack.stack_size == 0
}

/// Whether two stacks are of the same thing, so they can go together. The block id
/// isn't compared: it follows from the item, and Pumpkin always sends 0.
#[must_use]
pub fn same_item(a: &Stack, b: &Stack) -> bool {
    a.id == b.id && a.aux_value == b.aux_value && a.extra_data == b.extra_data
}

/// How many of the item a slot can hold, from its Java item. Anything the table doesn't
/// know stacks to 64, as most things do.
#[must_use]
pub fn max_stack(stack: &Stack) -> u8 {
    if is_empty(stack) {
        return 64;
    }
    let java = items::held_stack(stack.id, stack.aux_value.0);
    if java.is_empty() { 64 } else { java.get_max_stack_size().max(1) }
}

/// A copy of `stack` with `count` in it: none is an empty slot.
#[must_use]
pub fn with_count(stack: &Stack, count: u16) -> Stack {
    if count == 0 {
        return Stack::default();
    }
    let mut stack = stack.clone();
    stack.stack_size = count;
    stack
}

/// Whether `stack` may go in `slot`, as the server decides: armour only where it's worn,
/// one at a time. Everything goes everywhere else.
#[must_use]
pub fn fits(stack: &Stack, slot: SlotRef) -> bool {
    let SlotRef::Armour(index) = slot else {
        return true;
    };
    let java = items::held_stack(stack.id, stack.aux_value.0);
    java.get_data_component::<EquippableImpl>().is_some_and(|equippable| {
        let wanted = match index {
            0 => EquipmentSlot::HEAD,
            1 => EquipmentSlot::CHEST,
            2 => EquipmentSlot::LEGS,
            _ => EquipmentSlot::FEET,
        };
        std::mem::discriminant(equippable.slot) == std::mem::discriminant(&wanted)
    })
}

/// How many of `item` a slot holds at most: armour holds one.
#[must_use]
pub fn capacity(item: &Stack, slot: SlotRef) -> u16 {
    if matches!(slot, SlotRef::Armour(_)) { 1 } else { u16::from(max_stack(item)) }
}

/// The armour points what's worn gives, as vanilla works them out: each piece adds its
/// armour modifier, from 0. The HUD's armour row shows them, two to an icon.
#[must_use]
pub fn armour_points(slots: &Slots) -> u32 {
    let worn = slots.armour.iter().filter(|stack| !is_empty(stack));
    let total: f64 = worn
        .filter_map(|stack| {
            let java = items::held_stack(stack.id, stack.aux_value.0);
            let modifiers = java.get_data_component::<AttributeModifiersImpl>()?;
            let armour = modifiers.attribute_modifiers.iter().filter(|modifier| {
                modifier.r#type.id == Attributes::ARMOR.id && matches!(modifier.operation, Operation::AddValue)
            });
            Some(armour.map(|modifier| modifier.amount).sum::<f64>())
        })
        .sum();
    total.max(0.0).round() as u32
}

/// The stack's id from the server, or 0 if it has none.
#[must_use]
pub fn net_id(stack: &Stack) -> i32 {
    if is_empty(stack) { 0 } else { stack.net_id.map_or(0, NonZero::get) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_is_named_as_the_capture_names_it() {
        assert_eq!(SlotRef::Main(0).container(), (ContainerName::HotBar, 0));
        assert_eq!(SlotRef::Main(8).container(), (ContainerName::HotBar, 8));
        assert_eq!(SlotRef::Main(27).container(), (ContainerName::Inventory, 27));
        assert_eq!(SlotRef::Cursor.container(), (ContainerName::Cursor, 0));
        for slot in [SlotRef::Main(0), SlotRef::Main(35), SlotRef::Armour(3), SlotRef::Offhand, SlotRef::Cursor] {
            let (name, index) = slot.container();
            assert_eq!(SlotRef::from_container(name, index), Some(slot));
        }
        // A response may name the hotbar's slots as part of the inventory.
        assert_eq!(SlotRef::from_container(ContainerName::Inventory, 3), Some(SlotRef::Main(3)));
        assert_eq!(SlotRef::from_container(ContainerName::CreatedOutput, 50), None);
        assert_eq!(SlotRef::from_container(ContainerName::Inventory, 36), None);
    }

    #[test]
    fn what_is_worn_gives_its_armour_points() {
        let piece = |id| Stack {
            id,
            stack_size: 1,
            ..Stack::default()
        };
        let mut slots = Slots::default();
        assert_eq!(armour_points(&slots), 0);
        // An iron helmet and iron leggings: 2 and 5, as in Java.
        slots.armour[0] = piece(pumpkin_data::item::BedrockItem::IRON_HELMET.id);
        slots.armour[2] = piece(pumpkin_data::item::BedrockItem::IRON_LEGGINGS.id);
        assert_eq!(armour_points(&slots), 7);
    }
}
