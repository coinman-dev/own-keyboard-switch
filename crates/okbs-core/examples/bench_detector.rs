//! Measures data loading and per-word detection time.
//!
//! `cargo run -p okbs-core --release --example bench_detector`

use okbs_core::Lang;
use okbs_core::config::Config;
use okbs_core::detect::Detector;
use okbs_core::layouts::{builtin_keymap, keys_for_text};
use std::time::{Duration, Instant};

fn main() {
    let t = Instant::now();
    okbs_core::data::warm_up();
    println!("data warm-up: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);

    let detector = Detector::from_config(&Config::default());
    let mut cases = Vec::new();
    for lang in Lang::ALL {
        let path = format!(
            "{}/../../data/generated/eval-{}.tsv",
            env!("CARGO_MANIFEST_DIR"),
            lang.code()
        );
        let text = std::fs::read_to_string(path).expect("eval data");
        for word in text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split('\t').next())
        {
            if let Some(keys) = keys_for_text(word, builtin_keymap(lang)) {
                cases.push((keys, lang));
            }
        }
    }

    let mut times: Vec<Duration> = Vec::with_capacity(cases.len() * 2);
    let t = Instant::now();
    for (keys, lang) in &cases {
        for current in [*lang, lang.other()] {
            let one = Instant::now();
            std::hint::black_box(detector.decide(keys, current));
            times.push(one.elapsed());
        }
    }
    let total = t.elapsed();
    times.sort();
    let pct = |p: f64| times[((times.len() - 1) as f64 * p) as usize].as_secs_f64() * 1e6;
    println!(
        "{} decisions: mean {:.1} µs, p50 {:.1} µs, p99 {:.1} µs, p99.9 {:.1} µs, max {:.1} µs",
        times.len(),
        total.as_secs_f64() * 1e6 / times.len() as f64,
        pct(0.5),
        pct(0.99),
        pct(0.999),
        pct(1.0)
    );
}
