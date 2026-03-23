use squeez::strategies::grouping::group_files_by_dir;

#[test]
fn groups_many_files_in_same_dir() {
    let files: Vec<String> = (0..6).map(|i| format!("\tmodified:   src/components/Comp{}.tsx", i)).collect();
    let result = group_files_by_dir(files, 4);
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("src/components"));
    assert!(result[0].contains("6 modified"));
}

#[test]
fn keeps_files_below_threshold() {
    let files = vec!["modified:   src/a.ts".to_string(), "modified:   src/b.ts".to_string()];
    assert_eq!(group_files_by_dir(files, 4).len(), 2);
}

#[test]
fn mixes_grouped_and_ungrouped() {
    let mut files: Vec<String> = (0..5).map(|i| format!("modified:   src/components/C{}.tsx", i)).collect();
    files.push("modified:   README.md".to_string());
    let result = group_files_by_dir(files, 4);
    assert_eq!(result.len(), 2); // components/ grouped + README.md kept
}
