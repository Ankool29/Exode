//! Tests du noyau : convoyeurs, bras, machines, dechets, energie, conservation,
//! determinisme, sauvegarde.

use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::scenario;
use crate::sim::Simulation;

fn sim() -> Simulation {
    Simulation::new(42)
}

fn item(s: &Simulation, k: &str) -> u16 {
    s.content.item(k).unwrap()
}

/// Objets d'un convoyeur, voie par voie.
struct Lanes {
    lanes: [Vec<BeltItem>; 2],
}

fn belt(s: &Simulation, x: i32, y: i32) -> Lanes {
    let e = s.entity_at(Pos::new(x, y)).unwrap();
    assert!(matches!(e.data, Data::Belt(_)), "pas un convoyeur");
    Lanes {
        lanes: s.belt_lanes(e.id),
    }
}

fn storage_count(s: &Simulation, x: i32, y: i32, k: &str) -> u32 {
    let it = item(s, k);
    match &s.entity_at(Pos::new(x, y)).unwrap().data {
        Data::Storage(st) => st.items.get(&it).copied().unwrap_or(0),
        _ => panic!("pas un stockage"),
    }
}

#[test]
fn belt_moves_and_stops_at_end() {
    let mut s = sim();
    for x in 0..3 {
        s.build_key("convoyeur", x, 0, Dir::E).unwrap();
    }
    let a = item(&s, "minerai_a");
    assert!(s.debug_place_on_belt(Pos::new(0, 0), 0, a, 0));
    s.run(200);
    let b = belt(&s, 2, 0);
    assert_eq!(b.lanes[0].len(), 1);
    assert_eq!(b.lanes[0][0].pos, LANE_LEN);
    assert!(s.mass_balance_ok());
}

#[test]
fn belt_compresses_to_four_per_lane_per_tile() {
    let mut s = sim();
    for x in 0..4 {
        s.build_key("convoyeur", x, 0, Dir::E).unwrap();
    }
    let a = item(&s, "minerai_a");
    for _ in 0..40 {
        s.debug_place_on_belt(Pos::new(0, 0), 1, a, 0);
        s.run(5);
    }
    s.run(400);
    // la derniere case est pleine : 4 objets, ecartes exactement de SPACING
    let b = belt(&s, 3, 0);
    assert_eq!(b.lanes[1].len(), 4);
    for w in b.lanes[1].windows(2) {
        assert_eq!(w[0].pos - w[1].pos, SPACING);
    }
    assert_eq!(b.lanes[0].len(), 0, "les voies sont independantes");
}

#[test]
fn compressed_line_flows_at_full_speed() {
    // Une ligne compacte qui se vide doit garder son ecart (pas de trous crees par l'ordre de calcul).
    let mut s = sim();
    for x in 0..6 {
        s.build_key("convoyeur", x, 0, Dir::E).unwrap();
    }
    s.build_key("convoyeur", 6, 0, Dir::S).unwrap(); // fin de ligne : virage puis sortie libre
    let a = item(&s, "minerai_a");
    for x in 0..6 {
        for k in 0..4 {
            s.debug_place_on_belt(Pos::new(x, 0), 0, a, 32 + k * 64);
        }
    }
    let before = s.belt_item_count();
    s.run(1);
    // chaque objet a avance de la vitesse complete
    let b = belt(&s, 0, 0);
    assert_eq!(b.lanes[0].last().unwrap().pos, 32 + 16);
    assert_eq!(s.belt_item_count(), before);
}

#[test]
fn curve_keeps_lanes_and_sideload_targets_near_lane() {
    let mut s = sim();
    // virage : E puis N
    s.build_key("convoyeur", 0, 0, Dir::E).unwrap();
    s.build_key("convoyeur", 1, 0, Dir::N).unwrap();
    s.build_key("convoyeur", 1, -1, Dir::N).unwrap();
    let a = item(&s, "minerai_a");
    let b_ = item(&s, "minerai_b");
    s.debug_place_on_belt(Pos::new(0, 0), 0, a, 0);
    s.debug_place_on_belt(Pos::new(0, 0), 1, b_, 0);
    s.run(100);
    let b = belt(&s, 1, -1);
    assert_eq!(
        b.lanes[0][0].item, a,
        "voie gauche conservee dans le virage"
    );
    assert_eq!(
        b.lanes[1][0].item, b_,
        "voie droite conservee dans le virage"
    );

    // insertion laterale : ligne N avec arriere, alimentee par le cote ouest
    let mut s = sim();
    s.build_key("convoyeur", 5, 1, Dir::N).unwrap();
    s.build_key("convoyeur", 5, 0, Dir::N).unwrap();
    s.build_key("convoyeur", 5, -1, Dir::N).unwrap();
    s.build_key("convoyeur", 4, 0, Dir::E).unwrap();
    s.debug_place_on_belt(Pos::new(4, 0), 0, a, 0);
    s.debug_place_on_belt(Pos::new(4, 0), 1, b_, 100);
    s.run(100);
    let b = belt(&s, 5, -1);
    // l'ouest est a gauche d'un convoyeur oriente nord : tout arrive sur la voie gauche
    assert_eq!(b.lanes[0].len(), 2);
    assert_eq!(b.lanes[1].len(), 0);
    assert!(s.mass_balance_ok());
}

#[test]
fn head_on_belts_block() {
    let mut s = sim();
    s.build_key("convoyeur", 0, 0, Dir::E).unwrap();
    s.build_key("convoyeur", 1, 0, Dir::W).unwrap();
    let a = item(&s, "minerai_a");
    s.debug_place_on_belt(Pos::new(0, 0), 0, a, 0);
    s.run(60);
    assert_eq!(belt(&s, 0, 0).lanes[0][0].pos, LANE_LEN);
}

