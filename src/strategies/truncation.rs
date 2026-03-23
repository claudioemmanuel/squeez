pub enum Keep { Head, Tail }

pub fn apply(lines: Vec<String>, limit: usize, keep: Keep) -> Vec<String> {
    if lines.len() <= limit { return lines; }
    let dropped = lines.len() - limit;
    let notice = format!(
        "[... {} lines truncated — prefix command with --no-squeez to see full output]",
        dropped
    );
    match keep {
        Keep::Head => {
            let mut r: Vec<String> = lines.into_iter().take(limit).collect();
            r.push(notice);
            r
        }
        Keep::Tail => {
            let start = lines.len() - limit;
            let mut r = vec![notice];
            r.extend(lines.into_iter().skip(start));
            r
        }
    }
}
