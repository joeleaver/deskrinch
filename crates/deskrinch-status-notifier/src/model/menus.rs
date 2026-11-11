//! The open menus: which items' menus the caller has open, their reads
//! (paced and coalesced by the same state machine as the items' reads),
//! what was last read of each, and which entry is which from one reading
//! to the next.
//!
//! A menu is read only while it is open: once on opening, again whenever
//! the application says it changed (`LayoutUpdated`,
//! `ItemsPropertiesUpdated`), when its item moves to another connection,
//! and when the item's `Menu` moves to another object. Every read is of
//! the whole layout.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::read::Reads;
use super::{Change, Entry, Model};
use crate::item::ItemId;
use crate::menu::{EntryId, EntryKind, Menu, MenuEntry, MenuFailure, Toggle};
use MenuOutcome as Outcome;

/// An item's open menu.
#[derive(Debug)]
pub(super) struct OpenMenu {
    /// The object it is read from: the item's `Menu`.
    path: String,
    reads: Reads,
    menu: Option<Arc<Menu>>,
    /// Decoded pixels of `menu`'s icons, counted in the model's total.
    image_bytes: usize,
    /// Whether a read has started yet: the first is preceded by
    /// `AboutToShow`.
    started: bool,
}

/// A read of the item `id`'s menu to start no earlier than `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuDue {
    pub(crate) id: ItemId,
    pub(crate) at: Instant,
}

/// Whom a read of the item `id`'s menu goes to, decided as it starts: the
/// object `path` of connection `owner`, by its unique name. The first read
/// of an opening is preceded by `AboutToShow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuSource {
    pub(crate) id: ItemId,
    pub(crate) owner: String,
    pub(crate) path: String,
    pub(crate) about_to_show: bool,
}

/// Where a menu's events go: the item's registered name (calls that expect
/// no reply) and the menu's object; the item's own object for an
/// activation token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuTarget {
    pub(crate) service: String,
    pub(crate) item_path: String,
    pub(crate) path: String,
}

/// What a read of a menu came to.
#[derive(Debug)]
pub(crate) enum MenuOutcome {
    /// The layout, within bounds; its entries' ids not yet told apart.
    Read(Box<Menu>),
    /// The connection the read was addressed to does not exist.
    Gone,
    /// The read failed, and why.
    Failed(MenuFailure),
}

/// What [`Model::menu_read_done`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuDone {
    /// The change the caller hears of.
    pub(crate) change: Option<Change>,
    /// The next read, asked for while this one was in flight.
    pub(crate) next: Option<MenuDue>,
}

/// Why an operation on a menu was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuRefusal {
    /// No item with this id has been read.
    NotFound,
    /// The item has no menu, or its menu is not open.
    NoMenu,
    /// The open menu has no entry that is this one now, or not one that can
    /// do this.
    NoEntry,
}

impl Model {
    /// Opens the item `id`'s menu at `now`: its first read, due at once;
    /// `None` when it is open already.
    pub(crate) fn open_menu(
        &mut self,
        id: &ItemId,
        now: Instant,
    ) -> Result<Option<(MenuTarget, MenuDue)>, MenuRefusal> {
        let entry = self.read_entry(id)?;
        if entry.menu.is_some() {
            return Ok(None);
        }
        let path = entry.item_menu().ok_or(MenuRefusal::NoMenu)?;
        let target = entry.menu_target(&path);
        entry.menu = Some(OpenMenu {
            path,
            reads: Reads::first(now),
            menu: None,
            image_bytes: 0,
            started: false,
        });
        Ok(Some((
            target,
            MenuDue {
                id: id.clone(),
                at: now,
            },
        )))
    }

    /// Closes the item `id`'s menu, releasing what was read of it; where
    /// to say so, or `None` when it was not open.
    pub(crate) fn close_menu(&mut self, id: &ItemId) -> Option<MenuTarget> {
        let entry = self.entries.iter_mut().find(|e| &e.id == id)?;
        let open = entry.menu.take()?;
        self.image_bytes -= open.image_bytes;
        Some(entry.menu_target(&open.path))
    }

    /// The item `id`'s open menu, as last read.
    pub(crate) fn menu(&self, id: &ItemId) -> Option<Arc<Menu>> {
        self.entry(id)?.menu.as_ref()?.menu.clone()
    }

