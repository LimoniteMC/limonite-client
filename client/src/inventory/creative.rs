//! The creative list, as the server sends it at join (m5-plan Decision 10 and Step 12).
//!
//! **Groups** (worth teaching). `CreativeContent` lists every item once, in the order
//! the screen shows them, each in a group, and each group in a category, which is a tab
//! of the screen. A group with a name, such as all the planks, shows as one cell that
//! opens to show its items after it; a group without one is just its items. So what a
//! tab shows depends on which groups are open, and [`CreativeList::cells`] works that out
//! afresh each time one opens or closes.

use std::collections::HashSet;

use pumpkin_protocol::bedrock::{
    client::creative_content::CreativeCategory, client_codecs::CreativeContent, network_item::NetworkItemDescriptor,
};

use super::{
    items,
    requests::Creative,
    slots::{Stack, max_stack},
    stack_of,
};

/// A tab of the creative screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Construction,
    Equipment,
    Items,
    Nature,
}

impl Tab {
    /// In the order the screen shows them, left to right.
    pub const ALL: [Self; 4] = [Self::Construction, Self::Equipment, Self::Items, Self::Nature];

    /// Its number in `SetPlayerInventoryOptions`, which the official client sends when it
    /// changes tab: the order they're shown in, from 1.
    #[must_use]
    pub const fn options_index(self) -> i32 {
        match self {
            Self::Construction => 1,
            Self::Equipment => 2,
            Self::Items => 3,
            Self::Nature => 4,
        }
    }

    /// The tab a category is shown in. Items only commands give aren't shown.
    const fn of(category: CreativeCategory) -> Option<Self> {
        match category {
            CreativeCategory::Construction => Some(Self::Construction),
            CreativeCategory::Equipment => Some(Self::Equipment),
            CreativeCategory::Items => Some(Self::Items),
            CreativeCategory::Nature => Some(Self::Nature),
            _ => None,
        }
    }
}

/// An item of the list.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Its id in the list, from 1, which a creative take names.
    pub id: u32,
    /// One of it.
    pub stack: Stack,
    /// Its group.
    pub group: usize,
}

/// A group of the list.
#[derive(Clone, Debug)]
pub struct Group {
    pub tab: Option<Tab>,
    /// Empty for items shown on their own.
    pub name: String,
    /// The item it shows as, if the server gave one.
    pub icon: Option<(i16, u32)>,
}

/// One cell of a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// An item: on its own, or one of an open group's.
    Item { entry: usize, grouped: bool },
    /// A group with a name, open or shut.
    Group { group: usize, open: bool },
}

/// The creative list.
#[derive(Clone, Debug, Default)]
pub struct CreativeList {
    groups: Vec<Group>,
    entries: Vec<Entry>,
}

impl CreativeList {
    /// Takes what the server sent. An entry whose item doesn't decode, or whose group
    /// doesn't exist, is left out.
    #[must_use]
    pub fn new(content: &CreativeContent) -> Self {
        let groups = content
            .groups
            .iter()
            .map(|group| Group {
                tab: Tab::of(group.creative_category),
                name: group.name.clone(),
                icon: icon(&group.group_icon_item),
            })
            .collect::<Vec<_>>();
        let entries = content
            .entries
            .iter()
            .filter(|entry| (entry.group_index.0 as usize) < groups.len())
            .filter_map(|entry| {
                Some(Entry {
                    id: entry.id.0,
                    stack: stack_of(&entry.item)?,
                    group: entry.group_index.0 as usize,
                })
            })
            .collect();
        Self { groups, entries }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn entry(&self, index: usize) -> &Entry {
        &self.entries[index]
    }

    /// What a group shows as: its own icon, or else its first item.
    #[must_use]
    pub fn group_icon(&self, group: usize) -> Option<(i16, u32)> {
        self.groups[group].icon.or_else(|| {
            let first = self.entries.iter().find(|entry| entry.group == group)?;
            Some((first.stack.id, first.stack.aux_value.0))
        })
    }

    /// The cells of `tab`, in order, with the groups in `open` showing their items.
    #[must_use]
    pub fn cells(&self, tab: Tab, open: &HashSet<usize>) -> Vec<Cell> {
        let mut cells = Vec::new();
        let mut headed = HashSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let group = &self.groups[entry.group];
            if group.tab != Some(tab) {
                continue;
            }
            if group.name.is_empty() {
                cells.push(Cell::Item {
                    entry: index,
                    grouped: false,
                });
                continue;
            }
            let is_open = open.contains(&entry.group);
            if headed.insert(entry.group) {
                cells.push(Cell::Group {
                    group: entry.group,
                    open: is_open,
                });
            }
            if is_open {
                cells.push(Cell::Item {
                    entry: index,
                    grouped: true,
                });
            }
        }
        cells
    }

