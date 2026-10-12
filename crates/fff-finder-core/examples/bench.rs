//! Index the configured roots and time a few searches:
//! `cargo run --release -p fff-finder-core --example bench -- [query...]`

use std::time::{Duration, Instant};

use fff_finder_core::{Config, Engine, EngineOptions, SearchRequest};

fn main() {
    let roots = Config::load().search_roots;
    println!("roots: {roots:?}");
    let started = Instant::now();
    let engine = Engine::new(EngineOptions {
        roots,
        db_dir: None,
        watch: false,
    });
    while !engine.wait_ready(Duration::from_secs(2)) {
        let files: usize = engine.status().iter().map(|s| s.indexed_files).sum();
        println!("  scanning… {files} files ({:.1?})", started.elapsed());
    }
    let files: usize = engine.status().iter().map(|s| s.indexed_files).sum();
    println!("indexed {files} files in {:.2?}", started.elapsed());

    let queries: Vec<String> = std::env::args().skip(1).collect();
    let queries = if queries.is_empty() {
        vec![
            "readme".into(),
            "cargo.toml".into(),
            "main rs".into(),
            "p".into(),
        ]
    } else {
        queries
    };
    for q in queries {
        let res = engine.search(&SearchRequest {
            query: q.clone(),
            limit: 500,
            ..Default::default()
        });
        println!(
            "{q:>12}: {} hits / {} matched in {:.2?}",
            res.hits.len(),
            res.total_matched,
            res.elapsed
        );
        for hit in res.hits.iter().take(8) {
            println!("        {:>6}  {}", hit.score, hit.entry.path.display());
        }
    }
    let t = Instant::now();
    drop(engine);
    println!("dropped engine in {:.2?}", t.elapsed());
}
