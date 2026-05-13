//! Adaptive activity timeline: content-derived action classification.
//!
//! App name is **never** used as the primary classifier signal (see unit tests).

mod classify;
mod classify_rules;

pub use classify::{classify_action_class, ActionClass};
