//! KWin's virtual desktops, read through its own interface
//! (`org.kde.KWin.VirtualDesktopManager` on `/VirtualDesktopManager`), and
//! its current desktop's number (`org.kde.KWin.currentDesktop` on `/KWin`).
//! What changes them is [`KwinActions`](crate::KwinActions)'.

use zbus::OwnedVariant;

use crate::names::{KWIN, KWIN_OBJECT, MANAGER};
use crate::{Error, Kwin};

/// A virtual desktop, as KWin's manager lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDesktop {
    /// Its position among the desktops, from 0.
    pub position: u32,
    /// Its id, which KWin's calls take.
    pub id: String,
    /// Its name.
    pub name: String,
}

impl Kwin {
    /// KWin's virtual desktops, in the order its manager lists them.
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin cannot be asked; [`Error::Answer`] when its
    /// answer is not a list of desktops.
    pub fn virtual_desktops(&self) -> Result<Vec<VirtualDesktop>, Error> {
        desktops_in(self.manager_property("desktops")?)
    }

    /// KWin's current virtual desktop.
    ///
    /// # Errors
    ///
    /// As [`virtual_desktops`](Self::virtual_desktops);
    /// [`Error::NotListed`] when the current one is not among them.
    pub fn current_virtual_desktop(&self) -> Result<VirtualDesktop, Error> {
        let current = text_in(self.manager_property("current")?)?;
        self.virtual_desktops()?
            .into_iter()
            .find(|desktop| desktop.id == current)
            .ok_or(Error::NotListed { desktop: current })
    }

    /// KWin's current desktop's number, from 1 (its `currentDesktop`).
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin cannot be asked.
    pub fn current_desktop(&self) -> Result<i32, Error> {
        zbus::block_on(async {
            let reply = self
                .connect()
                .await?
                .call_method(
                    Some(KWIN),
                    KWIN_OBJECT.0,
                    Some(KWIN_OBJECT.1),
                    "currentDesktop",
                    &(),
                )
                .await?;
            Ok(reply.body().deserialize::<i32>()?)
        })
    }

    /// The manager's property `name`.
    fn manager_property(&self, name: &str) -> Result<OwnedVariant, Error> {
        zbus::block_on(async {
            let reply = self
                .connect()
                .await?
                .call_method(
                    Some(KWIN),
                    MANAGER.0,
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &(MANAGER.1, name),
                )
                .await?;
            Ok(reply.body().deserialize::<OwnedVariant>()?)
        })
    }
}

/// The desktops a `desktops` property's value lists.
fn desktops_in(value: OwnedVariant) -> Result<Vec<VirtualDesktop>, Error> {
    let shown = format!("{value:?}");
    Vec::<(u32, String, String)>::try_from(value)
        .map(|listed| {
            listed
                .into_iter()
                .map(|(position, id, name)| VirtualDesktop { position, id, name })
                .collect()
        })
        .map_err(|error| Error::Answer {
            answer: shown,
            reason: error.to_string(),
        })
}

/// The text a property's value is.
fn text_in(value: OwnedVariant) -> Result<String, Error> {
    let shown = format!("{value:?}");
    String::try_from(value).map_err(|error| Error::Answer {
        answer: shown,
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use zbus::Variant;

    use super::*;

    /// What KWin 6.6.7's manager answered in `cargo xtask session kwin`
    /// (2026-11-23), just started: one desktop, current.
    const ID: &str = "563e84c3-a479-4fbc-aa16-c0b53e7b26b8";

    #[test]
    fn the_desktops_are_read_with_their_positions_ids_and_names() {
        let answered =
            OwnedVariant::try_from(Variant::from(vec![(0_u32, ID, "Desktop 1")])).unwrap();
        assert_eq!(
            desktops_in(answered).unwrap(),
            [VirtualDesktop {
                position: 0,
                id: ID.to_owned(),
                name: "Desktop 1".to_owned()
            }]
        );
    }

    #[test]
    fn the_current_desktop_is_read_as_its_id() {
        let answered = OwnedVariant::try_from(Variant::from(ID)).unwrap();
        assert_eq!(text_in(answered).unwrap(), ID);
    }

    #[test]
    fn an_answer_that_is_not_desktops_is_an_error_showing_it() {
        let refusal = desktops_in(OwnedVariant::from(1_u32)).unwrap_err();
        assert!(matches!(refusal, Error::Answer { .. }), "{refusal:?}");
    }
}
