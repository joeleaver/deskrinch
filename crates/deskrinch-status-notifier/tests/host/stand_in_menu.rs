//! The stand-in item's menu: `com.canonical.dbusmenu` answered by hand, so
//! that a test can send any layout, answer late or not at all, and see the
//! calls the host makes.

use std::collections::HashMap;
use std::time::Duration;

use zbus::message::Header;
use zbus::wire::{Structure, StructureBuilder, Variant};
use zbus::{Connection, Message};

/// One entry of a layout to send: its id, its properties, its children.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) id: i32,
    pub(crate) properties: Vec<(&'static str, Prop)>,
    pub(crate) children: Vec<Entry>,
}

/// A property value to send.
#[derive(Debug, Clone)]
pub(crate) enum Prop {
    Text(String),
    Flag(bool),
    Number(i32),
}

impl Entry {
    /// A standard entry labelled `label`.
    pub(crate) fn labelled(id: i32, label: &str) -> Self {
        Self {
            id,
            properties: vec![("label", Prop::Text(label.into()))],
            children: Vec::new(),
        }
    }

    /// An entry with a submenu of `children`.
    pub(crate) fn submenu(id: i32, label: &str, children: Vec<Self>) -> Self {
        let mut entry = Self::labelled(id, label);
        entry
            .properties
            .push(("children-display", Prop::Text("submenu".into())));
        entry.children = children;
        entry
    }

    /// The root of a menu of `entries`.
    pub(crate) fn root(entries: Vec<Self>) -> Self {
        Self {
            id: 0,
            properties: vec![("children-display", Prop::Text("submenu".into()))],
            children: entries,
        }
    }

    pub(crate) fn with(mut self, key: &'static str, value: Prop) -> Self {
        self.properties.push((key, value));
        self
    }

    fn structure(&self) -> Structure<'static> {
        let properties: HashMap<&str, Variant<'static>> = self
            .properties
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Prop::Text(s) => Variant::from(s.clone()),
                    Prop::Flag(b) => Variant::from(*b),
                    Prop::Number(n) => Variant::from(*n),
                };
                (*k, v)
            })
            .collect();
        let children: Vec<Variant<'static>> = self
            .children
            .iter()
            .map(|c| Variant::from(c.structure()))
            .collect();
        StructureBuilder::new()
            .add_field(self.id)
            .add_field(properties)
            .add_field(children)
            .build()
            .unwrap()
    }
}

/// How the stand-in answers `GetLayout`.
#[derive(Debug, Clone)]
pub(crate) enum Layout {
    /// With this root.
    Root(Entry),
    /// With an error, as an object without a menu does.
    Error,
    /// Not at all.
    Silent,
}

/// How the stand-in answers `AboutToShow`.
#[derive(Debug, Clone)]
pub(crate) enum AboutToShow {
    /// With this answer, after this long, having switched to `then` (as an
    /// application builds its menu then).
    Answer {
        needs_update: bool,
        after: Duration,
        then: Option<Layout>,
    },
    /// With an error, as libdbusmenu does for an id it does not know.
    Error,
    /// Not at all.
    Silent,
}

/// The stand-in's menu: what it answers, at each object path it serves a
/// menu at.
#[derive(Debug, Clone)]
pub(crate) struct Menus {
    pub(crate) layouts: HashMap<String, Layout>,
    pub(crate) about_to_show: AboutToShow,
}

impl Default for Menus {
    fn default() -> Self {
        Self {
            layouts: HashMap::new(),
            about_to_show: AboutToShow::Answer {
                needs_update: false,
                after: Duration::ZERO,
                then: None,
            },
        }
    }
}

/// A menu call the stand-in received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MenuCall {
    GetLayout(String),
    AboutToShow(String, i32),
    Event(String, i32, String),
}

/// Answers `message`, a call of the menu's interface at `path`, from
/// `menus`; the call it was. `None` when it is not a menu call.
pub(crate) async fn handle(
    connection: &Connection,
    header: &Header<'_>,
    message: &Message,
    path: &str,
    menus: &std::sync::Mutex<Menus>,
) -> Option<MenuCall> {
    let member = header.member()?.as_str();
    let body = message.body();
    match member {
        "GetLayout" => {
            let layout = menus.lock().unwrap().layouts.get(path).cloned();
            match layout {
                Some(Layout::Root(root)) => {
                    // A structure as the body is sent as its fields, the
                    // revision and the root, as two arguments are: the
                    // header gives the signature without its parentheses.
                    let body = StructureBuilder::new()
                        .add_field(1_u32)
                        .add_field(root.structure())
                        .build()
                        .unwrap();
                    let reply = Message::method_return(header).build(&body).unwrap();
                    connection.send(&reply).await.unwrap();
                }
                Some(Layout::Silent) => {}
                Some(Layout::Error) | None => error(connection, header).await,
            }
            Some(MenuCall::GetLayout(path.to_owned()))
        }
        "AboutToShow" => {
            let id: i32 = body.deserialize().unwrap();
            let answer = menus.lock().unwrap().about_to_show.clone();
            match answer {
                AboutToShow::Answer {
                    needs_update,
                    after,
                    then,
                } => {
                    if !after.is_zero() {
                        std::thread::sleep(after);
                    }
                    if let Some(then) = then {
                        menus.lock().unwrap().layouts.insert(path.to_owned(), then);
                    }
                    let reply = Message::method_return(header)
                        .build(&(needs_update,))
                        .unwrap();
                    connection.send(&reply).await.unwrap();
                }
                AboutToShow::Error => error(connection, header).await,
                AboutToShow::Silent => {}
            }
            Some(MenuCall::AboutToShow(path.to_owned(), id))
        }
        "Event" => {
            let (id, event, _data, _time): (i32, String, zbus::wire::OwnedVariant, u32) =
                body.deserialize().unwrap();
            Some(MenuCall::Event(path.to_owned(), id, event))
        }
        _ => None,
    }
}

async fn error(connection: &Connection, header: &Header<'_>) {
    let reply = Message::error(header, "org.freedesktop.DBus.Error.UnknownObject")
        .build(&("no menu here",))
        .unwrap();
    connection.send(&reply).await.unwrap();
}
