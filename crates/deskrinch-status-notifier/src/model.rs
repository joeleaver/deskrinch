//! The registered items and hosts, apart from the bus: what is registered,
//! which items have been read, and which reads are due.
//!
//! Every registration is accepted (within the count bounds) and listed. An
//! item is *pending* until a read of its properties succeeds; only then does
//! the caller hear of it. A read is started on registration and on each of
//! the item's `New*` signals; while one is in flight, further signals ask
//! for one more read after it, never more than one.

use std::sync::Arc;

use crate::item::{Item, ItemId};
use crate::limits::Limits;

/// Where an item's properties are read, and its methods called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The name it registered under: a unique name, or a well-known name
    /// that the bus routes to its current owner.
    pub(crate) service: String,
    pub(crate) path: String,
}

/// A change the caller hears of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    Added(Arc<Item>),
    Changed(Arc<Item>),
    Removed(ItemId),
}

/// What a read of an item's properties came to.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// The properties, within bounds.
    Read(Box<Item>),
    /// The item's service has no owner: it left before it was read.
    Gone,
    /// The read failed, timed out or was out of bounds.
    Failed,
}

/// Why a registration was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("too many items are registered")]
    Items,
    #[error("this connection has registered too many items")]
    PerConnection,
    #[error("too many hosts are registered")]
    Hosts,
}

/// What [`Model::owner_changed`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OwnerChange {
    /// Items that left, as the watcher announces them.
    pub(crate) unregistered: Vec<ItemId>,
    /// The changes the caller hears of (removals of items it was told of).
    pub(crate) changes: Vec<Change>,
    /// Items now owned by another connection, to read again.
    pub(crate) reads: Vec<(ItemId, Target)>,
    /// Hosts that left.
    pub(crate) hosts_left: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    Idle,
    Busy,
    BusyThenAgain,
}

#[derive(Debug)]
struct Entry {
    id: ItemId,
    target: Target,
    /// The unique name of the connection that owns `target.service`, which
    /// the item's signals come from.
    owner: String,
    item: Option<Arc<Item>>,
    read: Read,
}

/// The registered items, in the order they registered, and the hosts other
/// than this process's own (which is registered for as long as it runs).
#[derive(Debug)]
pub(crate) struct Model {
    limits: Limits,
    entries: Vec<Entry>,
    hosts: Vec<String>,
    image_bytes: usize,
}

