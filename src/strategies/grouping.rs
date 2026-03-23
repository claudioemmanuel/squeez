use std::collections::HashMap;

pub fn group_files_by_dir(lines: Vec<String>, threshold: usize) -> Vec<String> {
    let parsed: Vec<(String, String, String)> = lines.iter().map(|l| {
        let (prefix, path) = split_status_line(l);
        let dir = parent_dir(path);
        (prefix.to_string(), dir, l.clone())
    }).collect();

    let mut counts: HashMap<String, usize> = HashMap::new();
    for (prefix, dir, _) in &parsed {
        *counts.entry(format!("{}{}", prefix, dir)).or_insert(0) += 1;
    }

    let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result = Vec::new();
    for (prefix, dir, original) in &parsed {
        let key = format!("{}{}", prefix, dir);
        let count = counts[&key];
        if count >= threshold {
            if emitted.insert(key.clone()) {
                result.push(format!("{}{}/  {} modified  [squeez grouped]", prefix, dir, count));
            }
        } else {
            result.push(original.clone());
        }
    }
    result
}

fn split_status_line(line: &str) -> (&str, &str) {
    let t = line.trim_start();
    if let Some(i) = t.find(':') {
        (&t[..=i], t[i+1..].trim())
    } else { ("", line) }
}

fn parent_dir(path: &str) -> String {
    path.rfind('/').map(|i| path[..i].to_string()).unwrap_or_else(|| ".".to_string())
}
