//! Moving items: an item stack request for each click, shown at once, and confirmed or
//! undone by the server's answer (m5-plan Decision 10 and Step 12; worth teaching).
//!
//! **Predicted, then reconciled.** The client keeps two copies of the slots: what the
//! server last said, and what the player sees, which is the server's copy with every
//! request it hasn't answered yet applied on top ([`Requests::shown`]). That one rule
//! covers every case:
//! - a click shows at once, since its request is applied as soon as it's sent;
//! - an answer that agrees applies the request to the server's copy, with the counts
//!   and stack ids the server gives, so what's shown doesn't change;
//! - an answer that refuses just drops the request, so what's shown goes back to what
//!   the server has: the undo is free;
//! - the server resends the whole inventory after each request it agrees to. If a later
//!   request is still waiting, it stays on top, so it doesn't flicker back meanwhile.
//!
//! **Stack ids.** Each request says which stack it expects in each slot, by the id the
//! server gave it. A slot that a request still waiting on an answer changed has no id
//! yet, so it goes by that request's id instead. The creative take's new stack, which
//! has none either, goes by its own request's id (the capture's creative take).
//!
//! **What a click does** is decided from the slots alone, in [`click`], [`quick_move`] and
//! [`creative`], as the official client did in Step 12's capture.

use std::num::NonZero;

use pumpkin_protocol::{
    bedrock::{
        client::item_stack_response::ItemStackResponseInfo,
        network_item::{ContainerName, FullContainerName},
        server::item_stack_request::{
            ItemStackRequest, ItemStackRequestAction, ItemStackRequestSlotInfo, SItemStackRequest, StackRequestItem,
        },
    },
    codec::{var_int::VarInt, var_uint::VarUInt},
};

use super::slots::{SlotRef, Slots, Stack, capacity, fits, is_empty, net_id, same_item, with_count};

/// The id of a session's first request: the official client's, in both captures.
const FIRST_ID: i32 = -3;
/// Where the creative take's new stack is, until it's taken.
const CREATED_OUTPUT_SLOT: u8 = 50;
/// What the official client sends as the filter cause: there's no text to filter.
const NO_FILTER: i32 = -1;

/// Which button clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// A creative item, and what a request says about it.
#[derive(Clone, Debug)]
pub struct Creative {
    /// Its id in the creative list, from 1.
    pub id: u32,
    /// Its name with its namespace.
    pub identifier: &'static str,
    /// One of it, as a slot holds it.
    pub stack: Stack,
    /// The block it places, which the request names. Pumpkin's list says 0 for every
    /// item, and the official client works it out, so the client does too.
    pub block: u32,
    /// How many a take makes: a whole stack.
    pub count: u8,
}

/// One change a request makes.
#[derive(Clone, Debug)]
pub enum Change {
    /// Some of a stack to a slot that's empty or holds the same item. To the cursor it's
    /// a take, and anywhere else a place.
    Move { count: u8, from: SlotRef, to: SlotRef },
    /// Two slots change places. The capture puts the cursor first.
    Swap(SlotRef, SlotRef),
    /// Some of a stack is gone.
    Destroy { count: u8, from: SlotRef },
    /// The creative take: a new stack in a slot, which must be empty.
    Create { item: Creative, to: SlotRef },
}

impl Change {
    /// Changes `slots` as the move says. A move that no longer makes sense, because a
    /// request before it was refused, does nothing.
    fn apply(&self, slots: &mut Slots) {
        match self {
            Self::Move { count, from, to } => {
                let source = slots.get(*from).clone();
                let target = slots.get(*to);
                let count = u16::from(*count).min(source.stack_size);
                if is_empty(&source) || count == 0 || !(is_empty(target) || same_item(&source, target)) {
                    return;
                }
                let target = slots.get_mut(*to);
                if is_empty(target) {
                    // A new stack, whose id only the server can give.
                    *target = with_count(&source, count);
                    target.net_id = None;
                } else {
                    target.stack_size += count;
                }
                *slots.get_mut(*from) = with_count(&source, source.stack_size - count);
            }
            Self::Swap(a, b) => {
                let first = slots.get(*a).clone();
                *slots.get_mut(*a) = std::mem::replace(slots.get_mut(*b), first);
            }
            Self::Destroy { count, from } => {
                let source = slots.get(*from).clone();
                let left = source.stack_size.saturating_sub(u16::from(*count));
                *slots.get_mut(*from) = with_count(&source, left);
            }
            Self::Create { item, to } => {
                if is_empty(slots.get(*to)) {
                    *slots.get_mut(*to) = with_count(&item.stack, u16::from(item.count));
                }
            }
        }
    }

