//! KWin's effects, through its own interface (`org.kde.kwin.Effects` on
//! `/Effects`): which are active, and whether one is loaded. KWin loads its
//! effects after it has started, so a test that needs one waits for it.

use zbus::OwnedVariant;

use crate::names::{EFFECTS, KWIN};
use crate::{Error, Kwin};

impl Kwin {
    /// The effects KWin has active now (its `activeEffects`), by name.
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin cannot be asked; [`Error::Answer`] when its
    /// answer is not a list of names.
    pub fn active_effects(&self) -> Result<Vec<String>, Error> {
        let value = zbus::block_on(async {
            let reply = self
                .connect()
                .await?
                .call_method(
                    Some(KWIN),
                    EFFECTS.0,
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &(EFFECTS.1, "activeEffects"),
                )
                .await?;
            Ok::<_, Error>(reply.body().deserialize::<OwnedVariant>()?)
        })?;
        names_in(value)
    }

    /// Whether KWin has loaded the effect `name` (its `isEffectLoaded`).
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin cannot be asked.
    pub fn effect_loaded(&self, name: &str) -> Result<bool, Error> {
        zbus::block_on(async {
            let reply = self
                .connect()
                .await?
                .call_method(
                    Some(KWIN),
                    EFFECTS.0,
                    Some(EFFECTS.1),
                    "isEffectLoaded",
                    &(name,),
                )
                .await?;
            Ok(reply.body().deserialize::<bool>()?)
        })
    }
}

/// The names a property's value lists.
fn names_in(value: OwnedVariant) -> Result<Vec<String>, Error> {
    let shown = format!("{value:?}");
    Vec::<String>::try_from(value).map_err(|error| Error::Answer {
        answer: shown,
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use zbus::Variant;

    use super::*;

    /// KWin 6.6.7's `activeEffects` in `cargo xtask session kwin`
    /// (2026-11-23), five seconds after it started: `["blur"]`.
    #[test]
    fn the_active_effects_are_read_as_names() {
        let answered = OwnedVariant::try_from(Variant::from(vec!["blur"])).unwrap();
        assert_eq!(names_in(answered).unwrap(), ["blur"]);
    }

    #[test]
    fn an_answer_that_is_not_names_is_an_error_showing_it() {
        let refusal = names_in(OwnedVariant::from(7_u32)).unwrap_err();
        assert!(
            matches!(&refusal, Error::Answer { answer, .. } if answer.contains('7')),
            "{refusal:?}"
        );
    }
}
