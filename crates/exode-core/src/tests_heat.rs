//! Chaleur : chaine complete (pompe a eau, chaudiere, caloducs, echangeur, turbine),
//! bilans matiere et energie, combustible extrait, sauvegardes.

use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::sim::{Command, Simulation};
use crate::world::Ground;

/// Case d'eau dont la colonne au nord est en terre ferme (apres remblai).
fn shore(s: &mut Simulation) -> (i32, i32) {
    for r in 40..800 {
        for k in 0..(8 * r) {
            let (x, y) = match k / (2 * r) {
                0 => (-r + k % (2 * r), -r),
                1 => (r, -r + k % (2 * r)),
                2 => (r - k % (2 * r), r),
                _ => (-r, r - k % (2 * r)),
            };
            if s.world.ground(Pos::new(x, y)) == Ground::Water
                && s.world.ground(Pos::new(x, y - 1)) != Ground::Water
            {
                return (x, y);
            }
        }
    }
    panic!("pas de lac");
}

struct Chain {
    boiler: Pos,
    exchanger: EntityId,
    turbine: EntityId,
    furnaces: Vec<EntityId>,
}

/// Chaine complete au bord d'un lac. Rien d'autre ne produit d'electricite.
fn chain(s: &mut Simulation) -> Chain {
    let (wx, wy) = shore(s);
    // terrain : tout le rectangle en terre ferme, sauf la prise d'eau
    for y in wy - 17..wy {
        for x in wx - 8..wx + 7 {
            let p = Pos::new(x, y);
            s.apply(Command::FillWater { pos: p });
            s.apply(Command::ClearFeature { pos: p });
        }
    }
    let b = |s: &mut Simulation, k: &str, x: i32, y: i32, d: Dir| {
        s.build_key(k, x, y, d)
            .unwrap_or_else(|e| panic!("{k} en ({x}, {y}) : {e:?}"))
    };
    b(s, "pompe_eau", wx, wy - 2, Dir::N);
    b(s, "conduite", wx, wy - 3, Dir::N);
    b(s, "conduite", wx, wy - 4, Dir::N);
    let exchanger = b(s, "echangeur", wx - 1, wy - 7, Dir::N);
    for y in 8..=10 {
        b(s, "conduite", wx, wy - y, Dir::N);
    }
    let turbine = b(s, "turbine", wx - 1, wy - 13, Dir::S);
    // caloducs : du raccord de l'echangeur (est) a celui de la chaudiere (sud)
    for x in 1..=4 {
        b(s, "caloduc", wx + x, wy - 6, Dir::N);
    }
    for y in 7..=11 {
        b(s, "caloduc", wx + 4, wy - y, Dir::N);
    }
    b(s, "chaudiere", wx + 3, wy - 14, Dir::N);
    // electricite : un poteau, des fours ; seule la turbine produit
    b(s, "poteau", wx - 3, wy - 12, Dir::N);
    let a = s.content.item("minerai_a").unwrap();
    let mut furnaces = Vec::new();
    for y in [15, 12, 9] {
        furnaces.push(b(s, "four", wx - 6, wy - y, Dir::N));
        s.debug_insert(Pos::new(wx - 6, wy - y), a, 8);
    }
    Chain {
        boiler: Pos::new(wx + 3, wy - 14),
        exchanger,
        turbine,
        furnaces,
    }
}

fn refuel(s: &mut Simulation, c: &Chain) {
    let fuel = s.content.item("combustible").unwrap();
    s.debug_insert(c.boiler, fuel, 5);
}

