//! Reseaux de fluides : sens impose par la pompe, vanne, reservoir qui sature, melange
//! refuse, conduite souterraine, bilan matiere, sauvegarde.

use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::sim::{BuildError, Command, Simulation};
use crate::world::Ground;

fn clear(s: &mut Simulation, x: i32, y: i32, w: i32, h: i32) {
    for dy in 0..h {
        for dx in 0..w {
            let p = Pos::new(x + dx, y + dy);
            s.apply(Command::ClearFeature { pos: p });
            s.apply(Command::FillWater { pos: p });
        }
    }
}

fn put(s: &mut Simulation, key: &str, x: i32, y: i32, d: Dir) -> EntityId {
    clear(s, x, y, 3, 3);
    s.build_key(key, x, y, d)
        .unwrap_or_else(|e| panic!("{key} en ({x}, {y}) : {e:?}"))
}

fn amount(s: &Simulation, x: i32, y: i32) -> u64 {
    let id = s.entity_at(Pos::new(x, y)).unwrap().id;
    s.fluid_of(id).unwrap().0.amount
}

/// Reservoir A (0,0) - conduites - appareil au milieu (5,1) - conduites - reservoir B (9,0),
/// avec un generateur et un poteau.
fn line(s: &mut Simulation, ox: i32, oy: i32, middle: &str) {
    put(s, "reservoir", ox, oy, Dir::N);
    put(s, "conduite", ox + 3, oy + 1, Dir::N);
    put(s, "conduite", ox + 4, oy + 1, Dir::N);
    put(s, middle, ox + 5, oy + 1, Dir::E);
    let after = if middle == "pompe" { 7 } else { 6 };
    for x in after..9 {
        put(s, "conduite", ox + x, oy + 1, Dir::N);
    }
    put(s, "reservoir", ox + 9, oy, Dir::N);
    put(s, "generateur", ox + 2, oy + 4, Dir::N);
    put(s, "poteau", ox + 5, oy + 3, Dir::N);
}

#[test]
fn pump_imposes_direction() {
    let mut s = Simulation::new(5);
    let (ox, oy) = (300, 300);
    line(&mut s, ox, oy, "pompe");
    let water = s.content.fluid("eau").unwrap();
    assert_eq!(s.debug_fill(Pos::new(ox, oy), water, 3000), 3000);
    s.run(300);
    let (a, b) = (amount(&s, ox, oy), amount(&s, ox + 9, oy));
    assert!(b > 2000, "la pompe a vide A dans B : A={a} B={b}");
    assert_eq!(
        s.entity_at(Pos::new(ox + 5, oy + 1)).unwrap().status,
        Status::NoInput,
        "A vide : la pompe attend"
    );
    // B plein, A presque vide : rien ne revient vers A malgre la difference de niveau
    // (A garde un reste d'arrondi de l'egalisation reservoir-conduite)
    let rest = amount(&s, ox, oy);
    assert!(rest < 50);
    s.debug_fill(Pos::new(ox + 9, oy), water, 5000);
    s.run(300);
    assert!(amount(&s, ox, oy) <= rest, "pas de retour a contre-sens");
    assert!(s.mass_balance_ok());
    // sans electricite, la pompe ne pompe pas
    let mut t = Simulation::new(5);
    line(&mut t, ox, oy, "pompe");
    t.remove_at(Pos::new(ox + 5, oy + 3));
    t.debug_fill(Pos::new(ox, oy), water, 3000);
    t.run(100);
    assert_eq!(amount(&t, ox + 9, oy), 0);
    assert_eq!(
        t.entity_at(Pos::new(ox + 5, oy + 1)).unwrap().status,
        Status::NoPower
    );
}

