//! What the player carries, as the server says (m5-plan Decision 10).
//!
//! **A mirror.** The server owns the inventory, and tells the client with
//! `InventoryContent` and `InventorySlot`, and which slot is selected with
//! `PlayerHotbar`. The client keeps a copy, the stacks exactly as they came, because it
//! has to hand them back: `MobEquipment` and a drop both name the item they're about.
//!
//! The first nine of the player's 36 slots are the hotbar. Pumpkin doesn't send a Bedrock
//! player's armour or off hand, so those wait for Step 12's own moves.

pub mod items;

use pumpkin_protocol::{
    bedrock::{
        network_item::{NetworkItemDescriptor, NetworkItemStackDescriptor},
        server::inventory_transaction::{
            InventoryAction, LegacySetItemSlot, NormalTransactionData, SInventoryTransaction,
            TransactionData, WINDOW_ID_INVENTORY,
        },
    },
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::{PacketRead, PacketWrite},
};

/// The slots of the player's inventory.
pub const MAIN: usize = 36;
/// The first of them, which the hotbar shows.
pub const HOTBAR: usize = 9;
/// The container the player's inventory goes by.
pub const CONTAINER_MAIN: u32 = 0;
/// The container a transaction's prediction names for a slot of the player's inventory.
const CONTAINER_PREDICTED: u8 = 29;
/// A transaction's source that's a container.
const SOURCE_CONTAINER: u32 = 0;
/// A transaction's source that's the world: what's dropped goes there.
const SOURCE_WORLD: u32 = 2;

/// What a slot holds, as far as anyone looking at it can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: i16,
    pub aux: u32,
    pub count: u16,
}

/// A slot's stack as an [`Item`], unless it's empty.
fn item_of(stack: &NetworkItemStackDescriptor) -> Option<Item> {
    (stack.id != 0 && stack.stack_size > 0).then_some(Item {
        id: stack.id,
        aux: stack.aux_value.0,
        count: stack.stack_size,
    })
}

/// The player's inventory.
#[derive(Debug)]
pub struct Inventory {
    main: Vec<NetworkItemStackDescriptor>,
    selected: u8,
    /// The id of the last transaction that predicted a change: they fall by 2.
    last_transaction: i32,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            main: vec![NetworkItemStackDescriptor::default(); MAIN],
            selected: 0,
            last_transaction: 0,
        }
    }
}

impl Inventory {
    /// Takes the whole of it. A short list leaves the rest empty.
    pub fn set_main(&mut self, slots: Vec<NetworkItemStackDescriptor>) {
        self.main = slots;
        self.main.resize(MAIN, NetworkItemStackDescriptor::default());
    }

    /// Takes one slot. A slot past the end is ignored.
    pub fn set_slot(&mut self, slot: usize, stack: NetworkItemStackDescriptor) {
        if let Some(place) = self.main.get_mut(slot) {
            *place = stack;
        }
    }

    /// The selected hotbar slot, 0 to 8.
    #[must_use]
    pub const fn selected(&self) -> u8 {
        self.selected
    }

    /// Selects a hotbar slot. Says whether that was a change, and a slot there is.
    pub fn select(&mut self, slot: u8) -> bool {
        if usize::from(slot) >= HOTBAR || slot == self.selected {
            return false;
        }
        self.selected = slot;
        true
    }

    /// The slot to the right of the selected one, round to the first.
    #[must_use]
    pub const fn next_slot(&self) -> u8 {
        (self.selected + 1) % HOTBAR as u8
    }

    /// The slot to the left of the selected one, round to the last.
    #[must_use]
    pub const fn previous_slot(&self) -> u8 {
        (self.selected + HOTBAR as u8 - 1) % HOTBAR as u8
    }

    /// The stack in the selected slot.
    #[must_use]
    pub fn held(&self) -> &NetworkItemStackDescriptor {
        &self.main[usize::from(self.selected)]
    }

    /// What the hotbar shows.
    #[must_use]
    pub fn hotbar(&self) -> [Option<Item>; HOTBAR] {
        std::array::from_fn(|slot| item_of(&self.main[slot]))
    }

    /// Takes one of the held item out of the hand, and returns the transaction that
    /// tells the server it was dropped. `None` if the hand is empty.
    pub fn drop_one(&mut self) -> Option<SInventoryTransaction> {
        let slot = usize::from(self.selected);
        item_of(&self.main[slot])?;
        let old = action_item(&self.main[slot])?;
        let stack = &mut self.main[slot];
        stack.stack_size -= 1;
        if stack.stack_size == 0 {
            *stack = NetworkItemStackDescriptor::default();
        }
        self.last_transaction -= 2;
        Some(drop_transaction(self.last_transaction, slot as u32, old))
    }
}

/// A slot's stack in the form transactions use, which also has its NBT read. The two are
/// the same on the wire.
fn action_item(stack: &NetworkItemStackDescriptor) -> Option<NetworkItemDescriptor> {
    let mut bytes = Vec::new();
    stack.write(&mut bytes).ok()?;
    NetworkItemDescriptor::read(&mut bytes.as_slice()).ok()
}

