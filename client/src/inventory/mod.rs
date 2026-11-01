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

use glam::IVec3;
use pumpkin_protocol::{
    bedrock::{
        network_item::{NetworkItemDescriptor, NetworkItemStackDescriptor},
        server::inventory_transaction::{
            HandSlot, InventoryAction, LegacySetItemSlot, NormalTransactionData, SInventoryTransaction,
            TransactionData, UseItemTransactionData, WINDOW_ID_INVENTORY,
        },
    },
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::{PacketRead, PacketWrite},
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

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
/// A use-item transaction's kinds: clicking a block, and clicking air.
const USE_CLICK_BLOCK: i32 = 0;
const USE_CLICK_AIR: i32 = 1;
/// What made it happen: the player, for a click on a block.
const TRIGGER_PLAYER: u8 = 1;
/// The face a click on air names.
const NO_FACE: u8 = 255;

/// A right click on a block, as a use-item transaction says it.
#[derive(Clone, Copy, Debug)]
pub struct Click {
    pub block: IVec3,
    pub face: u8,
    /// Where the player's eyes are, which is where Bedrock says a player is.
    pub eyes: Vector3<f32>,
    /// Where on the block, from its lowest corner.
    pub at: Vector3<f32>,
    /// What's there.
    pub runtime_id: u32,
}

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

    /// A right click on a block. If it `takes_one`, the hand loses one, and the
    /// transaction says so. `predicted` says the client expects it to place something.
    pub fn click_block(&mut self, click: &Click, predicted: bool, takes_one: bool) -> SInventoryTransaction {
        let slot = usize::from(self.selected);
        let held = action_item(&self.main[slot]).unwrap_or_default();
        let change = (takes_one && item_of(&self.main[slot]).is_some()).then(|| {
            // Each click takes two ids in the capture, whichever transaction the second is for.
            self.last_transaction -= 4;
            let stack = &mut self.main[slot];
            stack.stack_size -= 1;
            if stack.stack_size == 0 {
                *stack = NetworkItemStackDescriptor::default();
                // The capture's last item goes without an id.
                (0, NetworkItemDescriptor::default())
            } else {
                let mut left = held.clone();
                left.stack_size -= 1;
                (self.last_transaction, left)
            }
        });
        click_transaction(slot as u32, held, change, click, predicted)
    }

    /// The click on air that follows a click on a block, naming what's in hand by then,
    /// and the one that's all there is when nothing is aimed at.
    #[must_use]
    pub fn click_air(&self, eyes: Vector3<f32>) -> SInventoryTransaction {
        let slot = usize::from(self.selected);
        click_air_transaction(slot as u32, action_item(&self.main[slot]).unwrap_or_default(), eyes)
    }
}

/// A click on a block (m5-plan Results, Step 1). `change` is an id and what the slot
/// will hold, if it's predicted to change: a legacy line then names the slot, unless
/// the id is 0, and an action says what happened to it.
fn click_transaction(
    slot: u32,
    held: NetworkItemDescriptor,
    change: Option<(i32, NetworkItemDescriptor)>,
    click: &Click,
    predicted: bool,
) -> SInventoryTransaction {
    let id = change.as_ref().map_or(0, |(id, _)| *id);
    let legacy_set_item_slots = if id == 0 {
        Vec::new()
    } else {
        vec![LegacySetItemSlot {
            container_id: CONTAINER_PREDICTED,
            slots: vec![slot as u8],
        }]
    };
    let actions: Vec<InventoryAction> = change
        .map(|(_, left)| InventoryAction {
            source_type: SOURCE_CONTAINER,
            window_id: Some(WINDOW_ID_INVENTORY),
            source_flags: None,
            inventory_slot: slot,
            old_item: held.clone(),
            new_item: left,
        })
        .into_iter()
        .collect();
    SInventoryTransaction {
        legacy_request_id: VarInt(id),
        legacy_set_item_slots,
        has_value: !actions.is_empty(),
        actions,
        transaction_type: VarUInt(2),
        transaction_data: TransactionData::UseItem(UseItemTransactionData {
            action_type: VarInt(USE_CLICK_BLOCK),
            trigger_type: TRIGGER_PLAYER,
            block_position: BlockPos::new(click.block.x, click.block.y, click.block.z),
            block_face: click.face,
            hot_bar_slot: VarInt(slot as i32),
            hand: HandSlot::Mainhand,
            item_in_hand: held,
            player_position: click.eyes,
            click_position: click.at,
            block_runtime_id: VarUInt(click.runtime_id),
            client_prediction: u8::from(predicted),
            client_cooldown_state: 0,
        }),
    }
}

