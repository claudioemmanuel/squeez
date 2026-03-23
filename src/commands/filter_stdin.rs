use crate::config::Config;
use crate::filter;
use std::io::{self, Read};

pub fn run(hint: &str) -> i32 {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("squeez filter: failed to read stdin");
        return 1;
    }
    let config = Config::load();
    let lines: Vec<String> = input.lines().map(String::from).collect();
    let compressed = filter::compress(hint, lines, &config);
    if !compressed.is_empty() {
        println!("{}", compressed.join("\n"));
    }
    0
}
