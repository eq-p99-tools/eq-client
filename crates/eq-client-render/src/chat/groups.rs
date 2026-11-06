//! Ordered channel membership and selection, independent of chat history.
use crate::windows::WindowId;
use eq_client_core::chat::ChatTab;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

const MAIN: WindowId = WindowId::Chat;
const PREFIX: &str = "chat-tabs\t";

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Group {
    pub(super) tabs: Vec<ChatTab>,
    pub(super) active: Option<ChatTab>,
}

/// A tab appears in exactly one group. The main dock always has a frame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Groups {
    groups: BTreeMap<WindowId, Group>,
    owner: WindowId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Insert {
    Before(ChatTab),
    After(ChatTab),
    End,
}

impl Default for Groups {
    fn default() -> Self {
        Self {
            groups: BTreeMap::from([(
                MAIN,
                Group {
                    tabs: ChatTab::ALL.to_vec(),
                    active: Some(ChatTab::All),
                },
            )]),
            owner: MAIN,
        }
    }
}

impl Groups {
    pub(super) fn get(&self, id: WindowId) -> Option<&Group> {
        self.groups.get(&id)
    }

    pub(crate) fn contains(&self, id: WindowId) -> bool {
        self.groups.contains_key(&id)
    }

    pub(super) fn detached(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.groups.keys().copied().filter(|id| *id != MAIN)
    }

    pub(super) const fn owner(&self) -> WindowId {
        self.owner
    }

    pub(super) fn active(&self, id: WindowId) -> Option<ChatTab> {
        self.get(id)?.active
    }

    pub(super) fn focus(&mut self, id: WindowId) -> bool {
        if self.active(id).is_none() {
            return false;
        }
        self.owner = id;
        true
    }

    pub(super) fn select(&mut self, id: WindowId, tab: ChatTab) {
        if let Some(group) = self.groups.get_mut(&id)
            && group.tabs.contains(&tab)
        {
            group.active = Some(tab);
            self.owner = id;
        }
    }

    /// Move one tab, using a tab identity as an insertion anchor so removing
    /// the source works the same for leftward and rightward reorders.
    pub(super) fn move_tab(&mut self, source: WindowId, tab: ChatTab, to: WindowId, at: Insert) {
        let anchor = match at {
            Insert::Before(anchor) | Insert::After(anchor) => Some(anchor),
            Insert::End => None,
        };
        if anchor == Some(tab)
            || self
                .get(to)
                .is_none_or(|group| anchor.is_some_and(|a| !group.tabs.contains(&a)))
        {
            return;
        }
        let Some(group) = self.groups.get_mut(&source) else {
            return;
        };
        let Some(index) = group.tabs.iter().position(|other| *other == tab) else {
            return;
        };
        group.tabs.remove(index);
        if group.active == Some(tab) {
            group.active = group.tabs.get(index).or_else(|| group.tabs.last()).copied();
        }
        let destination = self.groups.get_mut(&to).expect("validated destination");
        let index = anchor
            .and_then(|anchor| destination.tabs.iter().position(|other| *other == anchor))
            .map_or(destination.tabs.len(), |index| {
                index + usize::from(matches!(at, Insert::After(_)))
            });
        destination.tabs.insert(index, tab);
        destination.active = Some(tab);
        self.owner = to;
        if source != MAIN && self.groups[&source].tabs.is_empty() {
            self.groups.remove(&source);
        }
    }

    pub(super) fn can_detach(&self, source: WindowId) -> bool {
        self.get(source)
            .is_some_and(|group| source == MAIN || group.tabs.len() > 1)
    }

    /// A lone floating tab already has its own movable frame.
    pub(super) fn detach(&mut self, source: WindowId, tab: ChatTab) -> Option<WindowId> {
        if !self.can_detach(source) || !self.get(source)?.tabs.contains(&tab) {
            return None;
        }
        let limit = u8::try_from(ChatTab::ALL.len()).expect("bounded channel count");
        let id = (1..=limit)
            .map(WindowId::ChatGroup)
            .find(|id| !self.contains(*id))?;
        self.groups.insert(id, Group::default());
        self.move_tab(source, tab, id, Insert::End);
        Some(id)
    }

    /// Close a floating frame by docking every tab back in its order.
    pub(super) fn close(&mut self, id: WindowId) {
        if id == MAIN {
            return;
        }
        let Some(group) = self.groups.remove(&id) else {
            return;
        };
        let main = self.groups.get_mut(&MAIN).expect("permanent main dock");
        main.tabs.extend(group.tabs);
        if self.owner == id || main.active.is_none() {
            main.active = group.active;
        }
        if self.owner == id {
            self.owner = MAIN;
        }
    }

