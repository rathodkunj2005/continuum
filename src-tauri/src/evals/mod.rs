//! Eval harnesses for FNDR's memory pipeline.
//!
//! Each submodule covers one piece of the system; tests are typically `#[ignore]`
//! because they require a local LLM model to be loaded. Run with:
//! `cargo test --lib -p fndr eval_ -- --ignored --nocapture`

pub mod memory_quality;
