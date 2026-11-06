//! What the player carries, as the server says (m5-plan Decision 10 and Step 12).
//!
//! **A mirror.** The server owns the inventory, and tells the client with
//! `InventoryContent` and `InventorySlot`, and which slot is selected with
//! `PlayerHotbar`. The client keeps a copy, the stacks exactly as they came, because it
//! has to hand them back: `MobEquipment`, a drop and every item stack request name the
//! stacks they're about.
//!
//! The first nine of the player's 36 slots are the hotbar. Since the fork's move to
//! 1.26.60, Pumpkin sends the armour and the off hand as well, in containers of their own
//! (`slots.rs`).
//!
//! **Two copies.** What the server last said, and what the player sees: the server's,
//! with the inventory screen's requests that are still waiting on an answer applied on
//! top (`requests.rs`). The hotbar's own changes, Q and placing, go by transactions the
//! server answers with the slot itself, so they change the server's copy straight away,
//! as they always did.

pub mod creative;
pub mod items;
pub mod requests;
pub mod slots;

use glam::IVec3;
use pumpkin_protocol::{
    bedrock::{
        client::item_stack_response::CItemStackResponse,
        network_item::{ContainerName, FullContainerName, NetworkItemDescriptor, NetworkItemStackDescriptor},
        server::{
            inventory_transaction::{
                HandSlot, InventoryAction, LegacySetItemSlot, NormalTransactionData, SInventoryTransaction,
                TransactionData, UseItemTransactionData, WINDOW_ID_INVENTORY,
            },
            item_stack_request::SItemStackRequest,
        },
    },
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::{PacketRead, PacketWrite},
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use tracing::info;

pub use self::slots::CONTAINER_MAIN;
use self::{
    requests::{Change, Requests},
    slots::{ARMOUR, CONTAINER_ARMOUR, CONTAINER_CURSOR, CONTAINER_OFFHAND, Slots, Stack},
};

/// The slots of the player's inventory.
pub const MAIN: usize = 36;
/// The first of them, which the hotbar shows.
pub const HOTBAR: usize = 9;
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
pub(crate) fn item_of(stack: &NetworkItemStackDescriptor) -> Option<Item> {
    (stack.id != 0 && stack.stack_size > 0).then_some(Item {
        id: stack.id,
        aux: stack.aux_value.0,
        count: stack.stack_size,
    })
}

/// An item as a slot holds it. The two forms are the same on the wire, except that a
/// slot's keeps the item's NBT as bytes.
pub(crate) fn stack_of(item: &NetworkItemDescriptor) -> Option<Stack> {
    let mut bytes = Vec::new();
    item.write(&mut bytes).ok()?;
    Stack::read(&mut bytes.as_slice()).ok()
}

/// The player's inventory.
#[derive(Debug, Default)]
pub struct Inventory {
    /// What the server last said.
    server: Slots,
    /// What the player sees: the server's, with the requests still waiting on top.
    shown: Slots,
    requests: Requests,
    selected: u8,
    /// The id of the last transaction that predicted a change: they fall by 2.
    last_transaction: i32,
    /// Whether the server has opened the inventory screen.
    open: bool,
    /// Requests sent, and how many of them the server refused.
    sent: usize,
    refused: usize,
}

impl Inventory {
    /// Takes the whole of the 36. A short list leaves the rest empty.
    pub fn set_main(&mut self, slots: Vec<NetworkItemStackDescriptor>) {
        self.server.main = slots;
        self.server.main.resize(MAIN, Stack::default());
        self.refresh();
    }

    /// Takes one of the 36. A slot past the end is ignored.
    pub fn set_slot(&mut self, slot: usize, stack: NetworkItemStackDescriptor) {
        if let Some(place) = self.server.main.get_mut(slot) {
            *place = stack;
            self.refresh();
        }
    }

    /// Takes a container's contents. Says whether it's one of the player's.
    pub fn set_content(&mut self, container: u32, name: &FullContainerName, slots: Vec<Stack>) -> bool {
        let mut slots = slots.into_iter();
        match container {
            CONTAINER_MAIN => {
                self.set_main(slots.collect());
                return true;
            }
            CONTAINER_ARMOUR => {
                for place in &mut self.server.armour {
                    *place = slots.next().unwrap_or_default();
                }
            }
            CONTAINER_OFFHAND => self.server.offhand = slots.next().unwrap_or_default(),
            _ if container == CONTAINER_CURSOR || name.container_name == ContainerName::Cursor => {
                self.server.cursor = slots.next().unwrap_or_default();
            }
            _ => return false,
        }
        self.refresh();
        true
    }

    /// Takes one slot of a container. Says whether it's one of the player's.
    pub fn set_container_slot(&mut self, container: u32, slot: usize, stack: Stack) -> bool {
        match container {
            CONTAINER_MAIN => {
                self.set_slot(slot, stack);
                return true;
            }
            CONTAINER_ARMOUR if slot < ARMOUR => self.server.armour[slot] = stack,
            CONTAINER_OFFHAND if slot == 0 => self.server.offhand = stack,
            _ => return false,
        }
        self.refresh();
        true
    }

    /// Works out what's shown again, after a change to the server's copy or the requests.
    fn refresh(&mut self) {
        self.shown = self.requests.shown(&self.server);
    }

    /// Everything the player sees.
    #[must_use]
    pub const fn slots(&self) -> &Slots {
        &self.shown
    }

    /// Whether the inventory screen is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// The server opened the inventory screen, or the screen closed.
    pub const fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Sends `moves` as one item stack request, and shows them at once. Nothing to move
    /// sends nothing.
    pub fn request(&mut self, moves: Vec<Change>) -> Option<SItemStackRequest> {
        if moves.is_empty() {
            return None;
        }
        let packet = self.requests.send(moves, &self.shown);
        self.sent += 1;
        self.refresh();
        Some(packet)
    }

    /// Takes the server's answers to requests. Says how many it refused.
    pub fn answer(&mut self, response: &CItemStackResponse) -> usize {
        let mut refused = 0;
        for info in &response.responses {
            if self.requests.answer(info, &mut self.server) == Some(false) {
                info!("The server refused item stack request {}, so it's undone", info.client_request_id.0);
                refused += 1;
            }
        }
        self.refused += refused;
        self.refresh();
        refused
    }

    /// How many requests are waiting on an answer.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.requests.waiting()
    }

    /// How many requests have been sent.
    #[must_use]
    pub const fn sent(&self) -> usize {
        self.sent
    }

    /// How many requests the server has refused.
    #[must_use]
    pub const fn refused(&self) -> usize {
        self.refused
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
        &self.shown.main[usize::from(self.selected)]
    }

    /// What the hotbar shows.
    #[must_use]
    pub fn hotbar(&self) -> [Option<Item>; HOTBAR] {
        std::array::from_fn(|slot| item_of(&self.shown.main[slot]))
    }

    /// Takes one of the held item out of the hand, and returns the transaction that
    /// tells the server it was dropped. `None` if the hand is empty.
    pub fn drop_one(&mut self) -> Option<SInventoryTransaction> {
        let slot = usize::from(self.selected);
        item_of(&self.server.main[slot])?;
        let old = action_item(&self.server.main[slot])?;
        let stack = &mut self.server.main[slot];
        stack.stack_size -= 1;
        if stack.stack_size == 0 {
            *stack = NetworkItemStackDescriptor::default();
        }
        self.refresh();
        self.last_transaction -= 2;
        Some(drop_transaction(self.last_transaction, slot as u32, old))
    }

    /// A right click on a block. If it `takes_one`, the hand loses one, and the
    /// transaction says so. `predicted` says the client expects it to place something.
    pub fn click_block(&mut self, click: &Click, predicted: bool, takes_one: bool) -> SInventoryTransaction {
        let slot = usize::from(self.selected);
        let held = action_item(&self.server.main[slot]).unwrap_or_default();
        let change = (takes_one && item_of(&self.server.main[slot]).is_some()).then(|| {
            // Each click takes two ids in the capture, whichever transaction the second is for.
            self.last_transaction -= 4;
            let stack = &mut self.server.main[slot];
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
        self.refresh();
        click_transaction(slot as u32, held, change, click, predicted)
    }

    /// The click on air that follows a click on a block, naming what's in hand by then,
    /// and the one that's all there is when nothing is aimed at.
    #[must_use]
    pub fn click_air(&self, eyes: Vector3<f32>) -> SInventoryTransaction {
        let slot = usize::from(self.selected);
        click_air_transaction(slot as u32, action_item(&self.shown.main[slot]).unwrap_or_default(), eyes)
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
        assert_eq!(inventory.server.main.len(), MAIN, "a slot past the end is ignored");
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
