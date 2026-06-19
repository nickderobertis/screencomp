//! Pure domain logic: no filesystem, network, process, or terminal access.
//!
//! Functions here operate on in-memory [`snapshot::Snapshot`] values and return
//! plain data, so they are deterministic and trivially unit-testable.

pub(crate) mod classify;
pub(crate) mod comment;
pub(crate) mod gallery;
pub(crate) mod index;
pub(crate) mod layout;
pub(crate) mod preflight;
pub(crate) mod scope;
pub(crate) mod snapshot;
pub(crate) mod toggle;
