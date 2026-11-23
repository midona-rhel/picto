//! Run with rustc -O --edition=2024 benches/visibility.rs -o /tmp/picto-visibility-bench.
#[path = "../src/grid.rs"]
mod grid;
use std::{hint::black_box, time::Instant};
fn main() {
    for count in [2_500, 20_000, 100_000] {
        let tiles = grid::waterfall(&vec![1.25; count], 1000., 200.);
        let index = grid::VisibilityIndex::new(&tiles);
        let samples = 10_000;
        let measure = |indexed: bool| {
            let start = Instant::now();
            for sample in 0..samples {
                let top = (sample * 137) as f32 % (index.height - 1800.).max(1.);
                if indexed {
                    black_box(index.visible(black_box(&tiles), top, top + 1800.));
                } else {
                    black_box(
                        tiles
                            .iter()
                            .enumerate()
                            .filter_map(|(i, tile)| {
                                (tile.y + tile.height >= top && tile.y <= top + 1800.).then_some(i)
                            })
                            .collect::<Vec<_>>(),
                    );
                }
            }
            start.elapsed().as_nanos() as f64 / samples as f64
        };
        let scan = measure(false);
        let indexed = measure(true);
        println!(
            "{count} tiles: scan {scan:.0} ns, index {indexed:.0} ns, {:.1}x; geometry lookup only, not frame time",
            scan / indexed
        );
    }
}
