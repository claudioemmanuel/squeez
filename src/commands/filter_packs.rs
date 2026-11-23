//! `squeez filters add|list|remove` — shareable filter-DSL rule packs.
//!
//! The shipped pack covers 41 commands and every other one waited on the
//! maintainer. A pack is a directory or git repository of `.ini` files in the
//! filter DSL; installing one copies its files to
//! `<squeez dir>/filters.d/<pack>/`, a layer between the shipped pack and the
//! user's own `filters.ini` (user and project rules still win).
//!
//! A pack is installed only if every rule carries at least one
//! `test_input`/`test_expect` pair and every test passes — the same gate the
//! shipped pack meets in CI. A rule that hides output must prove what it hides.

use std::path::{Path, PathBuf};

use crate::filter_dsl::{self, FilterDef};

pub fn packs_dir() -> PathBuf {
    crate::session::squeez_dir().join("filters.d")
}

/// Every installed pack's rules, packs in name order.
pub fn installed_defs() -> Vec<FilterDef> {
    installed_defs_in(&packs_dir())
}

pub fn installed_defs_in(dir: &Path) -> Vec<FilterDef> {
    let mut packs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    packs.sort();
    packs.iter().flat_map(|p| ini_files(p)).flat_map(|f| read_defs(&f)).collect()
}

fn read_defs(file: &Path) -> Vec<FilterDef> {
    std::fs::read_to_string(file).map(|s| filter_dsl::parse(&s)).unwrap_or_default()
}

/// `.ini` files at the top of `dir` or in its `filters/` subdirectory.
fn ini_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in [dir.to_path_buf(), dir.join("filters")] {
        if let Ok(rd) = std::fs::read_dir(&d) {
            out.extend(
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("ini")),
            );
        }
    }
    out.sort();
    out
}

/// Number of rules, or why the pack is refused.
pub fn validate(defs: &[FilterDef]) -> Result<usize, String> {
    if defs.is_empty() {
        return Err("no [filter \"…\"] rules found".into());
    }
    if let Some(d) = defs.iter().find(|d| d.tests.is_empty()) {
        return Err(format!("rule {:?} has no test_input/test_expect pair", d.name));
    }
    if let Some(d) = defs.iter().find(|d| d.name.trim().len() < 2) {
        return Err(format!("rule name {:?} is too short to match safely", d.name));
    }
    let failed: Vec<String> = filter_dsl::run_tests(defs)
        .into_iter()
        .filter(|r| !r.failed.is_empty())
        .map(|r| r.filter_name)
        .collect();
    if !failed.is_empty() {
        return Err(format!("failing tests in: {}", failed.join(", ")));
    }
    Ok(defs.len())
}

fn pack_name(source: &str) -> String {
    let base = source.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(source);
    let base = base.trim_end_matches(".git").trim_end_matches(".ini");
    base.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect()
}

fn is_remote(source: &str) -> bool {
    source.starts_with("https://") || source.starts_with("git@") || source.ends_with(".git")
}

/// Install `source` (a local dir, a local `.ini`, or a git URL) into `dir`.
pub fn add_in(dir: &Path, source: &str) -> Result<String, String> {
    let name = pack_name(source);
    if name.is_empty() {
        return Err(format!("cannot name a pack from {source:?}"));
    }
    let staging = std::env::temp_dir().join(format!("squeez-pack-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let files: Vec<PathBuf> = if is_remote(source) {
        let ok = crate::spawn::helper("git")
            .args(["clone", "--depth", "1", "--quiet", source])
            .arg(&staging)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return Err(format!("git clone failed for {source}"));
        }
        ini_files(&staging)
    } else {
        let p = Path::new(source);
        if p.is_dir() {
            ini_files(p)
        } else if p.is_file() {
            vec![p.to_path_buf()]
        } else {
            return Err(format!("{source} is neither a directory, an .ini file nor a git URL"));
        }
    };
    let defs: Vec<FilterDef> = files.iter().flat_map(|f| read_defs(f)).collect();
    let result = validate(&defs).and_then(|n| {
        let target = dir.join(&name);
        let existed = target.exists();
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        for f in &files {
            let dest = target.join(f.file_name().unwrap_or_default());
            std::fs::copy(f, dest).map_err(|e| e.to_string())?;
        }
        Ok(format!(
            "{} pack {name:?}: {n} rule(s), all tests passing",
            if existed { "updated" } else { "installed" }
        ))
    });
    let _ = std::fs::remove_dir_all(&staging);
    result
}

