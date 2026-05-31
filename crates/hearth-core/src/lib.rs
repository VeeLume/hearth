//! Hearth domain types and logic. Pure — no transport, no storage, no I/O.
//!
//! Types here are linked by both desktop (`hearth-app`) and server
//! (`hearth-server`, v2+) so domain stays single-source-of-truth.

pub mod profile;
pub mod sc_data;
pub mod types;

pub use profile::{ProfileError, ProfileInfo};
pub use types::{
    Account, BpPoolReward, BpRewardEntry, BpView, Ingredient, ItemRewardView, MissionCompletion,
    MissionView, OwnedBlueprint, Platform, Recipe, RecordId, RepRewardView, ScripRewardView,
    WishIntent, WishlistEntry,
};