    /// Whether the move changes `slot`.
    fn touches(&self, slot: SlotRef) -> bool {
        match self {
            Self::Move { from, to, .. } => *from == slot || *to == slot,
            Self::Swap(a, b) => *a == slot || *b == slot,
            Self::Destroy { from, .. } => *from == slot,
            Self::Create { to, .. } => *to == slot,
        }
    }
}

/// A request the server hasn't answered.
#[derive(Debug)]
struct Pending {
    id: i32,
    moves: Vec<Change>,
}

/// The requests waiting on an answer, and the next one's id.
#[derive(Debug)]
pub struct Requests {
    next: i32,
    pending: Vec<Pending>,
}

impl Default for Requests {
    fn default() -> Self {
        Self {
            next: FIRST_ID,
            pending: Vec::new(),
        }
    }
}

impl Requests {
    /// As a session whose next request is `id`, to check one from partway through the
    /// capture.
    #[cfg(test)]
    const fn starting_at(id: i32) -> Self {
        Self {
            next: id,
            pending: Vec::new(),
        }
    }

    /// Sends `moves` as one request, and returns its packet. `shown` is what the player
    /// sees now, which names the stacks it expects.
    pub fn send(&mut self, moves: Vec<Change>, shown: &Slots) -> SItemStackRequest {
        let id = self.next;
        // Ids fall by 2, as in the capture.
        self.next -= 2;
        let request = self.encode(id, &moves, shown);
        self.pending.push(Pending { id, moves });
        SItemStackRequest { requests: vec![request] }
    }