impl Model {
    /// No item, and no host but this process's own.
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: Vec::new(),
            hosts: Vec::new(),
            image_bytes: 0,
        }
    }

    /// Registers the item at `target`, whose service `owner` owns. `None`
    /// when it was registered already; otherwise its id and the read to
    /// start.
    pub(crate) fn register(
        &mut self,
        target: Target,
        owner: &str,
    ) -> Result<Option<(ItemId, Target)>, Refusal> {
        let id = ItemId::new(&target.service, &target.path);
        if self.entries.iter().any(|e| e.id == id) {
            return Ok(None);
        }
        if self.entries.len() >= self.limits.max_items {
            return Err(Refusal::Items);
        }
        let from_owner = self.entries.iter().filter(|e| e.owner == owner).count();
        if from_owner >= self.limits.max_items_per_connection {
            return Err(Refusal::PerConnection);
        }
        self.entries.push(Entry {
            id: id.clone(),
            target: target.clone(),
            owner: owner.to_owned(),
            item: None,
            read: Read::Busy,
        });
        Ok(Some((id, target)))
    }

    /// Registers another host, `service`. `false` when it was already.
    pub(crate) fn register_host(&mut self, service: &str) -> Result<bool, Refusal> {
        if self.hosts.iter().any(|h| h == service) {
            return Ok(false);
        }
        if self.hosts.len() >= self.limits.max_hosts {
            return Err(Refusal::Hosts);
        }
        self.hosts.push(service.to_owned());
        Ok(true)
    }

    /// The registered items as the watcher lists them, read or not.
    pub(crate) fn registered(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.id.to_string()).collect()
    }

    /// The items that have been read, in the order they registered.
    pub(crate) fn items(&self) -> Vec<Arc<Item>> {
        self.entries.iter().filter_map(|e| e.item.clone()).collect()
    }

    /// The item `id`, once read.
    pub(crate) fn item(&self, id: &ItemId) -> Option<Arc<Item>> {
        self.entry(id).and_then(|e| e.item.clone())
    }

    /// Where to call the item `id`'s methods; `None` unless it has been
    /// read (the caller knows of no other).
    pub(crate) fn target(&self, id: &ItemId) -> Option<Target> {
        self.entry(id)
            .filter(|e| e.item.is_some())
            .map(|e| e.target.clone())
    }

    /// The item at `path` of connection `sender` said something changed:
    /// the read to start, if one is not already in flight.
    pub(crate) fn signalled(&mut self, sender: &str, path: &str) -> Option<(ItemId, Target)> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.owner == sender && e.target.path == path)?;
        match entry.read {
            Read::Idle => {
                entry.read = Read::Busy;
                Some((entry.id.clone(), entry.target.clone()))
            }
            Read::Busy | Read::BusyThenAgain => {
                entry.read = Read::BusyThenAgain;
                None
            }
        }
    }

    /// A read of `id` ended with `outcome`: the change the caller hears of,
    /// whether the item was dropped (it left before it was read), and the
    /// next read to start.
    pub(crate) fn read_done(&mut self, id: &ItemId, outcome: Outcome) -> ReadDone {
        let mut done = ReadDone::default();
        let Some(index) = self.entries.iter().position(|e| &e.id == id) else {
            return done;
        };
        match outcome {
            Outcome::Gone => {
                let entry = self.entries.remove(index);
                done.unregistered = true;
                if let Some(item) = entry.item {
                    self.image_bytes -= item.image_bytes();
                    done.change = Some(Change::Removed(entry.id));
                }
                return done;
            }
            Outcome::Read(item) => done.change = self.store(index, *item),
            Outcome::Failed => {}
        }
        let entry = &mut self.entries[index];
        entry.read = match entry.read {
            Read::BusyThenAgain => {
                done.next = Some((entry.id.clone(), entry.target.clone()));
                Read::Busy
            }
            Read::Idle | Read::Busy => Read::Idle,
        };
        done
    }

    /// Keeps `item` as entry `index`'s, unless it would take the images
    /// over their total bound.
    fn store(&mut self, index: usize, item: Item) -> Option<Change> {
        let entry = &mut self.entries[index];
        let old_bytes = entry.item.as_ref().map_or(0, |i| i.image_bytes());
        let new_bytes = item.image_bytes();
        let total = self.image_bytes - old_bytes + new_bytes;
        if total > self.limits.max_image_bytes_total {
            return None;
        }
        if entry.item.as_deref() == Some(&item) {
            return None;
        }
        self.image_bytes = total;
        let item = Arc::new(item);
        let change = if entry.item.is_some() {
            Change::Changed(Arc::clone(&item))
        } else {
            Change::Added(Arc::clone(&item))
        };
        entry.item = Some(item);
        Some(change)
    }

    /// The bus name `name` is now owned by `new_owner`, or by nobody.
    pub(crate) fn owner_changed(&mut self, name: &str, new_owner: Option<&str>) -> OwnerChange {
        let mut change = OwnerChange::default();
        if let Some(new_owner) = new_owner {
            for entry in self.entries.iter_mut().filter(|e| e.target.service == name) {
                new_owner.clone_into(&mut entry.owner);
                match entry.read {
                    Read::Idle => {
                        entry.read = Read::Busy;
                        change.reads.push((entry.id.clone(), entry.target.clone()));
                    }
                    Read::Busy | Read::BusyThenAgain => entry.read = Read::BusyThenAgain,
                }
            }
            return change;
        }
        let mut freed = 0;
        self.entries.retain(|entry| {
            if entry.target.service != name {
                return true;
            }
            change.unregistered.push(entry.id.clone());
            if let Some(item) = &entry.item {
                freed += item.image_bytes();
                change.changes.push(Change::Removed(entry.id.clone()));
            }
            false
        });
        self.image_bytes -= freed;
        let before = self.hosts.len();
        self.hosts.retain(|h| h != name);
        change.hosts_left = before - self.hosts.len();
        change
    }

    fn entry(&self, id: &ItemId) -> Option<&Entry> {
        self.entries.iter().find(|e| &e.id == id)
    }
}

/// What [`Model::read_done`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReadDone {
    /// The change the caller hears of.
    pub(crate) change: Option<Change>,
    /// The item left before it was read and is no longer registered.
    pub(crate) unregistered: bool,
    /// The next read of the item, asked for while this one was in flight.
    pub(crate) next: Option<(ItemId, Target)>,
}

#[cfg(test)]
mod tests;
