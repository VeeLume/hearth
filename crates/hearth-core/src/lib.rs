//! Hearth domain types and logic. Pure — no transport, no storage, no I/O.
//!
//! Types here are linked by both desktop (`hearth-app`) and server
//! (`hearth-server`, v2+) so domain stays single-source-of-truth.

pub mod missions;
pub mod profile;
pub mod sc_data;
pub mod types;

pub use missions::missions_by_blueprint;
pub use profile::{ProfileError, ProfileInfo};
pub use types::{
    Account, BpPoolReward, BpRewardEntry, BpView, DifficultyView, EncounterView, FactionView,
    Ingredient, IngredientKind, InventoryLocationKind, InventoryStack, ItemRewardView,
    MissionCategoryView, MissionRef, MissionView, OwnedBlueprint, PayoutView, PlaceView, Platform,
    Recipe, RecordId, RegionView, RepRequirementView, RepRewardView, ScripRewardView, ShipSlotView,
    WaveView, WishIntent, WishlistEntry,
};