#[test]
fn closed_valve_blocks() {
    let mut s = Simulation::new(6);
    let (ox, oy) = (300, 300);
    line(&mut s, ox, oy, "vanne");
    let water = s.content.fluid("eau").unwrap();
    s.debug_fill(Pos::new(ox, oy), water, 4000);
    // vanne fermee : rien ne passe
    assert!(s.apply(Command::Toggle {
        pos: Pos::new(ox + 5, oy + 1)
    }));
    s.run(300);
    assert_eq!(amount(&s, ox + 9, oy), 0, "vanne fermee");
    // ouverte : les niveaux s'egalisent dans le sens de la vanne
    s.apply(Command::Toggle {
        pos: Pos::new(ox + 5, oy + 1),
    });
    s.run(600);
    let (a, b) = (amount(&s, ox, oy), amount(&s, ox + 9, oy));
    assert!(b > 1500 && a.abs_diff(b) < 200, "egalise : A={a} B={b}");
    // sens unique : plus rien de B vers A meme si B deborde
    let before = amount(&s, ox, oy);
    s.debug_fill(Pos::new(ox + 9, oy), water, 5000);
    s.run(200);
    assert!(amount(&s, ox, oy) <= before);
    assert!(s.mass_balance_ok());
}

/// Case d'eau dont les deux cases au nord sont du sol constructible.
fn find_shore(s: &mut Simulation) -> (i32, i32) {
    for r in 30..600 {
        for k in 0..(8 * r) {
            let (x, y) = match k / (2 * r) {
                0 => (-r + k % (2 * r), -r),
                1 => (r, -r + k % (2 * r)),
                2 => (r - k % (2 * r), r),
                _ => (-r, r - k % (2 * r)),
            };
            let land = |s: &mut Simulation, dy: i32| {
                let g = s.world.ground(Pos::new(x, y - dy));
                g != Ground::Water
            };
            if s.world.ground(Pos::new(x, y)) == Ground::Water && land(s, 1) && land(s, 2) {
                // toute la colonne au nord doit etre libre d'eau sur 12 cases
                if (1..14).all(|d| land(s, d)) {
                    return (x, y);
                }
            }
        }
    }
    panic!("pas de lac");
}

#[test]
fn water_pump_fills_tank_until_it_saturates() {
    let mut s = Simulation::new(7);
    let (wx, wy) = find_shore(&mut s);
    // pompe a eau tournee vers le nord : prise d'eau en (wx, wy)
    for dy in 1..14 {
        s.apply(Command::ClearFeature {
            pos: Pos::new(wx, wy - dy),
        });
    }
    let wp = s.build_key("pompe_eau", wx, wy - 2, Dir::N).unwrap();
    assert!(
        s.build_key("pompe_eau", wx + 40, wy - 40, Dir::N).is_err(),
        "loin de l'eau"
    );
    for dy in 3..9 {
        s.build_key("conduite", wx, wy - dy, Dir::N).unwrap();
    }
    // reservoir 3 x 3 dont le raccord sud est en (wx, wy - 8)
    clear(&mut s, wx - 1, wy - 11, 3, 3);
    s.build_key("reservoir", wx - 1, wy - 11, Dir::N).unwrap();
    let tank = s.entity_at(Pos::new(wx, wy - 10)).unwrap().id;
    let mut last = 0;
    let mut grew = 0;
    for _ in 0..40 {
        s.run(10);
        let a = s.fluid_of(tank).unwrap().0.amount;
        if a > last {
            grew += 1;
        }
        last = a;
    }
    assert!(grew > 30, "le reservoir se remplit");
    s.run(1200);
    let (b, cap) = s.fluid_of(tank).unwrap();
    assert_eq!(b.amount, cap, "reservoir plein");
    assert_eq!(b.temp, 15);
    assert_eq!(s.entity(tank).unwrap().status, Status::OutputFull);
    assert_eq!(
        s.entity(wp).unwrap().status,
        Status::OutputFull,
        "pompe bloquee"
    );
    let extracted = s.stats.mass_extracted;
    s.run(300);
    assert_eq!(s.stats.mass_extracted, extracted, "plus rien n'entre");
    assert!(s.mass_balance_ok());
}

