//! Electricite a corridors : corridor sous-dimensionne, pylone, capacites, bilan d'energie,
//! sauvegardes.

use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::sim::{Command, Simulation};

fn put(s: &mut Simulation, key: &str, x: i32, y: i32, d: Dir) -> EntityId {
    for dy in 0..3 {
        for dx in 0..3 {
            let p = Pos::new(x + dx, y + dy);
            s.apply(Command::ClearFeature { pos: p });
            s.apply(Command::FillWater { pos: p });
        }
    }
    s.build_key(key, x, y, d)
        .unwrap_or_else(|e| panic!("{key} en ({x}, {y}) : {e:?}"))
}

/// Centrale (generateur 1 MW) sur la sous-station A ; usine de 10 fours (900 kW) sur la
/// sous-station B, a 16 cases ; un seul poteau entre les deux.
fn factory(s: &mut Simulation, ox: i32, oy: i32) -> Vec<EntityId> {
    put(s, "sous_station", ox, oy, Dir::N);
    put(s, "generateur", ox - 5, oy - 1, Dir::N);
    put(s, "sous_station", ox + 16, oy, Dir::N);
    put(s, "poteau", ox + 8, oy, Dir::N);
    let a = s.content.item("minerai_a").unwrap();
    let mut furnaces = Vec::new();
    let spots: Vec<(i32, i32)> = (0..7)
        .map(|k| (10 + 2 * k, -6))
        .chain((0..3).map(|k| (12 + 2 * k, 5)))
        .collect();
    for (x, y) in spots {
        furnaces.push(put(s, "four", ox + x, oy + y, Dir::N));
        s.debug_insert(Pos::new(ox + x, oy + y), a, 8);
    }
    furnaces
}

fn check_links(s: &mut Simulation) {
    for (a, b, cap, f, _) in s.power_links() {
        assert!(
            f.unsigned_abs() <= cap,
            "liaison {a}-{b} : flot {f} > capacite {cap}"
        );
    }
}

#[test]
fn undersized_corridor_limits_the_factory_and_a_pylon_restores_it() {
    let mut s = Simulation::new(3);
    let furnaces = factory(&mut s, 400, 400);
    s.run(40);
    check_links(&mut s);
    let nets = s.power_nets();
    assert_eq!(nets.len(), 1, "un seul reseau");
    let (supply, demand, sat) = nets[0];
    assert_eq!(supply, 1_000_000);
    assert_eq!(demand, 900_000, "dix fours en marche");
    // 500 kW passent par le poteau : 5/9 de la demande
    assert_eq!(sat, 555, "usine partiellement alimentee");
    for &f in &furnaces {
        assert_eq!(s.satisfaction_of(f), 555);
        assert_eq!(s.entity(f).unwrap().status, Status::LowPower);
    }
    // le corridor limitant est signale
    let limiting: Vec<_> = s.power_links().into_iter().filter(|l| l.4).collect();
    assert!(!limiting.is_empty(), "corridor limitant signale");
    for l in &limiting {
        assert_eq!(l.3.unsigned_abs(), l.2, "liaison limitante saturee");
    }
    // un pylone entre les deux sous-stations : capacite elevee, usine alimentee
    put(&mut s, "pylone", 408, 403, Dir::N);
    s.run(5);
    check_links(&mut s);
    assert_eq!(s.power_nets()[0].2, 1000);
    for &f in &furnaces {
        assert_eq!(s.satisfaction_of(f), 1000);
    }
    assert!(
        s.power_links().iter().all(|l| !l.4),
        "plus de corridor limitant"
    );
    // retrait du pylone : on retombe sur le poteau
    assert!(s.remove_at(Pos::new(408, 403)));
    s.run(5);
    assert_eq!(s.power_nets()[0].2, 555);
    // retrait du poteau : deux reseaux, l'usine n'est plus alimentee
    assert!(s.remove_at(Pos::new(408, 400)));
    s.run(5);
    assert_eq!(s.power_nets().len(), 2);
    for &f in &furnaces {
        assert_eq!(s.satisfaction_of(f), 0);
    }
    assert!(s.mass_balance_ok());
}

#[test]
fn energy_balance_and_capacities_hold_over_thousands_of_ticks() {
    let mut s = Simulation::new(4);
    crate::scenario::build_demo(&mut s).unwrap();
    factory(&mut s, 400, 400);
    // une seconde centrale dans l'usine, branchee plus tard
    for t in 0..4000u64 {
        s.step();
        if t == 1500 {
            put(&mut s, "generateur", 430, 396, Dir::N);
        }
        if t % 7 == 0 {
            check_links(&mut s);
            let total_supply: u64 = s.power_nets().iter().map(|n| n.0).sum();
            let delivered_now: u64 = s.power_nets().iter().map(|n| n.1 * n.2 as u64 / 1000).sum();
            assert!(delivered_now <= total_supply + 1000);
        }
    }
    assert_eq!(s.energy.electric_produced, s.energy.electric_consumed);
    assert!(s.energy.electric_produced > 0);
    assert!(s.mass_balance_ok());
}

#[test]
fn power_is_deterministic_and_survives_save_and_load_mid_period() {
    let build = || {
        let mut s = Simulation::new(9);
        crate::scenario::build_demo(&mut s).unwrap();
        factory(&mut s, 400, 400);
        s
    };
    let mut a = build();
    let mut b = build();
    a.run(777);
    b.run(777);
    assert_eq!(a.state_hash(), b.state_hash());
    // 777 n'est pas un multiple de la periode de calcul : les solutions sont sauvegardees
    let mut c = Simulation::load_bytes(&a.save_bytes()).unwrap();
    assert_eq!(c.state_hash(), a.state_hash());
    put(&mut a, "pylone", 408, 403, Dir::N);
    put(&mut c, "pylone", 408, 403, Dir::N);
    a.run(1000);
    c.run(1000);
    assert_eq!(a.state_hash(), c.state_hash());
}

#[test]
fn version2_saves_are_converted() {
    let early: &[u8] = include_bytes!("../tests/data/save_v2_t1234.bin");
    let late: &[u8] = include_bytes!("../tests/data/save_v2_t4234.bin");
    assert_eq!(u32::from_le_bytes(early[4..8].try_into().unwrap()), 2);
    let mut s = Simulation::load_bytes(early).unwrap();
    assert_eq!(s.tick, 1234);
    assert!(s.mass_balance_ok());
    s.run(3000);
    let t = Simulation::load_bytes(late).unwrap();
    // aucun corridor ne limite ces deux reseaux : meme partie qu'avec l'ancien calcul global
    assert_eq!(s.state_hash_without_power(), t.state_hash_without_power());
    let u = Simulation::load_bytes(&s.save_bytes()).unwrap();
    assert_eq!(u.state_hash(), s.state_hash());
}
