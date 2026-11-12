//! The application launcher, deskrinch's second component.
//!
//! The launcher runs for the whole session as one process with one layer
//! surface: in the overlay layer, centred, taking all keyboard input while
//! it is shown, and unmapped while hidden. It is summoned through its own
//! session-bus interface, finds installed applications as the user types,
//! and starts the chosen one.
//!
//! The library holds what can be tested without a display; the
//! `deskrinch-launcher` binary is the process around it:
//!
//! - [`Model`]: what the launcher shows and what a key does: the query, the
//!   ranked results ([`Row`]s) and the selection. Pure: it needs no
//!   display, bus or files, and is tested on its own.
//! - [`Visibility`]: whether the launcher is shown, one value that the bus
//!   and the launcher's own UI change and its window follows.
//! - [`Service`]: the launcher's interface on the session bus
//!   (`org.deskrinch.Launcher`: `Toggle`, `Show`, `Hide`, `ForgetStarts`),
//!   whose name makes it one per session.
//! - [`send`]: the other end, what `deskrinch-launcher toggle` uses.
//!
//! # The process
//!
//! `deskrinch-launcher` runs the launcher: it takes its name on the session
//! bus, and makes its window hidden (`--show`: shown). Another launcher
//! started while one runs finds the name taken and leaves (asking the
//! running one to show itself, with `--show`).
//!
//! `deskrinch-launcher toggle`, `show` and `hide` ask the running launcher,
//! and are what a key binding runs. With none running, `toggle` and `show`
//! start one shown; `hide` does nothing.
//!
//! `deskrinch-launcher forget-starts` deletes the record of the
//! applications started: the running launcher empties its copy and deletes
//! the file, and answers once it is gone; with none running, the command
//! deletes the file itself and starts nothing.
//!
//! # The record of starts
//!
//! The launcher holds the shell's record of starts
//! (`deskrinch-start-record`) on a thread of its own, which does all its
//! file work in turn: it loads the record at start-up, before the first
//! show; records each application the launcher started (once the start
//! succeeded); deletes the record on `ForgetStarts`; and follows
//! `[general]` `remember-starts`, deleting the record when it is off (at
//! start-up too) and loading it again when it is turned back on. Each
//! change goes to the UI, which ranks it then: showing does no file work
//! and no ranking.
//!
//! # How the launcher behaves
//!
//! These are the model's rules; the process follows the [`Action`] each
//! [`Input`] gives.
//!
//! - The results are the applications shown in menus that match the query,
//!   by `deskrinch-desktop-entries`' search ranking.
//! - An empty query's results are the [`FREQUENT`] applications the record
//!   of starts (`deskrinch-start-record`) ranks best, among those shown in
//!   menus ([`frequent`]), drawn as any results are; with no record, or
//!   `[general]` `remember-starts = false`, there are none. A change to them
//!   while the launcher is shown is not drawn in place of what is shown:
//!   they are the results from the next hide on, or the next time the
//!   query is emptied. (While shown, an application removed meanwhile
//!   leaves them, as it leaves a search's.)
//! - The first result is selected whenever the query changes. Up and Down
//!   move the selection, stopping at the first and the last result.
//! - Eight results are drawn at once. The list moves only when the
//!   selection would leave the drawn ones, and starts at the top whenever
//!   the query changes.
//! - When the applications change while the launcher is shown (one is
//!   installed or removed), the results are searched again and the
//!   selection stays on the same application if it is still there.
//! - Enter starts the selected application; with none selected it does
//!   nothing. Escape hides the launcher.
//! - One application is started per show: after the first, Enter and
//!   clicks do nothing until the launcher is shown again.
//! - A click on a result starts it: the primary pointer button going down
//!   on it and coming up on it. Down on one result and up on another, or
//!   off the results, starts nothing. The pointer never moves the
//!   selection.
//! - An application that runs in a terminal (`Terminal=true`) starts in
//!   the one `[general]` `terminal` names, else in `xdg-terminal-exec`;
//!   with neither, it is not started, and that is reported.
//! - Losing the keyboard while shown hides the launcher, whatever the query
//!   says. A compositor moves the keyboard elsewhere when the user turns to
//!   another window, and KWin moves it to an application the launcher
//!   started as soon as that maps: either way the launcher has been left,
//!   and it never takes the keyboard back.
//! - The launcher starts with an empty query each time it is shown: the
//!   process [`clear`](Model::clear)s the model when it hides, which leaves
//!   the frequent applications as its results, ready before the next show.

mod client;
mod model;
mod service;
mod visibility;

pub use client::{Answer, ClientError, Request, send};
pub use model::{Action, FREQUENT, Input, Model, ROWS, Row, frequent};
pub use service::{INTERFACE, NAME, PATH, ServeError, Service};
pub use visibility::Visibility;