pub fn list_in(dir: &Path) -> Vec<(String, Vec<String>)> {
    let mut packs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    packs.sort();
    packs
        .iter()
        .map(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let rules = ini_files(p).iter().flat_map(|f| read_defs(f)).map(|d| d.name).collect();
            (name, rules)
        })
        .collect()
}

pub fn remove_in(dir: &Path, name: &str) -> Result<(), String> {
    let clean = pack_name(name);
    let target = dir.join(&clean);
    if clean.is_empty() || !target.is_dir() {
        return Err(format!("no installed pack named {name:?}"));
    }
    std::fs::remove_dir_all(target).map_err(|e| e.to_string())
}

/// CLI: `squeez filters add <dir|file.ini|git-url> | list | remove <name>`.
pub fn run(args: &[String]) -> i32 {
    let dir = packs_dir();
    match args.first().map(String::as_str) {
        Some("add") if args.len() > 1 => match add_in(&dir, &args[1]) {
            Ok(msg) => {
                println!("squeez filters: {msg}");
                0
            }
            Err(e) => {
                eprintln!("squeez filters: not installed — {e}");
                1
            }
        },
        Some("list") => {
            let packs = list_in(&dir);
            if packs.is_empty() {
                println!("squeez filters: no packs installed ({})", dir.display());
            }
            for (name, rules) in packs {
                println!("{name}  ({} rule(s)): {}", rules.len(), rules.join(", "));
            }
            0
        }
        Some("remove") if args.len() > 1 => match remove_in(&dir, &args[1]) {
            Ok(()) => {
                println!("squeez filters: removed {:?}", args[1]);
                0
            }
            Err(e) => {
                eprintln!("squeez filters: {e}");
                1
            }
        },
        _ => {
            eprintln!("usage: squeez filters add <dir|file.ini|git-url> | list | remove <name>");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "[filter \"mix compile\"]\nstrip_lines_containing = \"Compiling \"\n\
                        test_input = \"Compiling 3 files\\nerror: boom\"\ntest_expect = \"error: boom\"\n";

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("squeez-packs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn local_pack_installs_lists_layers_and_removes() {
        let root = tmp("ok");
        let src = root.join("elixir-pack");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("mix.ini"), GOOD).unwrap();
        let dir = root.join("filters.d");
        let msg = add_in(&dir, src.to_str().unwrap()).unwrap();
        assert!(msg.contains("installed pack \"elixir-pack\": 1 rule(s)"), "{msg}");
        assert_eq!(list_in(&dir), vec![("elixir-pack".to_string(), vec!["mix compile".to_string()])]);
        let defs = installed_defs_in(&dir);
        assert!(filter_dsl::find_in(&defs, "mix compile --force").is_some());
        assert!(add_in(&dir, src.to_str().unwrap()).unwrap().starts_with("updated"));
        remove_in(&dir, "elixir-pack").unwrap();
        assert!(list_in(&dir).is_empty());
        assert!(remove_in(&dir, "../etc").is_err());
    }

    #[test]
    fn untested_or_failing_rules_are_refused() {
        let root = tmp("bad");
        let dir = root.join("filters.d");
        let untested = root.join("untested.ini");
        std::fs::write(&untested, "[filter \"foo \"]\nstrip_lines_containing = \"x\"\n").unwrap();
        assert!(add_in(&dir, untested.to_str().unwrap()).unwrap_err().contains("no test_input"));
        let failing = root.join("failing.ini");
        std::fs::write(&failing, GOOD.replace("test_expect = \"error: boom\"", "test_expect = \"nope\"")).unwrap();
        assert!(add_in(&dir, failing.to_str().unwrap()).unwrap_err().contains("failing tests"));
        assert!(!dir.join("failing").exists(), "nothing installed on refusal");
        assert_eq!(pack_name("https://github.com/acme/squeez-elixir.git"), "squeez-elixir");
    }
}
