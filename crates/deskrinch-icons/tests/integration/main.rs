//! The crate against fixture themes in temporary directories. Nothing here
//! reads the real themes, or writes outside the temporary directory.

#[cfg(test)]
mod load;
#[cfg(test)]
mod support;
#[cfg(test)]
mod watch;
