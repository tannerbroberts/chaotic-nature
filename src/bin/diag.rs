//! Throwaway diagnostic: where is the energy feeding the herd coming from?

use pentagram::element::Element;
use pentagram::input::InputLog;
use pentagram::world::World;

fn main() {
    let mut w = World::new(0xEC0, 48);
    w.seed_population(10);
    let log = InputLog::new();

    println!(
        "{:>6} {:>6} {:>7} {:>7} {:>12} {:>12} {:>8}",
        "tick", "alive", "births", "deaths", "body_energy", "terrain_sat", "starving"
    );
    for step in 0..12 {
        for _ in 0..2500u32 {
            w.step(&log);
        }
        let energy: u64 = w.entities.iter().map(|e| e.energy as u64).sum();
        let sat: u64 = Element::ALL
            .iter()
            .flat_map(|e| w.terrain.sat[e.index()].iter())
            .map(|v| *v as u64)
            .sum();
        let starving = w.entities.iter().filter(|e| e.energy == 0).count();
        println!(
            "{:>6} {:>6} {:>7} {:>7} {:>12} {:>12} {:>8}",
            (step + 1) * 2500,
            w.alive_count(),
            w.stats.births,
            w.stats.deaths,
            energy,
            sat,
            starving
        );
    }
    let pop = w.population();
    for e in Element::ALL {
        println!(
            "{:<6} pop {:>5}  births {:>6}  deaths {:>6}",
            e.name(),
            pop[e],
            w.stats.births_by[e],
            w.stats.deaths_by[e]
        );
    }
}