    /// Where to send an event about `entry` of the item `id`'s open menu:
    /// only while the menu has an entry that is still the one `entry`
    /// names, of a kind that `accepts`.
    pub(crate) fn entry_target(
        &self,
        id: &ItemId,
        entry: EntryId,
        accepts: impl Fn(&MenuEntry) -> bool,
    ) -> Result<MenuTarget, MenuRefusal> {
        let item = self.entry(id).ok_or(MenuRefusal::NotFound)?;
        let open = item.menu.as_ref().ok_or(MenuRefusal::NoMenu)?;
        let menu = open.menu.as_ref().ok_or(MenuRefusal::NoEntry)?;
        MenuEntry::walk(&menu.entries)
            .find(|e| e.id == entry)
            .filter(|e| accepts(e))
            .ok_or(MenuRefusal::NoEntry)?;
        Ok(item.menu_target(&open.path))
    }

    /// Whom a read of the item `id`'s menu starting now goes to; `None`
    /// when the menu was closed or the item left, and the read is dropped.
    pub(crate) fn menu_source(&mut self, id: &ItemId) -> Option<MenuSource> {
        let entry = self.entries.iter_mut().find(|e| &e.id == id)?;
        let open = entry.menu.as_mut()?;
        let about_to_show = !open.started;
        open.started = true;
        Some(MenuSource {
            id: id.clone(),
            owner: entry.owner.clone(),
            path: open.path.clone(),
            about_to_show,
        })
    }

    /// Where the item `id`'s open menu is now, for a call that expects a
    /// reply and is not a read (`AboutToShow` for a submenu).
    pub(crate) fn menu_peer(&self, id: &ItemId) -> Option<MenuSource> {
        let entry = self.entry(id)?;
        let open = entry.menu.as_ref()?;
        Some(MenuSource {
            id: id.clone(),
            owner: entry.owner.clone(),
            path: open.path.clone(),
            about_to_show: false,
        })
    }

    /// The menu at `path` of connection `sender` said at `now` that it
    /// changed: a read of every open menu there, unless one is due or in
    /// flight.
    pub(crate) fn menu_signalled(
        &mut self,
        sender: &str,
        path: &str,
        now: Instant,
    ) -> Vec<MenuDue> {
        let interval = self.limits.min_read_interval;
        self.entries
            .iter_mut()
            .filter(|e| e.owner == sender)
            .filter_map(|e| {
                let open = e.menu.as_mut().filter(|m| m.path == path)?;
                let at = open.reads.ask(now, interval)?;
                Some(MenuDue {
                    id: e.id.clone(),
                    at,
                })
            })
            .collect()
    }

    /// Something about the item `id`'s open menu changed at `now`: its read,
    /// unless one is due or in flight.
    pub(crate) fn ask_menu(&mut self, id: &ItemId, now: Instant) -> Option<MenuDue> {
        let interval = self.limits.min_read_interval;
        let entry = self.entries.iter_mut().find(|e| &e.id == id)?;
        entry.ask_menu(now, interval)
    }

    /// A read of the item `id`'s menu, addressed to the object `path` of
    /// connection `owner`, ended at `now` with `outcome`.
    ///
    /// A read from a connection or object that is no longer the menu's
    /// says nothing about it, whatever it answered: the read that the move
    /// asked for follows. A first read that fails closes the menu, and the
    /// caller hears why; a later one keeps what was last read.
    pub(crate) fn menu_read_done(
        &mut self,
        source: &MenuSource,
        outcome: MenuOutcome,
        now: Instant,
    ) -> MenuDone {
        let mut done = MenuDone::default();
        let interval = self.limits.min_read_interval;
        let Some(index) = self.entries.iter().position(|e| e.id == source.id) else {
            return done;
        };
        let entry = &self.entries[index];
        let Some(open) = entry.menu.as_ref() else {
            return done;
        };
        let current = entry.owner == source.owner && open.path == source.path;
        match outcome {
            Outcome::Read(menu) if current => done.change = self.store_menu(index, *menu),
            Outcome::Failed(reason) if current && open.menu.is_none() => {
                self.close_menu(&source.id);
                done.change = Some(Change::MenuFailed(source.id.clone(), reason));
                return done;
            }
            // Gone from the owner: the item left, which the bus's report of
            // its name says. Stale, or a later read that failed: nothing.
            Outcome::Read(_) | Outcome::Failed(_) | Outcome::Gone => {}
        }
        let entry = &mut self.entries[index];
        if let Some(open) = entry.menu.as_mut() {
            done.next = open.reads.done(now, interval).map(|at| MenuDue {
                id: entry.id.clone(),
                at,
            });
        }
        done
    }

