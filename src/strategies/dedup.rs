use std::collections::HashMap;

pub fn apply(lines: Vec<String>, threshold: usize) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for l in &lines { *counts.entry(l.as_str()).or_insert(0) += 1; }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result = Vec::new();
    for l in &lines {
        let count = counts[l.as_str()];
        if count >= threshold {
            if seen.insert(l.clone()) {
                result.push(format!("{}  [×{}]", l, count));
            }
        } else {
            result.push(l.clone());
        }
    }
    result
}