/// What Q sends, as the official client does (m5-plan Results, Step 1): a normal
/// transaction of two actions, the slot losing one, and the world gaining it, with the
/// slot it predicts a change to named at the front.
fn drop_transaction(id: i32, slot: u32, old: NetworkItemDescriptor) -> SInventoryTransaction {
    let mut left = old.clone();
    left.stack_size -= 1;
    if left.stack_size == 0 {
        left = NetworkItemDescriptor::default();
    }
    let mut dropped = old.clone();
    dropped.stack_size = 1;
    SInventoryTransaction {
        legacy_request_id: VarInt(id),
        legacy_set_item_slots: vec![LegacySetItemSlot {
            container_id: CONTAINER_PREDICTED,
            slots: vec![slot as u8],
        }],
        has_value: true,
        actions: vec![
            InventoryAction {
                source_type: SOURCE_CONTAINER,
                window_id: Some(WINDOW_ID_INVENTORY),
                source_flags: None,
                inventory_slot: slot,
                old_item: old,
                new_item: left,
            },
            InventoryAction {
                source_type: SOURCE_WORLD,
                window_id: None,
                source_flags: Some(0),
                inventory_slot: 0,
                old_item: NetworkItemDescriptor::default(),
                new_item: dropped,
            },
        ],
        transaction_type: VarUInt(0),
        transaction_data: TransactionData::Normal(NormalTransactionData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: i16, count: u16) -> NetworkItemStackDescriptor {
        NetworkItemStackDescriptor {
            id,
            stack_size: count,
            ..NetworkItemStackDescriptor::default()
        }
    }

    #[test]
    fn it_starts_empty_with_the_first_slot_selected() {
        let inventory = Inventory::default();
        assert_eq!(inventory.selected(), 0);
        assert_eq!(inventory.hotbar(), [None; HOTBAR]);
    }

    #[test]
    fn the_hotbar_is_the_first_nine_slots() {
        let mut inventory = Inventory::default();
        inventory.set_slot(0, stack(5, 3));
        inventory.set_slot(8, stack(6, 1));
        inventory.set_slot(9, stack(7, 1));
        let hotbar = inventory.hotbar();
        assert_eq!(hotbar[0], Some(Item { id: 5, aux: 0, count: 3 }));
        assert_eq!(hotbar[8], Some(Item { id: 6, aux: 0, count: 1 }));
        assert_eq!(hotbar[1], None);
    }

    #[test]
    fn a_whole_list_replaces_it_and_a_short_one_leaves_the_rest_empty() {
        let mut inventory = Inventory::default();
        inventory.set_slot(5, stack(9, 1));
        inventory.set_main(vec![stack(5, 1)]);
        let hotbar = inventory.hotbar();
        assert_eq!(hotbar[0].map(|item| item.id), Some(5));
        assert_eq!(hotbar[5], None);
        inventory.set_slot(40, stack(9, 1));
        assert_eq!(inventory.main.len(), MAIN, "a slot past the end is ignored");
    }

    #[test]
    fn an_empty_stack_with_an_id_is_still_empty() {
        let mut inventory = Inventory::default();
        inventory.set_slot(0, stack(5, 0));
        assert_eq!(inventory.hotbar()[0], None);
    }

    #[test]
    fn selecting_says_if_it_changed_anything() {
        let mut inventory = Inventory::default();
        assert!(inventory.select(3));
        assert_eq!(inventory.selected(), 3);
        assert!(!inventory.select(3), "the same slot");
        assert!(!inventory.select(9), "no such slot");
        assert_eq!(inventory.selected(), 3);
    }

    #[test]
    fn the_wheel_wraps_around_the_hotbar() {
        let mut inventory = Inventory::default();
        assert_eq!(inventory.previous_slot(), 8);
        assert_eq!(inventory.next_slot(), 1);
        inventory.select(8);
        assert_eq!(inventory.next_slot(), 0);
        assert_eq!(inventory.previous_slot(), 7);
    }

    #[test]
    fn dropping_takes_one_from_the_hand() {
        let mut inventory = Inventory::default();
        inventory.set_slot(2, stack(5, 2));
        inventory.select(2);
        let transaction = inventory.drop_one().unwrap();
        assert_eq!(inventory.hotbar()[2].map(|item| item.count), Some(1));
        assert_eq!(transaction.legacy_request_id.0, -2);
        let (slot, world) = (&transaction.actions[0], &transaction.actions[1]);
        assert_eq!(world.source_type, SOURCE_WORLD);
        assert_eq!(world.new_item.stack_size, 1);
        assert_eq!(slot.source_type, SOURCE_CONTAINER);
        assert_eq!(slot.inventory_slot, 2);
        assert_eq!((slot.old_item.stack_size, slot.new_item.stack_size), (2, 1));

        // The last one empties the slot.
        let transaction = inventory.drop_one().unwrap();
        assert_eq!(inventory.hotbar()[2], None);
        assert_eq!(transaction.actions[0].new_item.id.0, 0);
        assert_eq!(transaction.legacy_request_id.0, -4, "ids fall by 2");
        assert!(inventory.drop_one().is_none(), "an empty hand drops nothing");
    }

    /// Q, byte for byte, against what the official client sent.
    #[test]
    fn a_drop_matches_the_official_client() {
        let fixture: &[u8] = include_bytes!(
            "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/inventory_transaction-drop.bin"
        );
        let official = SInventoryTransaction::read(&mut &*fixture).unwrap();
        let slot = official.actions.iter().find(|action| action.source_type == SOURCE_CONTAINER).unwrap();
        let ours = drop_transaction(official.legacy_request_id.0, slot.inventory_slot, slot.old_item.clone());
        let mut bytes = Vec::new();
        ours.write(&mut bytes).unwrap();
        assert_eq!(bytes, fixture);
    }
}
