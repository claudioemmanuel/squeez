//! i18n integration tests for compress_md.

use squeez::commands::compress_md::{compress_text, compress_text_with_locale, Locale, Mode};
use squeez::config::Config;

// ── Unit: Locale resolution ────────────────────────────────────────────────

#[test]
fn locale_from_code_aliases() {
    for code in &["pt", "pt-BR", "pt_BR", "pt-br"] {
        assert_eq!(
            Locale::from_code(code).code, "pt-BR",
            "alias '{}' should resolve to pt-BR", code
        );
    }
    for code in &["en", "", "xx", "fr", "de", "ja"] {
        assert_eq!(
            Locale::from_code(code).code, "en",
            "unknown code '{}' should fall back to en", code
        );
    }
}

#[test]
fn config_lang_default_en() {
    assert_eq!(Config::default().lang, "en");
}

#[test]
fn config_lang_parsed() {
    let c = Config::from_str("lang=pt\n");
    assert_eq!(c.lang, "pt");
    let c2 = Config::from_str("lang = pt-BR\n");
    assert_eq!(c2.lang, "pt-BR");
    let c3 = Config::from_str("lang = en\n");
    assert_eq!(c3.lang, "en");
}

// ── Unit: Unicode-correct helpers ─────────────────────────────────────────

#[test]
fn is_clean_word_accepts_accented_via_behavior() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale("apenas um teste\n", Mode::Full, pt);
    assert!(!r.output.contains("apenas"), "filler 'apenas' must be dropped");
    assert!(r.output.contains("teste"));
}

#[test]
fn replace_word_boundary_unicode_correct() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "a função e o funcionário trabalham juntos\n",
        Mode::Ultra,
        pt,
    );
    assert!(r.safe);
    assert!(r.output.contains("fn"), "'função' must be abbreviated to 'fn'");
    assert!(
        r.output.contains("funcionário"),
        "'funcionário' must not be corrupted"
    );
    assert!(
        !r.output.contains("fnário"),
        "partial word match 'fnário' must not occur"
    );
}

#[test]
fn drop_phrase_ci_unicode_accented_haystack() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "De modo geral, o sistema funciona bem\n",
        Mode::Full,
        pt,
    );
    assert!(r.safe);
    assert!(!r.output.to_lowercase().contains("de modo geral"));
    assert!(r.output.contains("sistema"));
}

// ── Feature: pt-BR locale behavior ────────────────────────────────────────

#[test]
fn pt_br_articles_dropped() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "o gato e a casa do João são bonitos\n",
        Mode::Full,
        pt,
    );
    assert!(r.safe);
    assert!(r.output.contains("gato"));
    assert!(r.output.contains("João"));
    assert!(!r.output.starts_with("o "), "leading article 'o' must be dropped");
}

#[test]
fn pt_br_fillers_dropped() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "isso é basicamente apenas um teste simples\n",
        Mode::Full,
        pt,
    );
    assert!(r.safe);
    assert!(!r.output.contains("basicamente"));
    assert!(!r.output.contains("apenas"));
    assert!(r.output.contains("teste"));
}

#[test]
fn pt_br_hedges_dropped() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "talvez isso seja possível de implementar\n",
        Mode::Full,
        pt,
    );
    assert!(r.safe);
    assert!(!r.output.contains("talvez"));
}

#[test]
fn pt_br_phrase_com_certeza_dropped() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale("Com certeza, posso ajudar você\n", Mode::Full, pt);
    assert!(r.safe);
    assert!(!r.output.to_lowercase().contains("com certeza"));
}

#[test]
fn pt_br_phrase_de_modo_geral_dropped() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale("De modo geral, o sistema funciona\n", Mode::Full, pt);
    assert!(r.safe);
    assert!(!r.output.to_lowercase().contains("de modo geral"));
}

