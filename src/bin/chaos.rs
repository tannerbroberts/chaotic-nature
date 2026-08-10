//! Chaotic Nature — live view.
//!
//! The tuning loop this project is built around: change a number in
//! `src/race.rs`, run `chaos`, watch what the world does about it.
//!
//! Zero dependencies, ANSI escapes only. Ctrl-C to quit.

// Presentation only. The simulation this renders never sees a float.
#![allow(clippy::float_arithmetic)]

use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use pentagram::element::{Element, PerElement};
use pentagram::input::InputLog;
use pentagram::race::{attrs, TERRAIN_PERIOD};
use pentagram::replay::{build, scripted_log};

const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const DENSITY: [char; 5] = ['·', ':', '+', '*', '#'];
const HISTORY: usize = 24;

/// Element colours, matching the design document.
const RGB: PerElement<(u8, u8, u8)> = PerElement([
    (127, 176, 105), // Wood
    (226, 105, 74),  // Fire
    (211, 164, 69),  // Earth
    (173, 164, 206), // Metal
    (89, 160, 198),  // Water
]);

struct Args {
    size: i32,
    pop: u32,
    speed: u64,
    cols: usize,
    rows: usize,
    seed: u64,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args {
            size: 96,
            pop: 20,
            speed: 120,
            cols: 78,
            rows: 30,
            seed: 0xBEEF,
        };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < argv.len() {
            let val = argv.get(i + 1).and_then(|s| s.parse::<i64>().ok());
            match argv[i].as_str() {
                "--size" => a.size = val.unwrap_or(96) as i32,
                "--pop" => a.pop = val.unwrap_or(20) as u32,
                "--speed" => a.speed = val.unwrap_or(120).max(1) as u64,
                "--cols" => a.cols = val.unwrap_or(78).clamp(20, 200) as usize,
                "--rows" => a.rows = val.unwrap_or(30).clamp(10, 100) as usize,
                "--seed" => a.seed = val.unwrap_or(0xBEEF) as u64,
                "--help" | "-h" => {
                    println!(
                        "chaos — Chaotic Nature live view\n\n\
                         options:\n  \
                         --size N    world edge in cells   (default 96)\n  \
                         --pop N     bodies per race       (default 20)\n  \
                         --speed N   sim ticks per second  (default 120, real time is 1.67)\n  \
                         --cols N    view width in chars   (default 78)\n  \
                         --rows N    view height in chars  (default 30)\n  \
                         --seed N    world seed            (default 0xBEEF)\n\n\
                         subcommands (via the wrapper): verify · soak · test · edit · watch"
                    );
                    std::process::exit(0);
                }
                _ => {}
            }
            i += 1;
        }
        a
    }
}

fn color(c: (u8, u8, u8)) -> String {
    format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2)
}

const DIM: &str = "\x1b[38;2;110;118;134m";
const BRIGHT: &str = "\x1b[38;2;232;234;239m";
const RESET: &str = "\x1b[0m";

fn spark(history: &[u32], max: u32) -> String {
    if max == 0 {
        return DIM.to_string() + &"▁".repeat(history.len().min(HISTORY)) + RESET;
    }
    history
        .iter()
        .map(|v| {
            let idx = ((*v as u64 * 7) / max.max(1) as u64).min(7) as usize;
            SPARK[idx]
        })
        .collect()
}

