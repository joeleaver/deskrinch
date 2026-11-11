//! The shell's one look, on rinch, dark or light as the configuration and
//! the desktop say.
//!
//! - [`look::props`]: the look itself, the one set of rinch theme
//!   properties every component uses. Components style themselves with the
//!   variables it makes and hard-code no colour or metric.
//! - [`resolve`]: dark or light from the `[general]` `color-scheme` setting
//!   (`deskrinch_settings::general`) and the desktop's preference from the
//!   settings portal (`deskrinch_appearance`). Pure.
//! - [`Theme`]: both followed for one component. A component gets its theme
//!   in one call in its root component, [`Theme::apply`], after making its
//!   window with [`Theme::initial`].
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_appearance::Bus;
//! use deskrinch_component::Configuration;
//! use deskrinch_theme::Theme;
//! use deskrinch_xdg::BaseDirs;
//! use rinch::prelude::*;
//!
//! let configuration = Configuration::start(&BaseDirs::from_process());
//! let theme = Theme::start(&configuration, Bus::Session);
//! let first = theme.initial();
//! // App::new(..).theme(first), and in the root component:
//! # let _ = |_scope: &mut RenderScope| {
//! let scheme = theme.apply();
//! # };
//! ```

pub mod look;
mod scheme;
mod theme;

pub use scheme::{Scheme, resolve};
pub use theme::Theme;