#[test]
fn fluids_never_mix_and_segments_split_by_capacity() {
    let mut s = Simulation::new(8);
    let (ox, oy) = (500, 500);
    for x in 0..5 {
        put(&mut s, "conduite", ox + x, oy, Dir::N);
        put(&mut s, "conduite", ox + x + 6, oy, Dir::N);
    }
    let water = s.content.fluid("eau").unwrap();
    let steam = s.content.fluid("vapeur").unwrap();
    s.debug_fill(Pos::new(ox, oy), water, 300);
    s.debug_fill(Pos::new(ox + 6, oy), steam, 200);
    assert_eq!(
        s.build_key("conduite", ox + 5, oy, Dir::N),
        Err(BuildError::FluidMix)
    );
    // segment de 5 conduites (500) : retrait du milieu, 60 retires, 120 de chaque cote
    assert!(s.remove_at(Pos::new(ox + 2, oy)));
    assert_eq!(amount(&s, ox, oy), 120);
    assert_eq!(amount(&s, ox + 4, oy), 120);
    assert_eq!(
        s.segment_of(s.entity_at(Pos::new(ox + 4, oy)).unwrap().id)
            .unwrap()
            .0,
        s.entity_at(Pos::new(ox + 3, oy)).unwrap().id,
        "identifiant = plus petit"
    );
    // reunion : les volumes s'additionnent
    put(&mut s, "conduite", ox + 2, oy, Dir::N);
    assert_eq!(amount(&s, ox, oy), 240);
    assert!(s.mass_balance_ok());
}

#[test]
fn underground_pipe_connects_through_obstacles() {
    let mut s = Simulation::new(9);
    let (ox, oy) = (700, 700);
    put(&mut s, "conduite", ox, oy, Dir::N);
    put(&mut s, "conduite_souterraine", ox + 1, oy, Dir::W);
    for x in 2..7 {
        put(&mut s, "coffre", ox + x, oy, Dir::N);
    }
    put(&mut s, "conduite_souterraine", ox + 7, oy, Dir::E);
    put(&mut s, "conduite", ox + 8, oy, Dir::N);
    let a = s.entity_at(Pos::new(ox, oy)).unwrap().id;
    let b = s.entity_at(Pos::new(ox + 8, oy)).unwrap().id;
    assert_eq!(
        s.segment_of(a),
        s.segment_of(b),
        "meme segment par le tunnel"
    );
    assert_eq!(s.segment_of(a).unwrap().1, 4);
    assert_eq!(s.pipe_mask(a), 2, "raccord vers l'est seulement");
    assert!(
        !s.rotate_at(Pos::new(ox + 1, oy)),
        "on ne tourne pas un tunnel"
    );
}

fn fluid_world() -> Simulation {
    let mut s = Simulation::new(10);
    line(&mut s, 300, 300, "pompe");
    line(&mut s, 300, 320, "vanne");
    let water = s.content.fluid("eau").unwrap();
    s.debug_fill(Pos::new(300, 300), water, 5000);
    s.debug_fill(Pos::new(300, 320), water, 5000);
    s
}

#[test]
fn fluids_conserve_matter_and_survive_save_and_load() {
    let mut a = fluid_world();
    let mut b = fluid_world();
    for t in 0..3000u64 {
        a.step();
        b.step();
        if t == 700 {
            for s in [&mut a, &mut b] {
                s.remove_at(Pos::new(304, 301));
                s.apply(Command::Toggle {
                    pos: Pos::new(305, 321),
                });
            }
        }
        if t == 1500 {
            for s in [&mut a, &mut b] {
                s.build_key("conduite", 304, 301, Dir::N).unwrap();
            }
        }
        if t % 250 == 0 {
            assert!(a.mass_balance_ok(), "bilan au tick {t}");
        }
    }
    assert_eq!(a.state_hash(), b.state_hash());
    let mut c = Simulation::load_bytes(&a.save_bytes()).unwrap();
    assert_eq!(c.state_hash(), a.state_hash());
    a.run(500);
    c.run(500);
    assert_eq!(a.state_hash(), c.state_hash());
    assert!(c.mass_balance_ok());
}

#[test]
fn version3_saves_are_converted() {
    let early: &[u8] = include_bytes!("../tests/data/save_v3_t1234.bin");
    let late: &[u8] = include_bytes!("../tests/data/save_v3_t4234.bin");
    assert_eq!(u32::from_le_bytes(early[4..8].try_into().unwrap()), 3);
    let mut s = Simulation::load_bytes(early).unwrap();
    s.run(3000);
    let t = Simulation::load_bytes(late).unwrap();
    // les fluides n'ont rien change au reste : meme partie, electricite comprise
    assert_eq!(s.state_hash(), t.state_hash());
    assert!(s.mass_balance_ok());
}