#[test]
fn pt_br_ultra_subs_applied() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "a configuração da função sem parâmetros\n",
        Mode::Ultra,
        pt,
    );
    assert!(r.safe);
    assert!(r.output.contains("config"));
    assert!(r.output.contains("fn"));
    assert!(r.output.contains("s/"));
    assert!(r.output.contains("param"));
}

#[test]
fn pt_br_preserves_accents_not_in_ultra_subs() {
    let pt = Locale::from_code("pt-BR");
    let r_full = compress_text_with_locale("a nação precisa disso\n", Mode::Full, pt);
    let r_ultra = compress_text_with_locale("a nação precisa disso\n", Mode::Ultra, pt);
    assert!(r_full.output.contains("nação"));
    assert!(r_ultra.output.contains("nação"));
}

#[test]
fn pt_br_word_boundary_no_false_match() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "o funcionário usa a função principal\n",
        Mode::Ultra,
        pt,
    );
    assert!(r.safe);
    assert!(r.output.contains("funcionário"));
    assert!(!r.output.contains("fnário"));
    assert!(r.output.contains("fn"));
}

#[test]
fn pt_br_trim_trailing_conjunction() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale("compila o código e\n", Mode::Full, pt);
    assert!(r.safe);
    let trimmed = r.output.trim_end();
    assert!(!trimmed.ends_with(" e"));
    assert!(!trimmed.ends_with(" ou"));
}

#[test]
fn pt_br_url_preserved() {
    let pt = Locale::from_code("pt-BR");
    let r = compress_text_with_locale(
        "veja a documentação em https://example.com/docs para mais\n",
        Mode::Full,
        pt,
    );
    assert!(r.safe);
    assert!(r.output.contains("https://example.com/docs"));
}

#[test]
fn pt_br_safety_check_realistic_fixture() {
    let pt = Locale::from_code("pt-BR");
    let fixture = "\
# Guia de Uso

Este é um guia básico de configuração do sistema. \
A função principal inicializa o repositório.

```bash
cargo build --release
```

Veja https://example.com/docs para mais detalhes sobre a documentação.

## Instalação

Basicamente, você precisa apenas executar o comando acima. \
Talvez seja necessário instalar as dependências primeiro.

| Comando       | Descrição           |
|---------------|---------------------|
| cargo build   | Compila o projeto   |
| cargo test    | Executa os testes   |
";
    let r = compress_text_with_locale(fixture, Mode::Full, pt);
    assert!(r.safe, "safety check failed: {:?}", r.stats);
    assert_eq!(r.stats.orig_headings, r.stats.new_headings);
    assert_eq!(r.stats.orig_code_blocks, r.stats.new_code_blocks);
    assert!(r.stats.new_urls >= r.stats.orig_urls);
}

// ── Feature: code/table/heading preservation ──────────────────────────────

#[test]
fn pt_br_code_block_untouched() {
    let pt = Locale::from_code("pt-BR");
    let input = "Use a função\n```\nfn configuração() {}\n```\n";
    let r = compress_text_with_locale(input, Mode::Ultra, pt);
    assert!(r.safe);
    assert!(r.output.contains("fn configuração() {}"));
}

#[test]
fn pt_br_table_preserved() {
    let pt = Locale::from_code("pt-BR");
    let input = "Intro.\n\n| coluna | valor |\n|--------|-------|\n| a      | 1     |\n\nFim.\n";
    let r = compress_text_with_locale(input, Mode::Full, pt);
    assert!(r.safe);
    assert!(r.output.contains("| coluna | valor |"));
    assert!(r.output.contains("| a      | 1     |"));
}

#[test]
fn pt_br_headings_preserved() {
    let pt = Locale::from_code("pt-BR");
    let input = "# Título\n\nconteúdo\n\n## Seção\n\nmais conteúdo\n";
    let r = compress_text_with_locale(input, Mode::Full, pt);
    assert_eq!(r.stats.orig_headings, r.stats.new_headings);
    assert!(r.safe);
}

