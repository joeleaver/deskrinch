//! [`KwinActions`]: what a test may change in KWin, through a handle of its
//! own, so that a function's signature says whether it can change the
//! session: one given a [`Kwin`] only reads.
//!
//! Its makers are [`Kwin`]'s, on the same bus proof
//! ([`deskrinch_test_dbus::OurBus`]): run directly, the actions would lay
//! out, switch, add and remove the desktops of the owner's KWin, moving its
//! windows, and invoke its shortcuts.

use zbus::Variant;
use zbus::zvariant::DynamicType;

use crate::desktops::MANAGER;
use crate::{Error, Kwin, VirtualDesktop};

/// KWin on a bus of ours, for changing it.
#[derive(Debug, Clone)]
pub struct KwinActions {
    kwin: Kwin,
}

impl KwinActions {
    /// KWin in the isolated session this process runs in, as
    /// [`Kwin::here`].
    ///
    /// # Errors
    ///
    /// As [`Kwin::here`]. Nothing is connected to.
    pub fn here() -> Result<Self, Error> {
        Kwin::here().map(|kwin| Self { kwin })
    }

    /// KWin on the bus at `address`, as [`Kwin::on`].
    ///
    /// # Errors
    ///
    /// As [`Kwin::on`]. Nothing is connected to.
    pub fn on(address: &str) -> Result<Self, Error> {
        Kwin::on(address).map(|kwin| Self { kwin })
    }

    /// The same KWin, for reading.
    #[must_use]
    pub fn reads(&self) -> &Kwin {
        &self.kwin
    }

    /// Makes the desktop named `name` current, as KWin's own interface
    /// does.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchDesktop`] when KWin has none of that name;
    /// [`Error::Bus`] when it refuses.
    pub fn set_current_virtual_desktop(&self, name: &str) -> Result<(), Error> {
        let id = self.id_of(name)?;
        self.set("current", Variant::from(id.as_str()))
    }

    /// Lays the desktops out in `rows` rows.
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin refuses.
    pub fn set_virtual_desktop_rows(&self, rows: u32) -> Result<(), Error> {
        self.set("rows", Variant::from(rows))
    }

    /// Adds a desktop named `name` at the end.
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin refuses; [`Error::Answer`] when it has more
    /// desktops than a `u32` counts.
    pub fn create_virtual_desktop(&self, name: &str) -> Result<(), Error> {
        let count = self.kwin.virtual_desktops()?.len();
        let position = u32::try_from(count).map_err(|error| Error::Answer {
            answer: count.to_string(),
            reason: error.to_string(),
        })?;
        self.call(MANAGER.0, MANAGER.1, "createDesktop", &(position, name))
    }

    /// Removes the desktop named `name`.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchDesktop`] when KWin has none of that name;
    /// [`Error::Bus`] when it refuses.
    pub fn remove_virtual_desktop(&self, name: &str) -> Result<(), Error> {
        let id = self.id_of(name)?;
        self.call(MANAGER.0, MANAGER.1, "removeDesktop", &(id,))
    }

    /// At least `count` desktops, the first current, in one row; the first
    /// desktop's name.
    ///
    /// # Errors
    ///
    /// As the actions it takes; [`Error::NoSuchDesktop`] when KWin lists no
    /// desktop.
    pub fn set_up_virtual_desktops(&self, count: usize) -> Result<String, Error> {
        loop {
            let listed = self.kwin.virtual_desktops()?.len();
            if listed >= count {
                break;
            }
            self.create_virtual_desktop(&format!("Desktop {}", listed + 1))?;
        }
        self.set_virtual_desktop_rows(1)?;
        let first = self
            .kwin
            .virtual_desktops()?
            .into_iter()
            .min_by_key(|desktop| desktop.position)
            .map(|desktop| desktop.name)
            .ok_or_else(|| Error::NoSuchDesktop {
                name: "(any)".to_owned(),
            })?;
        self.set_current_virtual_desktop(&first)?;
        Ok(first)
    }

    /// Invokes KWin's global shortcut `name` (`Overview`, say) through its
    /// shortcut service, which KWin itself owns
    /// ([`Kwin::kwin_shortcut_names`]).
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when the service refuses.
    pub fn invoke_kwin_shortcut(&self, name: &str) -> Result<(), Error> {
        zbus::block_on(async {
            self.kwin
                .connect()
                .await?
                .call_method(
                    Some("org.kde.kglobalaccel"),
                    "/component/kwin",
                    Some("org.kde.kglobalaccel.Component"),
                    "invokeShortcut",
                    &(name,),
                )
                .await?;
            Ok(())
        })
    }

    /// The id of the desktop named `name`.
    fn id_of(&self, name: &str) -> Result<String, Error> {
        self.kwin
            .virtual_desktops()?
            .into_iter()
            .find(|desktop| desktop.name == name)
            .map(|VirtualDesktop { id, .. }| id)
            .ok_or_else(|| Error::NoSuchDesktop {
                name: name.to_owned(),
            })
    }

    /// Sets the manager's property `name` to `value`.
    fn set(&self, name: &str, value: Variant<'_>) -> Result<(), Error> {
        self.call(
            MANAGER.0,
            "org.freedesktop.DBus.Properties",
            "Set",
            &(MANAGER.1, name, value),
        )
    }

    /// Calls KWin's `method` of `interface` on `path` with `body`.
    fn call<B>(&self, path: &str, interface: &str, method: &str, body: &B) -> Result<(), Error>
    where
        B: serde::Serialize + DynamicType,
    {
        zbus::block_on(async {
            self.kwin
                .connect()
                .await?
                .call_method(Some("org.kde.KWin"), path, Some(interface), method, body)
                .await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