    /// How many requests are waiting on an answer.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.pending.len()
    }

    /// The server's answer to a request: one it agreed to is applied to `server`, with
    /// the counts and stack ids it gives, and one it refused is dropped. Says which it
    /// was, or `None` for a request this client didn't send.
    pub fn answer(&mut self, response: &ItemStackResponseInfo, server: &mut Slots) -> Option<bool> {
        let at = self.pending.iter().position(|pending| pending.id == response.client_request_id.0)?;
        let pending = self.pending.remove(at);
        if response.result != 0 {
            return Some(false);
        }
        for change in &pending.moves {
            change.apply(server);
        }
        for container in &response.containers {
            for slot in &container.slots {
                let Some(at) = SlotRef::from_container(container.full_container_name.container_name, slot.slot) else {
                    continue;
                };
                let stack = server.get_mut(at);
                if slot.amount == 0 {
                    *stack = Stack::default();
                } else {
                    stack.stack_size = u16::from(slot.amount);
                    stack.net_id = NonZero::new(slot.item_stack_net_id.0);
                }
            }
        }
        Some(true)
    }

    /// What the player sees: the server's slots, with every request still waiting
    /// applied on top, in the order they went.
    #[must_use]
    pub fn shown(&self, server: &Slots) -> Slots {
        let mut shown = server.clone();
        for pending in &self.pending {
            for change in &pending.moves {
                change.apply(&mut shown);
            }
        }
        shown
    }

    /// The stack a request expects in `slot`: the last waiting request that changed it,
    /// by its id, or else the stack's own id from the server.
    fn stack_id(&self, slot: SlotRef, shown: &Slots) -> i32 {
        let changed = self.pending.iter().rev().find(|pending| pending.moves.iter().any(|change| change.touches(slot)));
        changed.map_or_else(|| net_id(shown.get(slot)), |pending| pending.id)
    }

    fn info(&self, slot: SlotRef, shown: &Slots) -> ItemStackRequestSlotInfo {
        let (container_name, slot_id) = slot.container();
        slot_info(container_name, slot_id, self.stack_id(slot, shown))
    }

    /// The request, action by action, as the official client writes it.
    fn encode(&self, id: i32, moves: &[Change], shown: &Slots) -> ItemStackRequest {
        let mut actions = Vec::with_capacity(moves.len());
        for change in moves {
            match change {
                Change::Move { count, from, to } => {
                    let (count, source, destination) = (*count, self.info(*from, shown), self.info(*to, shown));
                    actions.push(if *to == SlotRef::Cursor {
                        ItemStackRequestAction::Take { count, source, destination }
                    } else {
                        ItemStackRequestAction::Place { count, source, destination }
                    });
                }
                Change::Swap(a, b) => actions.push(ItemStackRequestAction::Swap {
                    slot1: self.info(*a, shown),
                    slot2: self.info(*b, shown),
                }),
                Change::Destroy { count, from } => actions.push(ItemStackRequestAction::Destroy {
                    count: *count,
                    source: self.info(*from, shown),
                }),
                Change::Create { item, to } => {
                    actions.push(ItemStackRequestAction::CraftCreative {
                        creative_item_id: VarUInt(item.id),
                        repetitions: 1,
                    });
                    // What the craft makes, which the official client still sends after it.
                    actions.push(ItemStackRequestAction::CraftResultsDeprecated {
                        result_items: vec![StackRequestItem {
                            identifier: Some(item.identifier.to_string()),
                            metadata_value: VarInt(item.stack.aux_value.0 as i32),
                            count: 1,
                            block_runtime_id: VarUInt(item.block),
                            extra_data: item.stack.extra_data.clone(),
                        }],
                        times_crafted: 1,
                    });
                    actions.push(ItemStackRequestAction::Take {
                        count: item.count,
                        source: slot_info(ContainerName::CreatedOutput, CREATED_OUTPUT_SLOT, id),
                        destination: self.info(*to, shown),
                    });
                }
            }
        }
        ItemStackRequest {
            request_id: VarInt(id),
            actions,
            filter_strings: Vec::new(),
            filter_cause: NO_FILTER,
        }
    }
}

const fn slot_info(container_name: ContainerName, slot_id: u8, stack_id: i32) -> ItemStackRequestSlotInfo {
    ItemStackRequestSlotInfo {
        container_name: FullContainerName {
            container_name,
            dynamic_id: None,
        },
        slot_id,
        stack_id,
    }
}

/// A count as a request carries it: a byte. No stack is bigger.
fn byte(count: u16) -> u8 {
    count.min(u16::from(u8::MAX)) as u8
}

/// What a click on `slot` does. With nothing on the cursor, it picks the stack up, all
/// of it or, with the right button, half, rounded up. With something, it puts it down:
/// all of it, or one with the right button, as far as there's room; and on another item,
/// the two change places. Armour only goes where it's worn.
#[must_use]
pub fn click(slots: &Slots, slot: SlotRef, button: Button) -> Vec<Change> {
    let (cursor, target) = (&slots.cursor, slots.get(slot));
    if is_empty(cursor) {
        if is_empty(target) {
            return Vec::new();
        }
        let count = match button {
            Button::Left => target.stack_size,
            Button::Right => target.stack_size.div_ceil(2),
        };
        return vec![Change::Move {
            count: byte(count),
            from: slot,
            to: SlotRef::Cursor,
        }];
    }
    if !fits(cursor, slot) {
        return Vec::new();
    }
    if is_empty(target) || same_item(cursor, target) {
        let have = if is_empty(target) { 0 } else { target.stack_size };
        let room = capacity(cursor, slot).saturating_sub(have);
        let wanted = match button {
            Button::Left => cursor.stack_size,
            Button::Right => 1,
        };
        let count = wanted.min(room);
        if count == 0 {
            return Vec::new();
        }
        return vec![Change::Move {
            count: byte(count),
            from: SlotRef::Cursor,
            to: slot,
        }];
    }
    vec![Change::Swap(SlotRef::Cursor, slot)]
}

