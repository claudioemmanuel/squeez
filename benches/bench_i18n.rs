// Benchmark: EN vs PT-BR compression speed + ratio.
// Run with: cargo run --release --bin bench_i18n
use std::hint::black_box;
use squeez::commands::compress_md::{compress_text_with_locale, Locale, Mode};

fn tokens(bytes: usize) -> usize { bytes / 4 }

fn print_ratio(label: &str, input: &str, locale: &'static Locale, mode: Mode) {
    let r = compress_text_with_locale(input, mode, locale);
    let before_tk = tokens(r.stats.orig_bytes);
    let after_tk  = tokens(r.stats.new_bytes);
    let pct = 100usize.saturating_sub(after_tk * 100 / before_tk.max(1));
    println!("  {:<24} {:>6}tk → {:>5}tk  -{:>2}%  safe={}", label, before_tk, after_tk, pct, r.safe);
}

fn main() {
    let en = Locale::from_code("en");
    let pt = Locale::from_code("pt-BR");

    let en_input = include_str!("fixtures/en_prose.txt");
    let pt_input = include_str!("fixtures/pt_br_prose.txt");

    println!("── Compression ratio ────────────────────────────────────────");
    print_ratio("EN prose  / Full",   en_input, en, Mode::Full);
    print_ratio("EN prose  / Ultra",  en_input, en, Mode::Ultra);
    print_ratio("PT-BR prose / Full", pt_input, pt, Mode::Full);
    print_ratio("PT-BR prose / Ultra",pt_input, pt, Mode::Ultra);
    println!();

    let iters = 1000u32;

    println!("── Latency (×{iters} iterations) ──────────────────────────────────");
    let start = std::time::Instant::now();
    for _ in 0..iters {
        black_box(compress_text_with_locale(black_box(en_input), Mode::Full, en));
    }
    let en_ms = start.elapsed().as_millis();

    let start = std::time::Instant::now();
    for _ in 0..iters {
        black_box(compress_text_with_locale(black_box(pt_input), Mode::Full, pt));
    }
    let pt_ms = start.elapsed().as_millis();

    println!("  EN Full:    {}ms  ({:.0}µs/call)", en_ms, en_ms as f64 * 1000.0 / iters as f64);
    println!("  PT-BR Full: {}ms  ({:.0}µs/call)  {:.2}× vs EN", pt_ms,
        pt_ms as f64 * 1000.0 / iters as f64,
        pt_ms as f64 / en_ms.max(1) as f64);

    assert!(pt_ms < en_ms * 3 + 100, "PT-BR too slow vs EN: {}ms vs {}ms", pt_ms, en_ms);

    println!();
    println!("── Before / after example (pt-BR) ────────────────────────────");
    let demo = "O sistema é basicamente apenas uma ferramenta para configurar o repositório. \
                De modo geral, você pode considerar que a função principal inicializa a documentação do projeto.";
    let rf = compress_text_with_locale(demo, Mode::Full,  pt);
    let ru = compress_text_with_locale(demo, Mode::Ultra, pt);
    println!("  IN:    {}", demo);
    println!("  Full:  {}", rf.output.trim());
    println!("  Ultra: {}", ru.output.trim());
}

#[allow(dead_code)]
fn show_example() {
    let pt = Locale::from_code("pt-BR");
    let inputs = [
        ("O sistema é basicamente apenas uma ferramenta para configurar o repositório. De modo geral, você pode considerar que a função principal inicializa a documentação do projeto.", "pt-BR Full"),
        ("O sistema é basicamente apenas uma ferramenta para configurar o repositório. De modo geral, você pode considerar que a função principal inicializa a documentação do projeto.", "pt-BR Ultra"),
    ];
    for (input, label) in inputs {
        let mode = if label.contains("Ultra") { Mode::Ultra } else { Mode::Full };
        let r = compress_text_with_locale(input, mode, pt);
        println!("{}: {}", label, r.output.trim());
    }
}
