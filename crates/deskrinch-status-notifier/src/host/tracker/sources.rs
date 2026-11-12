//! What the tracker waits on, and how it polls it: its calls that expect a
//! reply (each a future), every name's changes of owner, the items'
//! signals, and each open menu's. All of them are polled in the one loop,
//! so that no stream goes unread while a reply is awaited.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_util::stream::FuturesUnordered;
use zbus::{Message, MessageStream};

use crate::host::StartError;
use crate::host::shared::{Shared, Work};
use crate::item::ItemId;
use crate::model::{MenuOutcome, MenuPlace, MenuSource, Outcome, Source};

/// What one of the tracker's calls that expect a reply came to.
pub(super) enum Finished {
    /// The items' match is on the bus: the next stage of starting.
    Items(Box<Result<MessageStream, StartError>>),
    /// The host's names are owned, or could not be.
    Started(Box<Result<(), StartError>>),
    Read(Source, Outcome),
    MenuRead(Box<MenuRead>),
    /// `AboutToShow` for a submenu of the item's menu answered that the
    /// menu needs reading again.
    MenuStale(ItemId),
    /// Nothing to apply: the read's item or menu went while it waited, or
    /// the answer asked for nothing.
    Nothing,
}

/// A read of the open menu at `place`, with the match for its signals made
/// before it, and whether the menu signalled meanwhile.
pub(super) struct MenuRead {
    pub(super) source: MenuSource,
    pub(super) outcome: MenuOutcome,
    pub(super) place: MenuPlace,
    pub(super) stream: Option<MessageStream>,
    pub(super) signalled: bool,
}

/// A call that expects a reply, in flight.
pub(super) type Pending = Pin<Box<dyn Future<Output = Finished> + Send>>;

/// What woke the tracker.
pub(super) enum Step {
    Stop,
    Disconnected,
    Work(Work),
    Finished(Finished),
    Owner(Message),
    Signal(Message),
    MenuSignal(Message),
    Nothing,
}

/// An open menu's signals, followed while it is open: a match on the bus
/// for its owner and object alone, dropped (and so removed from the bus)
/// once the menu is not there any more.
pub(super) struct Watch {
    pub(super) menu: MenuPlace,
    pub(super) stream: MessageStream,
}

/// What the tracker waits on, besides its inbox.
pub(super) struct Sources {
    pub(super) pending: FuturesUnordered<Pending>,
    pub(super) owners: MessageStream,
    /// From the first stage of starting on.
    pub(super) items: Option<MessageStream>,
    pub(super) watches: Vec<Watch>,
}

/// Polls each source once, starting from a different one each turn, so
/// that a busy one cannot starve the others; the first that is ready is
/// the step.
pub(super) fn poll_sources(
    cx: &mut Context<'_>,
    turn: usize,
    shared: &Shared,
    sources: &mut Sources,
) -> Poll<Step> {
    if let Poll::Ready(asked) = shared.poll_inbox(cx) {
        return Poll::Ready(asked.map_or(Step::Stop, Step::Work));
    }
    for index in 0..4 {
        let step = match (turn + index) % 4 {
            0 => match Pin::new(&mut sources.pending).poll_next(cx) {
                Poll::Ready(Some(finished)) => Some(Step::Finished(finished)),
                // Nothing in flight; the inbox wakes the tracker for more.
                Poll::Ready(None) | Poll::Pending => None,
            },
            1 => message_step(Pin::new(&mut sources.owners).poll_next(cx), Step::Owner),
            2 => sources
                .items
                .as_mut()
                .and_then(|items| message_step(Pin::new(items).poll_next(cx), Step::Signal)),
            _ => poll_watches(cx, turn, &mut sources.watches),
        };
        if let Some(step) = step {
            return Poll::Ready(step);
        }
    }
    Poll::Pending
}

/// Polls each open menu's signals once, from a different one each turn.
fn poll_watches(cx: &mut Context<'_>, turn: usize, watches: &mut [Watch]) -> Option<Step> {
    let count = watches.len();
    (0..count).find_map(|index| {
        let watch = &mut watches[(turn + index) % count];
        message_step(Pin::new(&mut watch.stream).poll_next(cx), Step::MenuSignal)
    })
}

fn message_step(
    polled: Poll<Option<zbus::Result<Message>>>,
    step: fn(Message) -> Step,
) -> Option<Step> {
    match polled {
        Poll::Ready(Some(Ok(message))) => Some(step(message)),
        // A message that failed to parse is not one this host follows.
        Poll::Ready(Some(Err(_))) => Some(Step::Nothing),
        Poll::Ready(None) => Some(Step::Disconnected),
        Poll::Pending => None,
    }
}