    /// What a take of entry `index` sends.
    #[must_use]
    pub fn take(&self, index: usize) -> Option<Creative> {
        let entry = self.entries.get(index)?;
        let stack = &entry.stack;
        let identifier = items::registry_key(stack.id)?;
        Some(Creative {
            id: entry.id,
            identifier,
            stack: stack.clone(),
            block: items::block_of(stack.id, stack.aux_value.0).unwrap_or(0),
            count: max_stack(stack),
        })
    }
}

/// An icon's item, unless it's none.
fn icon(item: &NetworkItemDescriptor) -> Option<(i16, u32)> {
    (item.id.0 != 0).then_some((item.id.0 as i16, item.aux_value.0))
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::{
        bedrock::client::creative_content::{CreativeGroupInfoPayload, CreativeItemEntryPayload},
        codec::{var_int::VarInt, var_uint::VarUInt},
    };

    use super::*;

    fn item(id: i16) -> NetworkItemDescriptor {
        NetworkItemDescriptor {
            id: VarInt(i32::from(id)),
            stack_size: 1,
            ..NetworkItemDescriptor::default()
        }
    }

    /// Construction: dirt on its own, then the planks group of two. Nature: one item.
    fn list() -> CreativeList {
        let group = |creative_category, name: &str, icon_id| CreativeGroupInfoPayload {
            creative_category,
            name: name.to_string(),
            group_icon_item: if icon_id == 0 { NetworkItemDescriptor::default() } else { item(icon_id) },
        };
        let entry = |id, item_id, group_index| CreativeItemEntryPayload {
            id: VarUInt(id),
            item: item(item_id),
            group_index: VarUInt(group_index),
        };
        CreativeList::new(&CreativeContent {
            groups: vec![
                group(CreativeCategory::Construction, "", 0),
                group(CreativeCategory::Construction, "itemGroup.name.planks", 5),
                group(CreativeCategory::Nature, "", 0),
            ],
            entries: vec![
                entry(1, 3, 0),
                entry(2, 5, 1),
                entry(3, 6, 1),
                entry(4, 9, 2),
                // A group that doesn't exist is left out.
                entry(5, 9, 7),
            ],
        })
    }

    #[test]
    fn a_tab_shows_its_items_and_its_groups_shut() {
        let list = list();
        assert_eq!(list.len(), 4);
        assert_eq!(
            list.cells(Tab::Construction, &HashSet::new()),
            [
                Cell::Item { entry: 0, grouped: false },
                Cell::Group { group: 1, open: false },
            ]
        );
        assert_eq!(list.cells(Tab::Nature, &HashSet::new()), [Cell::Item { entry: 3, grouped: false }]);
        assert!(list.cells(Tab::Items, &HashSet::new()).is_empty());
    }

    #[test]
    fn an_open_group_shows_its_items_after_it() {
        let list = list();
        let open = HashSet::from([1]);
        assert_eq!(
            list.cells(Tab::Construction, &open),
            [
                Cell::Item { entry: 0, grouped: false },
                Cell::Group { group: 1, open: true },
                Cell::Item { entry: 1, grouped: true },
                Cell::Item { entry: 2, grouped: true },
            ]
        );
    }

    #[test]
    fn a_group_shows_as_its_icon_or_its_first_item() {
        let list = list();
        assert_eq!(list.group_icon(1), Some((5, 0)));
        assert_eq!(list.group_icon(0), Some((3, 0)), "no icon of its own");
    }

    #[test]
    fn the_tabs_go_by_the_official_clients_numbers() {
        let numbers: Vec<i32> = Tab::ALL.iter().map(|tab| tab.options_index()).collect();
        assert_eq!(numbers, [1, 2, 3, 4]);
    }
}