    /// Keeps `menu` as entry `index`'s open menu: its entries told apart
    /// from the last reading's, and its icons' pixels kept while they fit
    /// within the total. The change, unless nothing changed.
    fn store_menu(&mut self, index: usize, mut menu: Menu) -> Option<Change> {
        self.readings += 1;
        let reading = self.readings;
        let entry = &mut self.entries[index];
        let open = entry.menu.as_mut()?;
        let before = open.menu.as_deref().map(identities).unwrap_or_default();
        identify(&mut menu.entries, &before, reading);
        let budget = self.limits.max_image_bytes_total - (self.image_bytes - open.image_bytes);
        let bytes = keep_pixels(&mut menu.entries, budget);
        self.image_bytes = self.image_bytes - open.image_bytes + bytes;
        open.image_bytes = bytes;
        if open.menu.as_deref() == Some(&menu) {
            return None;
        }
        let menu = Arc::new(menu);
        open.menu = Some(Arc::clone(&menu));
        Some(Change::Menu(menu))
    }
}

impl Entry {
    /// Where events about the item's menu at `path` go.
    fn menu_target(&self, path: &str) -> MenuTarget {
        MenuTarget {
            service: self.target.service.clone(),
            item_path: self.target.path.clone(),
            path: path.to_owned(),
        }
    }

    /// The object path of the item's menu, once read.
    pub(super) fn item_menu(&self) -> Option<String> {
        self.item.as_ref()?.menu.clone()
    }

    /// A read of the open menu at `now`, unless one is due or in flight.
    pub(super) fn ask_menu(&mut self, now: Instant, interval: Duration) -> Option<MenuDue> {
        let at = self.menu.as_mut()?.reads.ask(now, interval)?;
        Some(MenuDue {
            id: self.id.clone(),
            at,
        })
    }

    /// The item was read again: if its menu is open and the item's `Menu`
    /// moved, the menu is read from where it is now, at `now`; if the item
    /// has none any more, the menu closes, and the caller hears why. The
    /// pixels freed, the change, and the read.
    pub(super) fn menu_moved(
        &mut self,
        now: Instant,
        interval: Duration,
    ) -> (usize, Option<Change>, Option<MenuDue>) {
        let path = self.item_menu();
        let Some(open) = self.menu.as_mut() else {
            return (0, None, None);
        };
        match path {
            Some(path) if path == open.path => (0, None, None),
            Some(path) => {
                open.path = path;
                (0, None, self.ask_menu(now, interval))
            }
            None => {
                let freed = self.menu.take().map_or(0, |m| m.image_bytes);
                let failed = Change::MenuFailed(self.id.clone(), MenuFailure::NoMenu);
                (freed, Some(failed), None)
            }
        }
    }

    /// Pixels of the open menu's icons.
    pub(super) fn menu_image_bytes(&self) -> usize {
        self.menu.as_ref().map_or(0, |m| m.image_bytes)
    }
}

impl Model {
    /// The entry of the read item `id`.
    fn read_entry(&mut self, id: &ItemId) -> Result<&mut Entry, MenuRefusal> {
        self.entries
            .iter_mut()
            .find(|e| &e.id == id && e.item.is_some())
            .ok_or(MenuRefusal::NotFound)
    }
}

/// What makes an entry the same entry from one reading to the next.
type Identity = (EntryKind, String, std::mem::Discriminant<Toggle>, bool);

fn identity(entry: &MenuEntry) -> Identity {
    (
        entry.kind,
        entry.label.text.clone(),
        std::mem::discriminant(&entry.toggle),
        entry.submenu.is_some(),
    )
}

/// The last reading's entries by the application's id: what each was, and
/// since when.
fn identities(menu: &Menu) -> HashMap<i32, (Identity, u64)> {
    MenuEntry::walk(&menu.entries)
        .map(|e| (e.id.id, (identity(e), e.id.since)))
        .collect()
}

/// Gives each of `entries` the reading it has been the same since: the
/// one before's, if the entry with its id then was this entry; `reading`
/// otherwise.
fn identify(entries: &mut [MenuEntry], before: &HashMap<i32, (Identity, u64)>, reading: u64) {
    for entry in entries {
        let same = before
            .get(&entry.id.id)
            .filter(|(was, _)| *was == identity(entry));
        entry.id.since = same.map_or(reading, |(_, since)| *since);
        if let Some(submenu) = &mut entry.submenu {
            identify(submenu, before, reading);
        }
    }
}

/// Keeps the pixels of `entries`' icons, in order, while they fit within
/// `budget` bytes, dropping the rest; the bytes kept.
fn keep_pixels(entries: &mut [MenuEntry], mut budget: usize) -> usize {
    fn walk(entries: &mut [MenuEntry], budget: &mut usize, kept: &mut usize) {
        for entry in entries {
            entry.icon.pixmaps.retain(|p| {
                let bytes = p.rgba().len();
                let fits = bytes <= *budget;
                if fits {
                    *budget -= bytes;
                    *kept += bytes;
                }
                fits
            });
            if let Some(submenu) = &mut entry.submenu {
                walk(submenu, budget, kept);
            }
        }
    }
    let mut kept = 0;
    walk(entries, &mut budget, &mut kept);
    kept
}