#[test]
fn inserter_needs_power_and_moves_items() {
    let mut s = sim();
    s.build_key("coffre", 0, 0, Dir::N).unwrap();
    s.build_key("bras", 1, 0, Dir::E).unwrap();
    s.build_key("coffre", 2, 0, Dir::N).unwrap();
    let a = item(&s, "minerai_a");
    s.debug_insert(Pos::new(0, 0), a, 5);
    s.run(200);
    assert_eq!(
        storage_count(&s, 2, 0, "minerai_a"),
        0,
        "sans energie, rien ne bouge"
    );
    assert_eq!(s.entity_at(Pos::new(1, 0)).unwrap().status, Status::NoPower);

    s.build_key("generateur", 0, 3, Dir::N).unwrap();
    s.build_key("poteau", 1, 1, Dir::N).unwrap();
    s.run(300);
    assert_eq!(storage_count(&s, 2, 0, "minerai_a"), 5);
    assert!(s.mass_balance_ok());
}

#[test]
fn demo_chain_produces_gears_and_respects_mass() {
    let mut s = sim();
    scenario::build_demo(&mut s).unwrap();
    s.run(30 * 120);
    let gears = storage_count(&s, 4, -3, "engrenage");
    assert!(gears > 5, "engrenages produits : {gears}");
    let plates_b = storage_count(&s, 1, 5, "plaque_b");
    assert!(plates_b > 10, "plaques B : {plates_b}");
    assert!(storage_count(&s, -2, -6, "scories") > 0, "scories evacuees");
    assert!(s.mass_balance_ok(), "conservation de la matiere");
    assert_eq!(s.power_nets().len(), 1);
}

#[test]
fn full_waste_bin_stops_the_furnace() {
    let mut s = sim();
    scenario::build_demo(&mut s).unwrap();
    // benne de la chaine B retiree : les scories s'accumulent dans le four
    s.remove_at(Pos::new(-2, 7));
    s.run(30 * 400);
    let furnace = s.entity_at(Pos::new(-2, 4)).unwrap();
    assert_eq!(furnace.status, Status::WasteFull);
    assert!(s.mass_balance_ok());
}

#[test]
fn simulation_is_deterministic() {
    let mut a = sim();
    let mut b = sim();
    scenario::build_demo(&mut a).unwrap();
    scenario::build_demo(&mut b).unwrap();
    a.run(2000);
    b.run(2000);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn camera_does_not_change_state() {
    let mut a = sim();
    let mut b = sim();
    scenario::build_demo(&mut a).unwrap();
    scenario::build_demo(&mut b).unwrap();
    // b "regarde" des regions lointaines : generation de chunks pour l'affichage
    for i in 0..20 {
        let _ = b.world.chunk((i * 7, -i * 3));
        b.run(50);
        a.run(50);
    }
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn save_and_load_mid_run_is_identical() {
    let mut a = sim();
    scenario::build_demo(&mut a).unwrap();
    a.run(1234);
    let bytes = a.save_bytes();
    let mut b = Simulation::load_bytes(&bytes).unwrap();
    assert_eq!(a.state_hash(), b.state_hash());
    a.run(3000);
    b.run(3000);
    assert_eq!(a.state_hash(), b.state_hash());
    assert!(Simulation::load_bytes(b"pas une sauvegarde").is_err());
}

#[test]
fn mining_depletes_and_is_saved() {
    let mut s = sim();
    scenario::build_demo(&mut s).unwrap();
    s.run(600);
    let bytes = s.save_bytes();
    let mut t = Simulation::load_bytes(&bytes).unwrap();
    // la meme case du gisement a la meme quantite apres rechargement
    for y in -3..=-2 {
        for x in -11..=-10 {
            assert_eq!(
                s.world.resource(Pos::new(x, y)),
                t.world.resource(Pos::new(x, y))
            );
        }
    }
}

#[test]
fn build_rules() {
    let mut s = sim();
    s.build_key("coffre", 0, 0, Dir::N).unwrap();
    assert!(s.build_key("coffre", 0, 0, Dir::N).is_err(), "case occupee");
    assert!(
        s.build_key("foreuse", 20, 20, Dir::N).is_err(),
        "foreuse hors gisement"
    );
    assert!(
        s.build_key("coffre", 1_000_001, 0, Dir::N).is_err(),
        "hors du monde"
    );
    assert!(s.remove_at(Pos::new(0, 0)));
    assert!(s.build_key("coffre", 0, 0, Dir::N).is_ok());
}

#[test]
fn drill_drops_on_near_lane() {
    // foreuse a l'ouest d'un convoyeur qui descend vers le sud : le minerai doit arriver
    // sur la voie cote foreuse (ouest = voie droite d'un convoyeur oriente sud)
    let mut s = sim();
    s.build_key("foreuse", -11, -3, Dir::E).unwrap();
    for y in -4..=0 {
        s.build_key("convoyeur", -9, y, Dir::S).unwrap();
    }
    s.build_key("generateur", -14, 1, Dir::N).unwrap();
    s.build_key("poteau", -10, -1, Dir::N).unwrap();
    s.run(600);
    let mut lanes = [0usize; 2];
    for y in -4..=0 {
        let l = s.belt_lanes(s.entity_at(Pos::new(-9, y)).unwrap().id);
        lanes[0] += l[0].len();
        lanes[1] += l[1].len();
    }
    assert!(lanes[1] > 0, "minerai attendu sur la voie cote foreuse");
    assert_eq!(lanes[0], 0, "rien sur la voie eloignee");
}