#[test]
fn full_chain_turns_fuel_and_water_into_electricity() {
    let mut s = Simulation::new(11);
    let c = chain(&mut s);
    let (mut powered, mut demand_ticks) = (0, 0);
    for t in 0..4000u64 {
        if t % 100 == 0 {
            refuel(&mut s, &c);
            for &f in &c.furnaces {
                let p = s.entity(f).unwrap().pos;
                let a = s.content.item("minerai_a").unwrap();
                s.debug_insert(p, a, 8);
            }
        }
        let demand = c.furnaces.iter().any(|f| s.entity(*f).unwrap().wants_power);
        s.step();
        if demand && t > 300 {
            demand_ticks += 1;
            if s.entity(c.turbine).unwrap().status == Status::Working {
                powered += 1;
            }
        }
        if t % 97 == 0 {
            assert!(s.mass_balance_ok(), "bilan matiere au tick {t}");
            assert!(s.heat_balance_ok(), "bilan thermique au tick {t}");
            assert_eq!(s.energy.electric_produced, s.energy.electric_consumed);
        }
    }
    let h = s.heat_stats.clone();
    assert!(h.fuel_burned > 0 && h.heat_produced > 0, "{h:?}");
    assert!(h.heat_to_steam > 0, "de la vapeur est produite : {h:?}");
    assert!(h.turbine_output > 0, "la turbine produit : {h:?}");
    // toute l'electricite vient de la turbine
    assert_eq!(s.energy.electric_produced, h.turbine_output);
    // vapeur evacuee = electricite livree + energie gardee dans la turbine
    let Data::Turbine(t) = s.entity(c.turbine).unwrap().data else {
        panic!()
    };
    assert_eq!(h.steam_used, h.turbine_output + t.energy);
    assert!(h.heat_lost > 0, "les caloducs perdent un peu de chaleur");
    // la turbine ne produit que s'il y a une demande ; une fois la chaine chaude, elle la sert
    assert!(demand_ticks > 300, "les fours ont demande de l'energie");
    assert!(
        powered * 10 >= demand_ticks * 9,
        "turbine en marche {powered} ticks sur {demand_ticks} ticks de demande"
    );
    // les fours ont tourne sur l'electricite de la turbine (puis leur sortie s'est remplie)
    let plates = s.content.item("plaque_a").unwrap();
    assert!(s.stats.produced.get(&plates).copied().unwrap_or(0) >= 12);
    assert!(s.furnaces_powered(&c.furnaces) || s.energy.electric_consumed > 0);
    let (temp, _, _) = s.heat_of(c.exchanger).unwrap();
    assert!(temp >= 165, "echangeur chaud : {temp}");
    assert!(s.mass_balance_ok() && s.heat_balance_ok());
}

impl Simulation {
    fn furnaces_powered(&self, ids: &[EntityId]) -> bool {
        ids.iter().any(|&f| {
            self.entity(f)
                .map(|e| e.status == Status::Working)
                .unwrap_or(false)
        })
    }
}

#[test]
fn without_fuel_nothing_runs_and_boiler_caps_its_temperature() {
    let mut s = Simulation::new(12);
    let c = chain(&mut s);
    s.run(300);
    assert_eq!(s.heat_stats.heat_produced, 0);
    assert_eq!(s.energy.electric_produced, 0);
    assert_eq!(
        s.entity_at(c.boiler).unwrap().status,
        Status::NoInput,
        "chaudiere sans combustible"
    );
    // chaudiere isolee : elle chauffe jusqu'a sa temperature maximale puis s'arrete
    let mut t = Simulation::new(12);
    for y in 0..3 {
        for x in 0..2 {
            t.apply(Command::ClearFeature {
                pos: Pos::new(900 + x, 900 + y),
            });
            t.apply(Command::FillWater {
                pos: Pos::new(900 + x, 900 + y),
            });
        }
    }
    let bid = t.build_key("chaudiere", 900, 900, Dir::N).unwrap();
    let fuel = t.content.item("combustible").unwrap();
    t.debug_insert(Pos::new(900, 900), fuel, 5);
    t.run(600);
    let (temp, _, _) = t.heat_of(bid).unwrap();
    assert_eq!(temp, 500);
    assert_eq!(t.entity(bid).unwrap().status, Status::OutputFull);
    assert!(t.heat_balance_ok() && t.mass_balance_ok());
}

#[test]
fn heat_ports_limit_the_transfer() {
    let mut s = Simulation::new(13);
    let c = chain(&mut s);
    refuel(&mut s, &c);
    let mut last = s.heat_stats.heat_produced;
    for _ in 0..200 {
        let before = s.heat_stored();
        s.step();
        let produced = s.heat_stats.heat_produced - last;
        last = s.heat_stats.heat_produced;
        // jamais plus que la puissance de la chaudiere
        assert!(produced <= 1_800_000);
        assert!(s.heat_stored() <= before + produced);
    }
}