/// What a shift-click on `slot` does: the stack goes across, onto stacks of the same item
/// first and then into the first empty slot, as Java does. From the hotbar it goes to
/// the rows above, from those to the hotbar, and from the armour or the off hand to
/// either. The capture's shift-click is one place into the first empty slot.
#[must_use]
pub fn quick_move(slots: &Slots, slot: SlotRef) -> Vec<Change> {
    let targets: Vec<SlotRef> = match slot {
        SlotRef::Main(_) if slot.on_hotbar() => (9..36).map(SlotRef::Main).collect(),
        SlotRef::Main(_) => (0..9).map(SlotRef::Main).collect(),
        SlotRef::Armour(_) | SlotRef::Offhand => (9..36).chain(0..9).map(SlotRef::Main).collect(),
        SlotRef::Cursor => return Vec::new(),
    };
    let mut scratch = slots.clone();
    let mut moves = Vec::new();
    for onto_same in [true, false] {
        for &to in &targets {
            let source = scratch.get(slot);
            if is_empty(source) {
                return moves;
            }
            let there = scratch.get(to);
            let count = if onto_same && !is_empty(there) && same_item(source, there) {
                capacity(there, to).saturating_sub(there.stack_size).min(source.stack_size)
            } else if !onto_same && is_empty(there) {
                source.stack_size
            } else {
                0
            };
            if count > 0 {
                let change = Change::Move {
                    count: byte(count),
                    from: slot,
                    to,
                };
                change.apply(&mut scratch);
                moves.push(change);
            }
        }
    }
    moves
}

/// What a click on the creative list does: with something on the cursor, that's thrown
/// away, wherever the click was; with nothing, a whole stack of `item` comes to it, if the
/// click was on one (the capture's creative take, and its stack dropped on the list).
#[must_use]
pub fn creative(slots: &Slots, item: Option<Creative>) -> Vec<Change> {
    if !is_empty(&slots.cursor) {
        return vec![Change::Destroy {
            count: byte(slots.cursor.stack_size),
            from: SlotRef::Cursor,
        }];
    }
    item.map(|item| vec![Change::Create { item, to: SlotRef::Cursor }]).unwrap_or_default()
}