fn click_air_transaction(slot: u32, held: NetworkItemDescriptor, eyes: Vector3<f32>) -> SInventoryTransaction {
    SInventoryTransaction {
        legacy_request_id: VarInt(0),
        legacy_set_item_slots: Vec::new(),
        has_value: false,
        actions: Vec::new(),
        transaction_type: VarUInt(2),
        transaction_data: TransactionData::UseItem(UseItemTransactionData {
            action_type: VarInt(USE_CLICK_AIR),
            trigger_type: 0,
            block_position: BlockPos::new(0, 0, 0),
            block_face: NO_FACE,
            hot_bar_slot: VarInt(slot as i32),
            hand: HandSlot::Mainhand,
            item_in_hand: held,
            player_position: eyes,
            click_position: Vector3::new(0.0, 0.0, 0.0),
            block_runtime_id: VarUInt(0),
            client_prediction: 0,
            client_cooldown_state: 0,
        }),
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

    fn eyes() -> Vector3<f32> {
        Vector3::new(1.5, 65.62, 2.5)
    }

    fn click() -> Click {
        Click {
            block: IVec3::new(1, 64, 2),
            face: 1,
            eyes: eyes(),
            at: Vector3::new(0.25, 1.0, 0.75),
            runtime_id: 77,
        }
    }

    fn use_item(transaction: &SInventoryTransaction) -> &UseItemTransactionData {
        let TransactionData::UseItem(data) = &transaction.transaction_data else {
            panic!("a use-item transaction");
        };
        data
    }

    #[test]
    fn a_click_that_places_takes_one_and_says_so() {
        let mut inventory = Inventory::default();
        inventory.set_slot(2, stack(5, 2));
        inventory.select(2);
        let transaction = inventory.click_block(&click(), true, true);
        assert_eq!(inventory.hotbar()[2].map(|item| item.count), Some(1));
        assert_eq!(transaction.legacy_request_id.0, -4, "as the capture's first is");
        assert_eq!(transaction.legacy_set_item_slots[0].container_id, CONTAINER_PREDICTED);
        assert_eq!(transaction.legacy_set_item_slots[0].slots, [2]);
        let action = &transaction.actions[0];
        assert_eq!((action.inventory_slot, action.old_item.stack_size, action.new_item.stack_size), (2, 2, 1));
        let data = use_item(&transaction);
        assert_eq!(data.item_in_hand.stack_size, 2, "what was in hand when it clicked");
        assert_eq!((data.client_prediction, data.hot_bar_slot.0, data.block_runtime_id.0), (1, 2, 77));

        // The click on air after it names what's in hand now.
        assert_eq!(use_item(&inventory.click_air(eyes())).item_in_hand.stack_size, 1);

        // The last one empties the hand, and goes without an id, as in the capture.
        let transaction = inventory.click_block(&click(), true, true);
        assert_eq!(inventory.hotbar()[2], None);
        assert_eq!(transaction.legacy_request_id.0, 0);
        assert!(transaction.legacy_set_item_slots.is_empty());
        assert_eq!(transaction.actions[0].new_item.id.0, 0);
    }

    #[test]
    fn a_click_that_doesnt_take_one_leaves_the_hand_alone() {
        let mut inventory = Inventory::default();
        inventory.set_slot(0, stack(5, 16));
        // In creative it places, and takes nothing.
        let transaction = inventory.click_block(&click(), true, false);
        assert_eq!(inventory.hotbar()[0].map(|item| item.count), Some(16));
        assert_eq!(transaction.legacy_request_id.0, 0);
        assert!(transaction.actions.is_empty() && transaction.legacy_set_item_slots.is_empty());
        assert_eq!(use_item(&transaction).client_prediction, 1);
        // An empty hand has nothing to take.
        let mut empty = Inventory::default();
        let transaction = empty.click_block(&click(), false, true);
        assert!(transaction.actions.is_empty());
        assert_eq!(use_item(&transaction).client_prediction, 0);
    }

    fn fixture_block(data: &UseItemTransactionData) -> IVec3 {
        let at = data.block_position.0;
        IVec3::new(at.x, at.y, at.z)
    }

    /// A placement, byte for byte, against what the official client sent, on top of a
    /// block in survival and on a side in creative.
    #[test]
    fn a_placement_matches_the_official_client() {
        let fixtures: [&[u8]; 2] = [
            include_bytes!(
                "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/inventory_transaction-place_on_top.bin"
            ),
            include_bytes!(
                "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/inventory_transaction-place_on_side.bin"
            ),
        ];
        for fixture in fixtures {
            let official = SInventoryTransaction::read(&mut &*fixture).unwrap();
            let data = use_item(&official);
            let click = Click {
                block: fixture_block(data),
                face: data.block_face,
                eyes: data.player_position,
                at: data.click_position,
                runtime_id: data.block_runtime_id.0,
            };
            let change = official
                .actions
                .first()
                .map(|action| (official.legacy_request_id.0, action.new_item.clone()));
            let ours = click_transaction(
                data.hot_bar_slot.0 as u32,
                data.item_in_hand.clone(),
                change,
                &click,
                data.client_prediction == 1,
            );
            let mut bytes = Vec::new();
            ours.write(&mut bytes).unwrap();
            assert_eq!(bytes, fixture);
        }
    }

    /// The click on air that follows, byte for byte.
    #[test]
    fn the_click_on_air_matches_the_official_client() {
        let fixture: &[u8] = include_bytes!(
            "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/inventory_transaction-click_air.bin"
        );
        let official = SInventoryTransaction::read(&mut &*fixture).unwrap();
        let data = use_item(&official);
        let ours = click_air_transaction(data.hot_bar_slot.0 as u32, data.item_in_hand.clone(), data.player_position);
        let mut bytes = Vec::new();
        ours.write(&mut bytes).unwrap();
        assert_eq!(bytes, fixture);
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
