//! Energy-flow and archetype diagnostics — the tuning bench.
//!
//!   cargo run --release --bin diag [size] [ticks] [interval]
//!
//! Prints the world trajectory, then per-race *archetype signatures*:
//!
//! - **spread** — mean distance to the race's centroid, in cells. Builders
//!   clump; nomads scatter.
//! - **range** — net cells covered per interval by bodies that survived it.
//!   Builders hover; nomads travel.
//! - **burst** — Fano factor (variance/mean) of births per interval. Steady
//!   breeders sit near 1; trove breeders spike far above it.
//! - **food** — total saturation of the plane this race eats, i.e. how set
//!   the table is.

// Reporting only. Floats never touch the simulation; the crate-wide lint
// makes that a compile error rather than a code-review hope.
#![allow(clippy::float_arithmetic)]

use std::collections::HashMap;

use pentagram::element::Element;
use pentagram::fx::V2;
use pentagram::input::InputLog;
use pentagram::world::World;

fn main() {
    let argv: Vec<i64> = std::env::args()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    let size = *argv.first().unwrap_or(&256) as i32;
    let ticks = *argv.get(1).unwrap_or(&60_000) as u64;
    let interval = *argv.get(2).unwrap_or(&5_000) as u64;

    let mut w = World::new(0xEC0, size);
    w.seed_population(10);
    let log = InputLog::new();

    println!("world {size}×{size} · {ticks} ticks · sampling every {interval}\n");
    println!(
        "{:>7} {:>6} {:>7} {:>7} {:>12} {:>12} {:>8}",
        "tick", "alive", "births", "deaths", "body_energy", "terrain_sat", "starving"
    );

    let races = Element::ALL;
    let mut prev_pos: HashMap<u32, V2> = HashMap::new();
    let mut prev_births = [0u64; 5];
    let mut birth_series: [Vec<f64>; 5] = Default::default();
    let mut drift_sum = [0.0f64; 5];
    let mut drift_n = [0u64; 5];

    let samples = (ticks / interval).max(1);
    for s in 0..samples {
        for _ in 0..interval {
            w.step(&log);
        }
        let now = (s + 1) * interval;

        // Trajectory row.
        let energy: u64 = w.entities.iter().map(|e| e.energy as u64).sum();
        let sat: u64 = races
            .iter()
            .flat_map(|e| w.terrain.sat[e.index()].iter())
            .map(|v| *v as u64)
            .sum();
        let starving = w.entities.iter().filter(|e| e.energy == 0).count();
        println!(
            "{:>7} {:>6} {:>7} {:>7} {:>12} {:>12} {:>8}",
            now,
            w.alive_count(),
            w.stats.births,
            w.stats.deaths,
            energy,
            sat,
            starving
        );

        // Births per interval, per race — burstiness raw material.
        for e in races {
            let b = w.stats.births_by[e];
            birth_series[e.index()].push((b - prev_births[e.index()]) as f64);
            prev_births[e.index()] = b;
        }

        // Drift: net range covered by bodies present at both samples —
        // cells of ground actually left behind, per interval. Builders
        // hover near zero; nomads leave.
        for ent in &w.entities {
            if let Some(old) = prev_pos.get(&ent.id) {
                let d = (ent.pos - *old).len().to_f32_render() as f64;
                drift_sum[ent.element.index()] += d;
                drift_n[ent.element.index()] += 1;
            }
        }
        prev_pos = w.entities.iter().map(|e| (e.id, e.pos)).collect();
    }

    // ------------------------------------------------------------------
    // Archetype signatures.
    // ------------------------------------------------------------------
    println!(
        "\n{:<6} {:>5} {:>7} {:>7} {:>8} {:>8} {:>7} {:>12}",
        "race", "pop", "births", "deaths", "spread", "range", "burst", "food(eats)"
    );
    for e in races {
        let bodies: Vec<&pentagram::entity::Entity> =
            w.entities.iter().filter(|b| b.element == e).collect();
        let pop = bodies.len();

        // Mean distance to centroid.
        let spread = if pop > 1 {
            let (mut cx, mut cy) = (0.0f64, 0.0f64);
            for b in &bodies {
                cx += b.pos.x.to_f32_render() as f64;
                cy += b.pos.y.to_f32_render() as f64;
            }
            cx /= pop as f64;
            cy /= pop as f64;
            bodies
                .iter()
                .map(|b| {
                    let dx = b.pos.x.to_f32_render() as f64 - cx;
                    let dy = b.pos.y.to_f32_render() as f64 - cy;
                    (dx * dx + dy * dy).sqrt()
                })
                .sum::<f64>()
                / pop as f64
        } else {
            0.0
        };

        // Fano factor of births per interval.
        let series = &birth_series[e.index()];
        let mean = series.iter().sum::<f64>() / series.len().max(1) as f64;
        let var = series.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
            / series.len().max(1) as f64;
        let burst = if mean > 0.0 { var / mean } else { 0.0 };

        let drift = if drift_n[e.index()] > 0 {
            drift_sum[e.index()] / drift_n[e.index()] as f64
        } else {
            0.0
        };

        let food: u64 = w.terrain.sat[e.eats().index()].iter().map(|v| *v as u64).sum();

        println!(
            "{:<6} {:>5} {:>7} {:>7} {:>8.1} {:>8.1} {:>7.1} {:>12}",
            e.name(),
            pop,
            w.stats.births_by[e],
            w.stats.deaths_by[e],
            spread,
            drift,
            burst,
            food
        );
    }
    println!(
        "\nsignatures: builders → small spread, low drift, burst ≈ 1 · \
         nomads → large spread, high drift, burst ≫ 1 · \
         the aggressor → high deaths per body, its prey's food plane gutted"
    );
}
