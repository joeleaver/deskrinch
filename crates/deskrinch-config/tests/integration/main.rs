//! The crate against real files in temporary directories: loading, and the
//! watcher's reloads. No test reads or writes the user's own configuration.

#[cfg(test)]
mod load;
#[cfg(test)]
mod support;
#[cfg(test)]
mod watch;