#[test]
fn drills_extract_fuel_from_resource_c() {
    let mut s = Simulation::new(14);
    // gisement de combustible garanti au sud du depart (generateur v2)
    let mut spot = None;
    'f: for y in 13..=19 {
        for x in -13..=-7 {
            if s.can_build(s.content.proto("foreuse").unwrap(), Pos::new(x, y), Dir::E)
                .is_ok()
            {
                spot = Some((x, y));
                break 'f;
            }
        }
    }
    let (x, y) = spot.expect("place pour une foreuse");
    s.build_key("foreuse", x, y, Dir::E).unwrap();
    s.build_key("coffre", x + 2, y, Dir::N).unwrap();
    s.build_key("generateur", x - 4, y - 1, Dir::N).unwrap();
    s.build_key("poteau", x - 1, y + 2, Dir::N).unwrap();
    s.run(600);
    let fuel = s.content.item("combustible").unwrap();
    let chest = s.entity_at(Pos::new(x + 2, y)).unwrap();
    match &chest.data {
        Data::Storage(st) => assert!(st.items.get(&fuel).copied().unwrap_or(0) > 0),
        _ => panic!(),
    }
    assert!(s.mass_balance_ok());
}

#[test]
fn heat_chain_survives_save_and_load_mid_run() {
    let build = || {
        let mut s = Simulation::new(15);
        let c = chain(&mut s);
        refuel(&mut s, &c);
        (s, c)
    };
    let (mut a, ca) = build();
    let (mut b, _) = build();
    a.run(1111);
    b.run(1111);
    assert_eq!(a.state_hash(), b.state_hash());
    let mut c = Simulation::load_bytes(&a.save_bytes()).unwrap();
    assert_eq!(c.state_hash(), a.state_hash());
    refuel(&mut a, &ca);
    refuel(&mut c, &ca);
    a.run(1500);
    c.run(1500);
    assert_eq!(a.state_hash(), c.state_hash());
    assert!(c.heat_balance_ok() && c.mass_balance_ok());
    // demontage d'un caloduc en route : bilans toujours justes
    let p = Pos::new(
        a.entity(ca.exchanger).unwrap().pos.x + 4,
        a.entity(ca.exchanger).unwrap().pos.y + 1,
    );
    assert!(a.remove_at(p));
    a.run(300);
    assert!(a.heat_balance_ok() && a.mass_balance_ok());
}

#[test]
fn heat_control_scene_runs() {
    let mut s = Simulation::new(42);
    crate::scenario::build_demo(&mut s).unwrap();
    crate::scenario::build_heat_demo(&mut s, 60, 40).unwrap();
    s.run(3000);
    assert!(s.heat_stats.turbine_output > 0, "{:?}", s.heat_stats);
    assert!(s.heat_balance_ok() && s.mass_balance_ok());
}

#[test]
fn version4_saves_are_converted_and_keep_their_v1_world() {
    let early: &[u8] = include_bytes!("../tests/data/save_v4_t1234.bin");
    let late: &[u8] = include_bytes!("../tests/data/save_v4_t4234.bin");
    assert_eq!(u32::from_le_bytes(early[4..8].try_into().unwrap()), 4);
    let mut s = Simulation::load_bytes(early).unwrap();
    // monde genere en v1 : il le reste (pas de combustible au sud du depart)
    assert_eq!(s.world.params.generator_version, 1);
    for y in 12..=20 {
        for x in -14..=-6 {
            assert_ne!(s.world.resource(Pos::new(x, y)).map(|r| r.0), Some(2));
        }
    }
    s.run(3000);
    let t = Simulation::load_bytes(late).unwrap();
    // la chaleur n'a rien change au reste : meme partie, electricite et fluides compris
    assert_eq!(s.state_hash(), t.state_hash());
    assert!(s.mass_balance_ok() && s.heat_balance_ok());
}
