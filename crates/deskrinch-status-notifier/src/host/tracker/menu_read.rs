//! A read of an open menu, as one of the tracker's futures: the match for
//! the menu's signals made first, and that stream drained while the read
//! waits for its replies.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::Instant;

use deskrinch_timer::Timer;
use futures_core::Stream;
use zbus::Connection;

use super::sources::{Finished, MenuRead, Pending};
use crate::dbus::{menu, streams};
use crate::host::shared::Shared;
use crate::limits::Limits;
use crate::model::{MenuDue, MenuPlace};

/// Messages a menu read takes from its stream in one poll: a stream's
/// queue (zbus's default).
const DRAIN_AT_ONCE: usize = 64;

/// The menu read `due`, once its time has come, from wherever the menu is
/// then; nothing when it was closed by then.
///
/// The match for the menu's signals there is made first, so that it is on
/// the bus before the read's first call: a change after the read is never
/// missed. When the place is followed already, zbus only counts one more
/// user of the match, without a call. The read's own stream is drained
/// while the read waits for its replies (a stream left full would stall
/// every reply behind it), and a change it shows asks for one more read;
/// the tracker then keeps the stream, unless it follows the place already
/// or the menu has gone.
pub(super) fn menu_read(
    connection: Connection,
    timer: Timer,
    shared: Arc<Shared>,
    limits: Limits,
    due: MenuDue,
) -> Pending {
    let wait = (due.at > Instant::now()).then(|| timer.sleep_until(due.at));
    Box::pin(async move {
        if let Some(wait) = wait {
            wait.await;
        }
        let Some(source) = shared.with_model(|m| m.menu_source(&due.id)) else {
            return Finished::Nothing;
        };
        let place = MenuPlace {
            id: source.id.clone(),
            owner: source.owner.clone(),
            path: source.path.clone(),
        };
        // It fails only when the connection is gone, which the end of the
        // owners' stream reports.
        let mut stream = streams::menu_signals(&connection, &place.owner, &place.path)
            .await
            .ok();
        let mut read = pin!(menu::read(connection, timer, source, limits));
        let mut signalled = false;
        let (source, outcome) = poll_fn(|cx| {
            if let Some(stream) = stream.as_mut() {
                // A queue's worth at a time, so that a menu that floods
                // cannot keep the read from being polled.
                let mut drained = 0;
                while drained < DRAIN_AT_ONCE {
                    match Pin::new(&mut *stream).poll_next(cx) {
                        Poll::Ready(Some(message)) => {
                            signalled |= message.is_ok_and(|m| streams::menu_signal(&m).is_some());
                            drained += 1;
                        }
                        Poll::Ready(None) | Poll::Pending => break,
                    }
                }
                // More may be waiting: this future is polled again soon.
                if drained == DRAIN_AT_ONCE {
                    cx.waker().wake_by_ref();
                }
            }
            read.as_mut().poll(cx)
        })
        .await;
        Finished::MenuRead(Box::new(MenuRead {
            source,
            outcome,
            place,
            stream,
            signalled,
        }))
    })
}
