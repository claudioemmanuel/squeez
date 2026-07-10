pub mod cache;
pub mod factsheet;
pub mod hash;
pub mod intensity;
pub mod redundancy;
pub mod retrieve;
pub mod summarize;
pub mod transcript;

pub use cache::SessionContext;
pub use intensity::Intensity;

use crate::config::Config;
use std::path::Path;

/// Pre-pass: load context, derive intensity from current usage, return a
/// scaled clone of the user's config to use for this single call.
pub fn pre_pass(
    cfg: &Config,
    sessions_dir: &Path,
    used_tokens: u64,
) -> (SessionContext, Intensity, Config) {
    let mut ctx = if cfg.context_cache_enabled {
        SessionContext::load(sessions_dir)
    } else {
        SessionContext::default()
    };
    // Phase 5: apply configurable tunables so all methods use user's values.
    ctx.init_tunables_from_config(cfg);
    // Real-context tracking (CF-1): squeez's own accounting only counts bytes
    // it processed. The transcript-measured context (when observed via hooks)
    // is authoritative — squeez's own counters are cumulative monotonic sums of
    // tool I/O, not context occupancy, so on long sessions they over-report and
    // would pin intensity at Ultra. Use the measured value when present; fall
    // back to the byte counter only when no transcript has been seen.
    let effective_used = if ctx.real_ctx_tokens > 0 {
        ctx.real_ctx_tokens
    } else {
        used_tokens
    };
    let level = intensity::derive_with(effective_used, cfg, ctx.real_ctx_window);
    let scaled = intensity::scale(cfg, level);
    (ctx, level, scaled)
}
