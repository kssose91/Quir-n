//! Evaluator module for blame assignment and credit tracking.
//!
//! This module implements the Evaluator component from MASTER_PLAN phases 13-15:
//! - Runs: Group events by execution run
//! - Blame: Trace failures back to causal events
//! - Credit: Assign credit to patches that contributed to success

pub mod blame;
pub mod credit;
pub mod runs;

pub use blame::{BlameGraph, BlameResult};
pub use credit::{CreditAssigner, CreditResult};
pub use runs::{Run, RunTracker};
