//! Diff on re-read: a file read again after a small change is sent as the
//! changed region against the earlier read, not in full.
//!
//! Agents edit three lines of a 900-line file and Read it again; the second
//! Read costs the whole file although the model already holds the first one.
//! When the earlier full read is still in the model's context (its call is
//! above the dedup floor), the changed region is cut out by trimming the
//! common prefix and suffix, and sent as one unified hunk. A change spread
//! over the file makes the region large, and the full output is kept.

/// Smallest file worth diffing; below this the full read is cheap anyway.
pub const MIN_LINES: usize = 40;

/// The changed region may be at most this share of the new file's lines.
const MAX_REGION_PCT: usize = 30;

/// Context lines kept around the changed region.
const CONTEXT: usize = 2;

#[derive(Debug, PartialEq)]
pub struct RegionDiff {
    /// 1-based first line of the hunk in the old and new file.
    pub old_start: usize,
    pub new_start: usize,
    pub old_len: usize,
    pub new_len: usize,
    /// Unified hunk, `@@ … @@` header included.
    pub hunk: String,
}

/// One-hunk diff of `old` → `new`, or `None` when they are equal, too short,
/// or the changed region is too large to be worth it.
pub fn region_diff(old: &str, new: &str) -> Option<RegionDiff> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    if b.len() < MIN_LINES || a == b {
        return None;
    }
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    let changed = (b.len() - prefix - suffix).max(a.len() - prefix - suffix);
    if changed * 100 > b.len() * MAX_REGION_PCT {
        return None;
    }
    let start = prefix.saturating_sub(CONTEXT);
    let a_end = (a.len() - suffix + CONTEXT).min(a.len());
    let b_end = (b.len() - suffix + CONTEXT).min(b.len());
    let mut hunk = format!(
        "@@ -{},{} +{},{} @@\n",
        start + 1,
        a_end - start,
        start + 1,
        b_end - start
    );
    for l in &a[start..prefix] {
        hunk.push_str(&format!(" {l}\n"));
    }
    for l in &a[prefix..a.len() - suffix] {
        hunk.push_str(&format!("-{l}\n"));
    }
    for l in &b[prefix..b.len() - suffix] {
        hunk.push_str(&format!("+{l}\n"));
    }
    for l in &b[b.len() - suffix..b_end] {
        hunk.push_str(&format!(" {l}\n"));
    }
    Some(RegionDiff {
        old_start: start + 1,
        new_start: start + 1,
        old_len: a_end - start,
        new_len: b_end - start,
        hunk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn small_edit_becomes_one_hunk() {
        let old = file(100);
        let mut new = old.clone();
        new[49] = "line 50 changed".into();
        new.insert(50, "inserted".into());
        let d = region_diff(&old.join("\n"), &new.join("\n")).unwrap();
        assert_eq!((d.old_start, d.old_len, d.new_len), (48, 5, 6));
        assert!(d.hunk.starts_with("@@ -48,5 +48,6 @@\n line 48\n line 49\n-line 50\n+line 50 changed\n+inserted\n line 51\n"), "{}", d.hunk);
    }

    #[test]
    fn append_and_delete_at_edges() {
        let old = file(60);
        let mut grown = old.clone();
        grown.push("tail".into());
        let d = region_diff(&old.join("\n"), &grown.join("\n")).unwrap();
        assert!(d.hunk.ends_with("+tail\n"), "{}", d.hunk);
        let shrunk = &old[1..];
        let d = region_diff(&old.join("\n"), &shrunk.join("\n")).unwrap();
        assert!(d.hunk.starts_with("@@ -1,3 +1,2 @@\n-line 1\n"), "{}", d.hunk);
    }

    #[test]
    fn equal_short_or_sweeping_changes_are_kept_whole() {
        let old = file(100);
        assert!(region_diff(&old.join("\n"), &old.join("\n")).is_none());
        assert!(region_diff("a\nb", "a\nc").is_none(), "short files");
        let mut new = old.clone();
        new[0] = "first".into();
        new[99] = "last".into(); // region spans the whole file
        assert!(region_diff(&old.join("\n"), &new.join("\n")).is_none());
    }
}
