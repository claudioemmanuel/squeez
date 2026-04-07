pub mod cache;
pub mod hash;
pub mod intensity;
pub mod redundancy;
pub mod summarize;

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
    let ctx = if cfg.context_cache_enabled {
        SessionContext::load(sessions_dir)
    } else {
        SessionContext::default()
    };
    let level = intensity::derive(used_tokens, cfg);
    let scaled = intensity::scale(cfg, level);
    (ctx, level, scaled)
}
