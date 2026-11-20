//! Bancs de mesure EXODE (cahier v0.4 section 12). Resultats indicatifs : seules les mesures
//! faites sur les appareils de reference, consignees avec version et duree, valent resultat.
//!
//! Utilisation :
//!   exode-bench belts <lignes> <longueur> <ticks>   (F1 : convoyeurs satures en mouvement)
//!   exode-bench demo <copies> <ticks>               (F2/F11 : chaines completes recopiees)
//!   exode-bench all                                 (jeu de mesures par defaut)

use exode_core::scenario;
use exode_core::{Dir, Pos, Simulation};
use std::time::Instant;

fn report(name: &str, sim: &Simulation, ticks: u64, secs: f64) {
    let ms = secs * 1000.0 / ticks as f64;
    println!(
        "{name:<34} entites={:>7} objets_tapis={:>8} ticks={:>6} ms/tick={:>8.3} ({:.0} ticks/s) empreinte={:016x}",
        sim.entity_count(),
        sim.belt_item_count(),
        ticks,
        ms,
        1000.0 / ms.max(1e-9),
        sim.state_hash()
    );
}

fn bench_belts(lines: i32, length: i32, ticks: u64) {
    let mut sim = Simulation::new(1);
    // zone degagee : on construit loin de l'eau en testant les cases une a une
    let (ox, oy) = (-(length / 2), -lines);
    let mut built = 0;
    let a = sim.content.item("minerai_a").unwrap();
    for l in 0..lines {
        let y = oy + l * 2;
        let mut x = ox;
        let end = ox + length;
        while x < end {
            if sim.build_key("convoyeur", x, y, Dir::E).is_ok() {
                built += 1;
            }
            x += 1;
        }
    }
    // remplissage : 3 objets par voie et par case (ligne mobile, pas totalement compacte)
    let positions: Vec<Pos> = sim.entities().map(|e| e.pos).collect();
    for p in positions {
        for lane in 0..2 {
            for k in 0..3 {
                sim.debug_place_on_belt(p, lane, a, 20 + k * 80);
            }
        }
    }
    let t = Instant::now();
    sim.run(ticks);
    report(
        &format!("F1 convoyeurs {lines}x{length} ({built})"),
        &sim,
        ticks,
        t.elapsed().as_secs_f64(),
    );
}

fn bench_demo(copies: i32, ticks: u64) {
    let mut sim = Simulation::new(42);
    scenario::build_demo(&mut sim).expect("scene de demonstration");
    let mut extra = 0;
    // Copies sans foreuses : on alimente les fours directement pour mesurer la charge
    // machines + bras + convoyeurs. Placees en grille loin du depart.
    let mut k = 0;
    let mut tries = 0;
    while extra < copies - 1 && tries < copies * 20 {
        let gx = (k % 40) * 30 + 60;
        let gy = (k / 40) * 30 + 60;
        k += 1;
        tries += 1;
        if build_feed_copy(&mut sim, gx, gy).is_ok() {
            extra += 1;
        }
    }
    let t = Instant::now();
    sim.run(ticks);
    report(
        &format!("F2 chaines {} copies", extra + 1),
        &sim,
        ticks,
        t.elapsed().as_secs_f64(),
    );
    assert!(sim.mass_balance_ok(), "bilan matiere");
}

/// Copie de chaine sans gisement : coffre source -> bras -> convoyeurs -> four -> assembleur.
fn build_feed_copy(sim: &mut Simulation, ox: i32, oy: i32) -> Result<(), String> {
    use Dir::*;
    let plan: &[(&str, i32, i32, Dir)] = &[
        ("coffre", 0, 0, N),
        ("bras", 1, 0, E),
        ("convoyeur", 2, 0, E),
        ("convoyeur", 3, 0, E),
        ("convoyeur", 4, 0, E),
        ("convoyeur", 5, 0, E),
        ("bras", 6, 0, E),
        ("four", 7, -1, E),
        ("bras", 7, -2, N),
        ("benne", 7, -3, N),
        ("bras", 9, 0, E),
        ("assembleur", 10, -1, E),
        ("bras", 10, -2, N),
        ("benne", 10, -3, N),
        ("bras", 12, 0, E),
        ("coffre", 13, 0, N),
        ("generateur", 2, 2, N),
        ("poteau", 1, 1, N),
        ("poteau", 6, 1, N),
        ("poteau", 11, 1, N),
    ];
    // verification prealable pour ne rien laisser a moitie construit
    for (key, x, y, d) in plan {
        let proto = sim.content.proto(key).unwrap();
        sim.can_build(proto, Pos::new(ox + x, oy + y), *d)
            .map_err(|e| format!("{e:?}"))?;
    }
    for (key, x, y, d) in plan {
        sim.build_key(key, ox + x, oy + y, *d)
            .map_err(|e| format!("{e:?}"))?;
    }
    let gear = sim.content.recipe("engrenage").unwrap();
    sim.set_recipe(Pos::new(ox + 10, oy - 1), Some(gear));
    let a = sim.content.item("minerai_a").unwrap();
    sim.debug_insert(Pos::new(ox, oy), a, 400);
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: i64| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    match args.get(1).map(|s| s.as_str()) {
        Some("belts") => bench_belts(num(2, 100) as i32, num(3, 100) as i32, num(4, 300) as u64),
        Some("demo") => bench_demo(num(2, 50) as i32, num(3, 600) as u64),
        _ => {
            println!("Mesures indicatives sur la machine courante (ce ne sont pas des resultats sur appareil).");
            bench_belts(20, 100, 300);
            bench_belts(100, 100, 300);
            bench_belts(200, 200, 150);
            bench_demo(1, 900);
            bench_demo(100, 600);
            bench_demo(400, 300);
        }
    }
}
