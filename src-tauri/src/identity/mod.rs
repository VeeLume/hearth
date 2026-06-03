//! RSI identity domain.
//!
//! Two cooperating pieces, kept together because they share the public
//! citizen-profile scrape as their single source of immutable identity:
//!
//! - [`fetch`] — the HTTP fetch + parse of `robertsspaceindustries.com`'s
//!   public citizen page (I/O lives here, not in `hearth-core`, to keep the
//!   domain crate network-free).
//! - [`rename`] — startup handle-rename detection, which uses the scrape's
//!   immutable citizen-record anchor to decide whether a changed launcher
//!   handle is a rename of an existing account or a separate one.

pub mod fetch;
pub mod rename;

pub use fetch::{IdentityError, fetch_profile};
pub(crate) use rename::spawn_rename_check;