fn main() {
    let a = Args::parse();

    // A long scripted command stream so incarnations keep arriving; the same
    // deterministic log the verifier uses.
    let total_ticks = 100_000_000u64;
    let log = scripted_log(0x5EED, 200_000, a.pop * 5);
    let mut w = build(a.seed, a.size, a.pop);
    let empty = InputLog::new();

    let mut history: PerElement<Vec<u32>> = PerElement([
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ]);

    let frame_dt = Duration::from_millis(50);
    let ticks_per_frame = (a.speed / 20).max(1);
    let mut out = BufWriter::new(std::io::stdout());

    // Alternate screen, cursor hidden. Restored on Ctrl-C by the wrapper.
    let _ = write!(out, "\x1b[?1049h\x1b[?25l\x1b[2J");
    let start = Instant::now();

    let mut tick_budget = 0u64;
    while w.tick < total_ticks {
        let frame_start = Instant::now();

        for _ in 0..ticks_per_frame {
            if w.tick < log.last_tick() {
                w.step(&log);
            } else {
                w.step(&empty);
            }
        }
        tick_budget += ticks_per_frame;

        // ---- sample the world into a character grid ----
        let mut counts = vec![[0u16; Element::COUNT]; a.cols * a.rows];
        for e in &w.entities {
            if !e.alive {
                continue;
            }
            let fx = e.pos.x.floor_int().max(0) as usize;
            let fy = e.pos.y.floor_int().max(0) as usize;
            let sz = a.size.max(1) as usize;
            let cx = (fx * a.cols / sz).min(a.cols - 1);
            let cy = (fy * a.rows / sz).min(a.rows - 1);
            counts[cy * a.cols + cx][e.element.index()] += 1;
        }

        let pop = w.population();
        for e in Element::ALL {
            let h = history.get_mut(e);
            h.push(pop[e]);
            if h.len() > HISTORY {
                h.remove(0);
            }
        }

        // ---- draw ----
        let _ = write!(out, "\x1b[H");
        let sim_min = w.tick / TERRAIN_PERIOD;
        // Suppress the rate until there is enough elapsed time to divide by,
        // otherwise the first few frames report nonsense.
        let secs = start.elapsed().as_secs_f64();
        let rate = if secs > 1.0 {
            format!("{}× real time", (tick_budget as f64 / secs / 1.667) as u64)
        } else {
            "measuring…".to_string()
        };
        let _ = writeln!(
            out,
            "{BRIGHT}CHAOTIC NATURE{RESET}{DIM}  ·  live{RESET}    \
             tick {}  ·  {}d {:02}h {:02}m simulated  ·  {}{DIM}      {RESET}",
            w.tick,
            sim_min / 1440,
            (sim_min / 60) % 24,
            sim_min % 60,
            rate
        );

        let _ = writeln!(out, "{DIM}┌{}┐{RESET}", "─".repeat(a.cols));
        for row in 0..a.rows {
            let _ = write!(out, "{DIM}│{RESET}");
            for col in 0..a.cols {
                let c = &counts[row * a.cols + col];
                let total: u16 = c.iter().sum();
                if total == 0 {
                    let _ = write!(out, " ");
                    continue;
                }
                let (best, _) = c
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, n)| **n)
                    .unwrap_or((0, &0));
                let d = DENSITY[((total as usize).saturating_sub(1)).min(4)];
                let _ = write!(out, "{}{}{}", color(RGB.0[best]), d, RESET);
            }
            let _ = writeln!(out, "{DIM}│{RESET}");
        }
        let _ = writeln!(out, "{DIM}└{}┘{RESET}", "─".repeat(a.cols));

        let _ = writeln!(
            out,
            "{DIM}race     alive  {:<24}  granted / ceiling   state{RESET}",
            "population"
        );

        let peak = Element::ALL
            .iter()
            .flat_map(|e| history[*e].iter().copied())
            .max()
            .unwrap_or(1)
            .max(1);

        for e in Element::ALL {
            let g = w.last_deposit[e];
            let band = attrs(e).deposit;
            let state = if pop[e] == 0 {
                format!("{DIM}extinct — churning at floor{RESET}")
            } else if g.clipped > 0 {
                format!("{}rate-limited{RESET}", color(RGB[e]))
            } else if g.forced > 0 {
                format!("{DIM}idle{RESET}")
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "{}{:<7}{RESET} {:>5}  {}{:<24}{RESET}  {:>7} / {:<7}   {}",
                color(RGB[e]),
                e.name(),
                pop[e],
                color(RGB[e]),
                spark(&history[e], peak),
                g.granted,
                band.ceiling,
                state
            );
        }

        let _ = writeln!(
            out,
            "\n{DIM}deaths {}   births {}   collisions {}   \
             tune: src/race.rs   ctrl-c to quit{RESET}\x1b[J",
            w.stats.deaths, w.stats.births, w.stats.collisions
        );
        let _ = out.flush();

        if let Some(rest) = frame_dt.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}
