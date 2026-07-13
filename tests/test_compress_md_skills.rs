// Layer 1 — skill-body compression. Verifies the markdown compressor preserves
// a leading YAML front-matter block verbatim (skill `name:`/`description:` drive
// identity + activation matching and must not be abbreviated) while still
// compressing the prose body. Idempotence: a second pass must stay safe and not
// re-touch the protected front-matter.

use squeez::commands::compress_md::{compress_text, Mode};

const FRONT_MATTER: &str = "---\nname: DEBUG_error-detective\ndescription: \"Use this agent when you need to diagnose why errors are occurring in your system without delay.\"\ntools: Read, Write, Edit, Bash, Glob, Grep\nmodel: sonnet\n---\n";

#[test]
fn front_matter_preserved_verbatim() {
    let input = format!(
        "{}\nConfigure the function with these parameters because of the documentation.\n",
        FRONT_MATTER
    );
    let r = compress_text(&input, Mode::Ultra);
    assert!(r.safe, "compression must pass integrity check");

    // Every front-matter line survives byte-for-byte.
    assert!(r.output.contains("name: DEBUG_error-detective"));
    assert!(r.output.contains(
        "description: \"Use this agent when you need to diagnose why errors are occurring in your system without delay.\""
    ));
    assert!(r.output.contains("tools: Read, Write, Edit, Bash, Glob, Grep"));
    assert!(r.output.contains("model: sonnet"));

    // The front-matter `with`/`because`/`documentation` must NOT be abbreviated.
    let fm_end = r.output.find("---\n\n").or_else(|| r.output.find("---\n")).unwrap();
    let (fm, _body) = r.output.split_at(fm_end);
    assert!(!fm.contains("w/"), "front-matter must not be abbreviated: {}", fm);
    assert!(!fm.contains("b/c"), "front-matter must not be abbreviated: {}", fm);
}

#[test]
fn prose_body_still_compressed() {
    // E6: EN word-substitution (function->fn, with->w/, because->b/c) was
    // pruned from ultra_subs -- measured COSTS/NEUTRAL against real
    // tokenizers. The body still gets the validated mechanism: filler and
    // article dropping.
    let input = format!(
        "{}\nThis is just really the configuration you need.\n",
        FRONT_MATTER
    );
    let r = compress_text(&input, Mode::Ultra);
    assert!(!r.output.to_lowercase().contains("really"), "body should drop filler 'really'");
    assert!(!r.output.to_lowercase().contains("just"), "body should drop filler 'just'");
    assert!(r.output.contains("configuration"), "body content survives");
    assert!(r.stats.new_bytes < r.stats.orig_bytes, "must reduce overall size");
}

#[test]
fn idempotent_second_pass_safe() {
    let input = format!(
        "{}\nThe system should correlate errors across services to find the root cause.\n",
        FRONT_MATTER
    );
    let first = compress_text(&input, Mode::Ultra);
    assert!(first.safe);
    let second = compress_text(&first.output, Mode::Ultra);
    assert!(second.safe, "second pass must remain safe");
    // Front-matter still intact after a re-run.
    assert!(second.output.contains("name: DEBUG_error-detective"));
    assert!(second.output.contains("model: sonnet"));
}

#[test]
fn no_front_matter_file_unaffected() {
    // A plain prose file (no leading `---`) compresses exactly as before.
    let input = "This is just really the configuration you need.\n";
    let r = compress_text(input, Mode::Ultra);
    assert!(r.safe);
    assert!(!r.output.to_lowercase().contains("really"));
    assert!(!r.output.to_lowercase().contains("just"));
}
