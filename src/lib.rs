//! TreeSpec, read-only inventory, structure snapshots, and reviewed migrations.

pub mod error;
pub mod frontend;
mod fsops;
pub mod git;
pub mod llm;
mod metadata;
pub mod migration;
pub mod output;
pub mod scan;
pub mod storage;
pub mod sync;
pub mod tree;
pub mod workspace;