    /// Appends arrangement records to the same profile file as the placements.
    pub(crate) fn write(&self, text: &mut String) {
        for (id, group) in &self.groups {
            let active = group.active.map_or("-", ChatTab::label);
            let _ = write!(text, "{PREFIX}{} {active}", id.key());
            for tab in &group.tabs {
                let _ = write!(text, " {}", tab.label());
            }
            text.push('\n');
        }
    }

    /// Older files have no group records. Keep every known channel even if a
    /// file was edited by hand or had fields from another client version.
    pub(crate) fn read(text: &str) -> Self {
        let mut groups = BTreeMap::new();
        let mut assigned = BTreeSet::new();
        let tab = |label: &str| ChatTab::ALL.into_iter().find(|tab| tab.label() == label);
        for line in text.lines().filter_map(|line| line.strip_prefix(PREFIX)) {
            let mut fields = line.split_ascii_whitespace();
            let Some(id) = fields
                .next()
                .and_then(WindowId::named)
                .filter(|id| id.is_chat())
            else {
                continue;
            };
            let Some(active) = fields.next() else {
                continue;
            };
            if groups.contains_key(&id) {
                continue;
            }
            let tabs: Vec<_> = fields
                .filter_map(tab)
                .filter(|tab| assigned.insert(*tab))
                .collect();
            if id == MAIN || !tabs.is_empty() {
                let active = tab(active)
                    .filter(|tab| tabs.contains(tab))
                    .or_else(|| tabs.first().copied());
                groups.insert(id, Group { tabs, active });
            }
        }
        let main = groups.entry(MAIN).or_default();
        main.tabs.extend(
            ChatTab::ALL
                .into_iter()
                .filter(|tab| !assigned.contains(tab)),
        );
        main.active = main.active.or_else(|| main.tabs.first().copied());
        let owner = if main.active.is_some() {
            MAIN
        } else {
            *groups
                .keys()
                .find(|id| **id != MAIN)
                .expect("all tabs are assigned")
        };
        Self { groups, owner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_recovers_all_tabs_and_preserves_group_order_and_selection() {
        let mut layout = Groups::read(
            "chat-tabs\tchat-group-3 OOC OOC Say\n\
             chat-tabs\tchat-group-3 Tell Tell\n\
             chat-tabs\tchat Auc Guild unknown OOC Auc\n\
             chat-tabs\tchat-group-255 Group Group\n\
             chat-tabs\tchat-group-2 unknown Tell\n",
        );
        let first = WindowId::ChatGroup(3);
        let second = WindowId::ChatGroup(2);
        assert_eq!(
            layout.get(first).unwrap().tabs,
            [ChatTab::Ooc, ChatTab::Say]
        );
        assert_eq!(layout.get(second).unwrap().active, Some(ChatTab::Tell));
        assert_eq!(
            layout.get(MAIN).unwrap().tabs[..2],
            [ChatTab::Guild, ChatTab::Auction]
        );
        assert_eq!(layout.active(MAIN), Some(ChatTab::Auction));
        assert_eq!(
            layout
                .groups
                .values()
                .map(|group| group.tabs.len())
                .sum::<usize>(),
            ChatTab::ALL.len()
        );
        let mut written = String::new();
        layout.write(&mut written);
        assert_eq!(Groups::read(&written), layout);

        layout.select(first, ChatTab::Say);
        layout.close(first);
        assert_eq!(layout.owner(), MAIN);
        assert_eq!(layout.active(MAIN), Some(ChatTab::Say));
        assert!(
            layout
                .get(MAIN)
                .unwrap()
                .tabs
                .ends_with(&[ChatTab::Ooc, ChatTab::Say])
        );
        assert_eq!(
            Groups::read("# an older windows profile"),
            Groups::default()
        );
    }

    #[test]
    fn an_empty_main_group_stays_a_dock_and_a_lone_floating_tab_can_return() {
        let mut groups = Groups::default();
        for tab in ChatTab::ALL {
            assert!(groups.detach(MAIN, tab).is_some());
        }
        assert_eq!(groups.active(MAIN), None);
        let owner = groups.owner();
        assert_eq!(groups.detach(owner, ChatTab::System), None);
        let mut text = String::new();
        groups.write(&mut text);
        let mut read = Groups::read(&text);
        assert_eq!(read.active(MAIN), None);
        assert_eq!(read.detached().count(), ChatTab::ALL.len());
        read.move_tab(owner, ChatTab::System, MAIN, Insert::End);
        assert_eq!(read.active(MAIN), Some(ChatTab::System));
        assert!(!read.contains(owner));
    }
}
