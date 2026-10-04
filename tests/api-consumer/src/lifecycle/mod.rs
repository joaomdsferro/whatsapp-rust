//! Standalone public lifecycle fixture, without workspace features or flags.
pub mod admission;
#[cfg(all(test, feature = "native", not(target_arch = "wasm32")))]
#[path = "../../../lifecycle_outcomes.rs"]
mod contract;