/// What a shift-click on the hotbar does in creative: the stack is thrown away (the
/// capture).
#[must_use]
pub fn creative_quick_move(slots: &Slots, slot: SlotRef) -> Vec<Change> {
    let stack = slots.get(slot);
    if is_empty(stack) {
        return Vec::new();
    }
    vec![Change::Destroy {
        count: byte(stack.stack_size),
        from: slot,
    }]
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::{
        bedrock::client::item_stack_response::{ItemStackResponseContainerInfo, ItemStackResponseSlotInfo},
        serial::PacketWrite,
    };

    use super::*;
    use crate::inventory::{items, slots::max_stack};

    const DIRT: i16 = 3;
    const PLANKS: i16 = 5;
    const TORCH: i16 = 50;
    const STONE: i16 = 1;
    const IRON_HELMET: i16 = 346;

    /// A fixture of the Step 12 capture, from the fork.
    macro_rules! fixture {
        ($name:literal) => {
            &include_bytes!(concat!(
                "../../../pumpkin/crates/pumpkin-protocol/src/bedrock/client_codecs/fixtures/",
                $name
            ))[..]
        };
    }

    fn stack(id: i16, count: u16, net_id: i32) -> Stack {
        Stack {
            id,
            stack_size: count,
            net_id: NonZero::new(net_id),
            ..Stack::default()
        }
    }

    fn bytes(packet: &SItemStackRequest) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.write(&mut bytes).unwrap();
        bytes
    }

    /// The server agreed, and these slots now hold this much, with these ids.
    fn agreed(id: i32, slots: &[(SlotRef, u8, i32)]) -> ItemStackResponseInfo {
        let containers = slots
            .iter()
            .map(|&(slot, amount, net_id)| {
                let (container_name, index) = slot.container();
                ItemStackResponseContainerInfo {
                    full_container_name: FullContainerName {
                        container_name,
                        dynamic_id: None,
                    },
                    slots: vec![ItemStackResponseSlotInfo {
                        requested_slot: index,
                        slot: index,
                        amount,
                        item_stack_net_id: VarInt(net_id),
                        custom_name: String::new(),
                        filtered_custom_name: String::new(),
                        durability_correction: VarInt(0),
                    }],
                }
            })
            .collect();
        ItemStackResponseInfo {
            result: 0,
            client_request_id: VarInt(id),
            containers,
        }
    }

    fn refused(id: i32) -> ItemStackResponseInfo {
        ItemStackResponseInfo {
            result: 1,
            client_request_id: VarInt(id),
            containers: Vec::new(),
        }
    }

    /// The server side of a session: its slots, and the requests waiting on it.
    struct Session {
        server: Slots,
        requests: Requests,
    }

    impl Session {
        fn new(server: Slots) -> Self {
            Self {
                server,
                requests: Requests::default(),
            }
        }

        fn shown(&self) -> Slots {
            self.requests.shown(&self.server)
        }

        fn send(&mut self, moves: Vec<Change>) -> Vec<u8> {
            let shown = self.shown();
            bytes(&self.requests.send(moves, &shown))
        }

        fn answer(&mut self, response: &ItemStackResponseInfo) -> Option<bool> {
            self.requests.answer(response, &mut self.server)
        }
    }

    /// The capture's survival inventory, after the three gives.
    fn given() -> Slots {
        let mut slots = Slots::default();
        slots.main[0] = stack(DIRT, 16, 8);
        slots.main[1] = stack(PLANKS, 16, 10);
        slots.main[2] = stack(TORCH, 4, 12);
        slots
    }

    /// Step 12's survival moves, one after another, each answered as the capture shows,
    /// and each request byte for byte the official client's.
    #[test]
    fn the_survival_moves_match_the_official_client() {
        let mut session = Session::new(given());

        // The dirt, picked up whole.
        let take = click(&session.shown(), SlotRef::Main(0), Button::Left);
        assert_eq!(session.send(take), fixture!("item_stack_request-take.bin"));
        assert_eq!(session.shown().item(SlotRef::Cursor).map(|item| item.count), Some(16), "shown at once");
        let answer = agreed(-3, &[(SlotRef::Main(0), 0, 0), (SlotRef::Cursor, 16, 15)]);
        assert_eq!(session.answer(&answer), Some(true));

        // Put down in the bottom row's first slot.
        let place = click(&session.shown(), SlotRef::Main(27), Button::Left);
        assert_eq!(session.send(place), fixture!("item_stack_request-place.bin"));
        session.answer(&agreed(-5, &[(SlotRef::Cursor, 0, 0), (SlotRef::Main(27), 16, 16)]));

        // Half the planks, with the right button, put down beside it.
        let half = click(&session.shown(), SlotRef::Main(1), Button::Right);
        assert_eq!(session.send(half), fixture!("item_stack_request-take_half.bin"));
        session.answer(&agreed(-7, &[(SlotRef::Main(1), 8, 10), (SlotRef::Cursor, 8, 17)]));
        let place = click(&session.shown(), SlotRef::Main(28), Button::Left);
        session.send(place);
        session.answer(&agreed(-9, &[(SlotRef::Cursor, 0, 0), (SlotRef::Main(28), 8, 18)]));

        // The torches, then put on the dirt: the two change places.
        let take = click(&session.shown(), SlotRef::Main(2), Button::Left);
        session.send(take);
        session.answer(&agreed(-11, &[(SlotRef::Main(2), 0, 0), (SlotRef::Cursor, 4, 19)]));
        let swap = click(&session.shown(), SlotRef::Main(27), Button::Left);
        assert!(matches!(swap[..], [Change::Swap(SlotRef::Cursor, SlotRef::Main(27))]));
        assert_eq!(session.send(swap), fixture!("item_stack_request-swap.bin"));
        session.answer(&agreed(-13, &[(SlotRef::Cursor, 16, 16), (SlotRef::Main(27), 4, 21)]));
        assert_eq!(session.shown().item(SlotRef::Main(27)).map(|item| item.id), Some(TORCH));

        // The dirt back on the hotbar, then shift-clicked up into the first empty slot.
        let place = click(&session.shown(), SlotRef::Main(0), Button::Left);
        session.send(place);
        session.answer(&agreed(-15, &[(SlotRef::Cursor, 0, 0), (SlotRef::Main(0), 16, 20)]));
        let shift = quick_move(&session.shown(), SlotRef::Main(0));
        assert_eq!(session.send(shift), fixture!("item_stack_request-shift_click.bin"));
        session.answer(&agreed(-17, &[(SlotRef::Main(0), 0, 0), (SlotRef::Main(9), 16, 22)]));

        let shown = session.shown();
        assert_eq!(shown.item(SlotRef::Main(9)).map(|item| (item.id, item.count)), Some((DIRT, 16)));
        assert_eq!(shown.item(SlotRef::Main(28)).map(|item| (item.id, item.count)), Some((PLANKS, 8)));
        assert!(shown.item(SlotRef::Cursor).is_none());
        assert_eq!(session.requests.waiting(), 0);
    }

    /// Stone from the creative list, a stack thrown back on it, and a shift-click on the
    /// hotbar, byte for byte.
    #[test]
    fn the_creative_moves_match_the_official_client() {
        let mut stone = stack(STONE, 1, 0);
        // An item without NBT, and nothing it can be placed on or break.
        stone.extra_data = vec![0; 10];
        let item = Creative {
            id: 604,
            identifier: items::registry_key(STONE).unwrap(),
            stack: stone.clone(),
            block: items::block_of(STONE, 0).unwrap(),
            count: max_stack(&stone),
        };
        assert_eq!(item.identifier, "minecraft:stone");
        assert_eq!(item.block, 0x8031_0e21, "the hashed id the official client sent");
        assert_eq!(item.count, 64);

        // The capture's creative take was request -19.
        let mut session = Session::new(Slots::default());
        session.requests = Requests::starting_at(-19);
        let take = creative(&session.shown(), Some(item.clone()));
        assert_eq!(session.send(take), fixture!("item_stack_request-creative_take.bin"));
        let cursor = session.shown().item(SlotRef::Cursor);
        assert_eq!(cursor.map(|item| (item.id, item.count)), Some((STONE, 64)));

        // A stack on the cursor, dropped on the list: request -27, stack 25.
        let mut session = Session::new(Slots::default());
        session.requests = Requests::starting_at(-27);
        session.server.cursor = stack(STONE, 64, 25);
        let destroy = creative(&session.shown(), None);
        assert_eq!(session.send(destroy), fixture!("item_stack_request-destroy.bin"));
        assert!(session.shown().item(SlotRef::Cursor).is_none());

        // A shift-click on the planks on the hotbar: request -23.
        let mut session = Session::new(Slots::default());
        session.requests = Requests::starting_at(-23);
        session.server.main[1] = stack(PLANKS, 8, 10);
        let destroy = creative_quick_move(&session.shown(), SlotRef::Main(1));
        assert_eq!(session.send(destroy), fixture!("item_stack_request-creative_shift_click.bin"));
    }

    #[test]
    fn a_refused_request_is_undone_and_what_came_after_it_does_nothing() {
        let mut session = Session::new(given());
        let take = click(&session.shown(), SlotRef::Main(0), Button::Left);
        session.send(take);
        // Sent before the answer came.
        let place = click(&session.shown(), SlotRef::Main(20), Button::Left);
        session.send(place);
        assert_eq!(session.shown().item(SlotRef::Main(20)).map(|item| item.count), Some(16));

        assert_eq!(session.answer(&refused(-3)), Some(false));
        let shown = session.shown();
        assert_eq!(shown.item(SlotRef::Main(0)).map(|item| item.count), Some(16), "back where it was");
        assert!(shown.item(SlotRef::Main(20)).is_none(), "the place has nothing to put down");
        assert!(shown.item(SlotRef::Cursor).is_none());
        assert_eq!(session.answer(&refused(-99)), None, "not one of ours");
    }

    #[test]
    fn a_slot_a_waiting_request_changed_goes_by_that_requests_id() {
        let mut session = Session::new(given());
        let take = click(&session.shown(), SlotRef::Main(0), Button::Left);
        session.send(take);
        let place = click(&session.shown(), SlotRef::Main(20), Button::Left);
        let shown = session.shown();
        let packet = session.requests.send(place, &shown);
        let ItemStackRequestAction::Place { source, destination, .. } = &packet.requests[0].actions[0] else {
            panic!("expected a place");
        };
        assert_eq!(source.stack_id, -3, "the take's id: the cursor's stack has none yet");
        assert_eq!(destination.stack_id, 0, "an empty slot");
    }

    #[test]
    fn a_waiting_request_stays_shown_when_the_server_sends_its_copy() {
        let mut session = Session::new(given());
        let take = click(&session.shown(), SlotRef::Main(0), Button::Left);
        session.send(take);
        // The server sends the 36 as they were before it got to the take.
        session.server.main = given().main;
        assert!(session.shown().item(SlotRef::Main(0)).is_none());
        assert_eq!(session.shown().item(SlotRef::Cursor).map(|item| item.count), Some(16));
    }

    #[test]
    fn putting_down_fills_what_room_there_is_and_the_right_button_puts_down_one() {
        let mut slots = given();
        slots.cursor = stack(DIRT, 60, 30);
        slots.main[5] = stack(DIRT, 10, 31);
        assert!(matches!(
            click(&slots, SlotRef::Main(5), Button::Left)[..],
            [Change::Move { count: 54, from: SlotRef::Cursor, to: SlotRef::Main(5) }]
        ));
        assert!(matches!(click(&slots, SlotRef::Main(5), Button::Right)[..], [Change::Move { count: 1, .. }]));
        // A full stack of the same takes nothing, and swaps nothing.
        slots.main[5] = stack(DIRT, 64, 31);
        assert!(click(&slots, SlotRef::Main(5), Button::Left).is_empty());
        // Half of an odd stack is rounded up.
        let mut slots = given();
        slots.main[4] = stack(DIRT, 5, 32);
        assert!(matches!(click(&slots, SlotRef::Main(4), Button::Right)[..], [Change::Move { count: 3, .. }]));
    }

    #[test]
    fn armour_only_goes_where_its_worn() {
        let mut slots = Slots {
            cursor: stack(IRON_HELMET, 1, 40),
            ..Slots::default()
        };
        assert!(click(&slots, SlotRef::Armour(1), Button::Left).is_empty(), "not on the chest");
        assert!(matches!(
            click(&slots, SlotRef::Armour(0), Button::Left)[..],
            [Change::Move { count: 1, to: SlotRef::Armour(0), .. }]
        ));
        slots.cursor = stack(DIRT, 16, 41);
        assert!(click(&slots, SlotRef::Armour(0), Button::Left).is_empty(), "dirt isn't worn");
        assert!(matches!(click(&slots, SlotRef::Offhand, Button::Left)[..], [Change::Move { count: 16, .. }]));
    }

    #[test]
    fn a_shift_click_fills_stacks_of_the_same_first() {
        let mut slots = given();
        slots.main[12] = stack(DIRT, 60, 50);
        assert!(matches!(
            quick_move(&slots, SlotRef::Main(0))[..],
            [Change::Move { count: 4, to: SlotRef::Main(12), .. }, Change::Move { count: 12, to: SlotRef::Main(9), .. }]
        ));
        // And from the rows above down to the hotbar: onto its dirt, then the first empty slot.
        assert!(matches!(
            quick_move(&slots, SlotRef::Main(12))[..],
            [Change::Move { count: 48, to: SlotRef::Main(0), .. }, Change::Move { count: 12, to: SlotRef::Main(3), .. }]
        ));
    }
}
