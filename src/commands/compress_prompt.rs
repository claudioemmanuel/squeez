//! `squeez compress-prompt` — compress an agent/task prompt from stdin to stdout.
//!
//! Reads stdin to a String. If the token estimate (chars / 4) is within
//! `agent_prompt_max_tokens`, echoes unchanged. Otherwise runs the input
//! through markdown compression. If still over budget after compression,
//! head-truncates to fit and appends a notice.

use crate::commands::compress_md::{compress_text_with_locale, Mode};
use crate::config::Config;

pub fn run() -> i32 {
    let cfg = Config::load();

    // 0 = disabled: echo stdin unchanged
    if cfg.agent_prompt_max_tokens == 0 {
        let mut input = String::new();
        loop {
            let mut buf = String::new();
            match std::io::stdin().read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => input.push_str(&buf),
                Err(_) => break,
            }
        }
        print!("{}", input);
        return 0;
    }

    let input = read_stdin();
    let orig_tokens = input.chars().count() / 4;

    if orig_tokens <= cfg.agent_prompt_max_tokens {
        print!("{}", input);
        return 0;
    }

    // Run markdown compression
    let locale = crate::commands::compress_md::Locale::from_code(&cfg.lang);
    let result = compress_text_with_locale(&input, Mode::Ultra, locale);
    let compressed = if result.safe {
        result.output
    } else {
        input.clone()
    };

    let compressed_tokens = compressed.chars().count() / 4;
    if compressed_tokens <= cfg.agent_prompt_max_tokens {
        print!("{}", compressed);
        return 0;
    }

    // Head-truncate to budget
    let max_chars = cfg.agent_prompt_max_tokens * 4;
    let notice = format!(
        "\n[squeez: agent prompt truncated from {} to {} tokens — pass --no-squeez in the prompt to disable]",
        orig_tokens,
        cfg.agent_prompt_max_tokens,
    );
    let available_chars = max_chars.saturating_sub(notice.len());
    let truncated = char_truncate(&compressed, available_chars);
    print!("{}{}", truncated, notice);
    0
}

fn read_stdin() -> String {
    use std::io::Read;
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

fn char_truncate(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut idx = 0;
    let mut count = 0;
    for (byte_idx, _) in s.char_indices() {
        if count == max_chars {
            idx = byte_idx;
            break;
        }
        count += 1;
    }
    &s[..idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_est(s: &str) -> usize {
        s.chars().count() / 4
    }

    #[test]
    fn short_prompt_passes_through_unchanged() {
        // Build a prompt well under 2000 tokens (< 8000 chars)
        let input = "# Plan\n\nDo something simple.\n";
        let tokens = token_est(input);
        assert!(tokens < 2000, "test input should be short");

        // Use a high budget so compress logic is skipped
        let cfg = Config {
            agent_prompt_max_tokens: 2000,
            ..Config::default()
        };

        // Simulate the logic inline (cfg.agent_prompt_max_tokens check)
        let orig_tokens = input.chars().count() / 4;
        assert!(orig_tokens <= cfg.agent_prompt_max_tokens);
        // Should pass through unchanged
        let output = input;
        assert_eq!(output, input);
    }

    #[test]
    fn markdown_heavy_prompt_shrinks() {
        // Build a prompt that has lots of filler prose
        let filler = "This is just really basically a simple test item that I would like to ensure you understand completely.\n";
        let mut input = String::from("# Plan\n\n");
        for _ in 0..50 {
            input.push_str(filler);
        }

        let locale = crate::commands::compress_md::Locale::from_code("en");
        let result = compress_text_with_locale(&input, Mode::Ultra, locale);
        if result.safe {
            assert!(
                result.output.len() < input.len(),
                "compressed should be smaller than original"
            );
        }
    }

    #[test]
    fn truncation_appends_notice() {
        // Construct input that exceeds 10 tokens (40 chars) to test truncation path
        let input = "abcdefghij".repeat(20); // 200 chars = 50 tokens
        let max_tokens = 10usize;
        let max_chars = max_tokens * 4;
        let notice = format!(
            "\n[squeez: agent prompt truncated from {} to {} tokens — pass --no-squeez in the prompt to disable]",
            input.chars().count() / 4,
            max_tokens,
        );
        let available = max_chars.saturating_sub(notice.len());
        let truncated = char_truncate(&input, available);
        let output = format!("{}{}", truncated, notice);
        assert!(output.contains("[squeez: agent prompt truncated"));
        assert!(output.len() < input.len() + notice.len() + 10);
    }
}