#[test]
fn pt_br_idempotent_second_pass() {
    let pt = Locale::from_code("pt-BR");
    let input = "# Título\n\nO sistema funciona bem com esta configuração.\n";
    let r1 = compress_text_with_locale(input, Mode::Full, pt);
    let r2 = compress_text_with_locale(&r1.output, Mode::Full, pt);
    assert!(r2.safe);
    assert_eq!(r2.stats.new_headings, r1.stats.new_headings);
    assert_eq!(r2.stats.new_code_blocks, r1.stats.new_code_blocks);
}

// ── EN locale regression ───────────────────────────────────────────────────

#[test]
fn en_compress_text_matches_with_locale() {
    let en = Locale::from_code("en");
    let inputs = [
        "The quick brown fox really just jumps.\n",
        "Configure the function with these parameters.\n",
        "# Title\n\nSome prose with the article.\n```rust\nfn main() {}\n```\n",
    ];
    for input in &inputs {
        let legacy = compress_text(input, Mode::Full);
        let with_locale = compress_text_with_locale(input, Mode::Full, en);
        assert_eq!(legacy.output, with_locale.output, "input: {:?}", input);
    }
}

#[test]
fn en_articles_still_dropped() {
    let en = Locale::from_code("en");
    let r = compress_text_with_locale("The quick brown fox jumped over the lazy dog.\n", Mode::Full, en);
    assert!(!r.output.to_lowercase().contains(" the "));
    assert!(r.output.contains("fox"));
}

#[test]
fn en_ultra_subs_still_work() {
    let en = Locale::from_code("en");
    let r = compress_text_with_locale(
        "Configure the function with these parameters.\n",
        Mode::Ultra,
        en,
    );
    assert!(r.output.contains("fn"));
    assert!(r.output.contains("w/"));
    assert!(r.output.contains("param"));
}

// ── Cross-locale contract ─────────────────────────────────────────────────

fn assert_locale_contract(locale: &'static Locale, label: &str) {
    let fixture = "# Title\n\nSome prose content here.\n\n```bash\necho hello\n```\n\nSee https://example.com for details.\n\n| col1 | col2 |\n|------|------|\n| a    | b    |\n";
    let r = compress_text_with_locale(fixture, Mode::Full, locale);
    assert!(r.safe, "[{}] safety check failed", label);
    assert_eq!(r.stats.orig_headings, r.stats.new_headings, "[{}] headings", label);
    assert_eq!(r.stats.orig_code_blocks, r.stats.new_code_blocks, "[{}] code blocks", label);
    assert!(r.stats.new_urls >= r.stats.orig_urls, "[{}] urls", label);
    assert!(r.stats.new_bytes > 0, "[{}] output not empty", label);
}

#[test]
fn contract_en_locale() {
    assert_locale_contract(Locale::from_code("en"), "en");
}

#[test]
fn contract_pt_br_locale() {
    assert_locale_contract(Locale::from_code("pt-BR"), "pt-BR");
}

#[test]
fn contract_unknown_locale_falls_back_to_en() {
    for code in &["fr", "de", "ja", "zh", "ar", "ru", "es", "it"] {
        let locale = Locale::from_code(code);
        assert_eq!(locale.code, "en", "unknown '{}' should fall back to en", code);
        assert_locale_contract(locale, code);
    }
}

#[test]
fn ultra_mode_contract_both_locales() {
    let input = "# Section\n\nThis is some prose content with details.\n\n```rust\nfn main() {}\n```\n";
    let pt_input = "# Seção\n\nEste é o conteúdo com detalhes da configuração.\n\n```rust\nfn main() {}\n```\n";

    let r_en = compress_text_with_locale(input, Mode::Ultra, Locale::from_code("en"));
    let r_pt = compress_text_with_locale(pt_input, Mode::Ultra, Locale::from_code("pt-BR"));

    assert!(r_en.safe);
    assert!(r_pt.safe);
    assert!(r_en.output.contains("fn main() {}"));
    assert!(r_pt.output.contains("fn main() {}"));
}
