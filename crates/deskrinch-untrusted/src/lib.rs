//! The shared rules for what an untrusted peer on the bus may hand over:
//! strings cleaned of control characters and bounded, pixel data validated
//! into plain RGBA, and themed icon names that cannot name a path.
//!
//! The notification service and the status-notifier host both take input
//! from applications they do not trust; the rules each applies to a string
//! or an image are these, in one place.
//!
//! # Example
//!
//! ```
//! use deskrinch_untrusted::{IconName, Image, text};
//!
//! assert_eq!(text::clean("a\u{7}b\r\nc", 64, text::Lines::Join), "ab c");
//! assert!(IconName::parse("../../etc/passwd").is_none());
//! assert!(Image::new(1, 1, vec![0, 0, 0, 255]).is_some());
//! ```

mod image;
pub mod text;

pub use image::{IconName, Image};
