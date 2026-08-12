//! `squeez prune` — on-demand cleanup of the blob store and session memory.
//!
//! Both cleanups already run opportunistically elsewhere (blob TTL pruning on
//! every `wrap` call that stashes a new output, `summaries.jsonl` pruning on
//! every session init), so this command doesn't do anything those paths
//! don't — it just gives users (and the `run squeez prune` hints in
//! `squeez_search_history`/`squeez_file_history`) something to actually run
//! on demand instead of falling through to the usage banner.

use crate::config::Config;
use crate::{context, memory, session};

/// CLI entry point: prune expired blobs and old session summaries, print a
/// one-line report of what was freed.
pub fn run() -> i32 {
    let cfg = Config::load();
    let (blobs_removed, bytes_freed) =
        context::retrieve::prune(cfg.retrieve_ttl_days.saturating_mul(86_400));
    memory::prune_old(&session::memory_dir(), cfg.memory_retention_days);
    println!(
        "squeez prune: removed {} expired blob(s), freed {} bytes; pruned session summaries older than {} day(s)",
        blobs_removed, bytes_freed, cfg.memory_retention_days
    );
    0
}
