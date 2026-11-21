//! Convoyeurs (banc F1) : equivalence entre les lignes de transport et l'ancienne simulation
//! case par case, boucle remplie a 100 %, lecture des sauvegardes de version 1.

use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::rng::mix64;
use crate::sim::{Command, Simulation};
use std::collections::BTreeMap;

/// Hasard deterministe pour generer les reseaux de test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix64(self.0)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

type Snapshot = BTreeMap<EntityId, [Vec<BeltItem>; 2]>;

fn snapshot(s: &Simulation) -> Snapshot {
    let ids: Vec<EntityId> = s
        .entities()
        .filter(|e| matches!(e.data, Data::Belt(_)))
        .map(|e| e.id)
        .collect();
    let mut m: Snapshot = ids.iter().map(|&id| (id, Default::default())).collect();
    s.visit_belt_items(&ids, |id, lane, it| m.get_mut(&id).unwrap()[lane].push(it));
    m
}

/// Deux simulations alimentees par les memes commandes : reference case par case et lignes.
struct Pair {
    old: Simulation,
    new: Simulation,
    compared_ticks: u64,
}

impl Pair {
    fn new(seed: u64) -> Pair {
        let mut old = Simulation::new(seed);
        old.use_reference_belts();
        Pair {
            old,
            new: Simulation::new(seed),
            compared_ticks: 0,
        }
    }

    /// Applique la meme operation aux deux simulations ; les resultats doivent etre egaux.
    fn both<R: PartialEq + std::fmt::Debug>(&mut self, f: impl Fn(&mut Simulation) -> R) -> R {
        let a = f(&mut self.old);
        let b = f(&mut self.new);
        assert_eq!(a, b, "les deux simulations divergent sur une commande");
        a
    }

    fn check(&self, what: &str) {
        let a = snapshot(&self.old);
        let b = snapshot(&self.new);
        if a == b {
            return;
        }
        for (id, la) in &a {
            let lb = &b[id];
            if la != lb {
                let e = self.old.entity(*id).unwrap();
                panic!(
                    "{what}, tick {} : convoyeur {id} en ({}, {}) {:?}\n  reference {:?}\n  lignes    {:?}",
                    self.old.tick, e.pos.x, e.pos.y, e.dir, la, lb
                );
            }
        }
        panic!("{what} : ensembles de convoyeurs differents");
    }

    /// Fait avancer les deux simulations et compare chaque objet a chaque tick.
    fn run(&mut self, ticks: u64, what: &str) {
        self.check(what);
        for t in 0..ticks {
            self.old.step();
            self.new.step();
            self.check(what);
            if t % 97 == 0 {
                assert_eq!(
                    self.old.state_hash(),
                    self.new.state_hash(),
                    "{what} : empreinte au tick {}",
                    self.old.tick
                );
            }
        }
        self.compared_ticks += ticks;
        assert_eq!(self.old.state_hash(), self.new.state_hash(), "{what}");
        assert!(
            self.old.mass_balance_ok() && self.new.mass_balance_ok(),
            "{what}"
        );
        assert_eq!(
            self.new.ring_rotations(),
            0,
            "{what} : une boucle pleine est hors du champ de l'equivalence"
        );
    }
}

/// Degage le terrain puis construit.
fn put(s: &mut Simulation, key: &str, x: i32, y: i32, d: Dir) -> bool {
    let size = match key {
        "generateur" => 3,
        "four" | "assembleur" | "recycleur" | "foreuse" => 2,
        _ => 1,
    };
    for dy in 0..size {
        for dx in 0..size {
            let p = Pos::new(x + dx, y + dy);
            s.apply(Command::ClearFeature { pos: p });
            s.apply(Command::FillWater { pos: p });
        }
    }
    s.build_key(key, x, y, d).is_ok()
}

/// Chemin de convoyeurs : segments (direction, longueur) a partir de (x, y).
fn path(p: &mut Pair, x: i32, y: i32, segs: &[(Dir, i32)]) -> (i32, i32) {
    let (mut x, mut y) = (x, y);
    for &(d, n) in segs {
        for _ in 0..n {
            assert!(
                p.both(|s| put(s, "convoyeur", x, y, d)),
                "pose en ({x}, {y})"
            );
            let (dx, dy) = d.delta();
            x += dx;
            y += dy;
        }
    }
    (x, y)
}

fn item(p: &Pair, k: &str) -> u16 {
    p.new.content.item(k).unwrap()
}

/// Generateur et grille de poteaux couvrant le rectangle.
fn power(p: &mut Pair, x0: i32, y0: i32, x1: i32, y1: i32) {
    assert!(p.both(|s| {
        let mut ok = false;
        for k in 0..20 {
            if put(s, "generateur", x0 - 6 - k * 4, y0 - 6, Dir::N) {
                ok = put(s, "poteau", x0 - 2 - k * 4, y0 - 4, Dir::N);
                for j in 0..=k {
                    put(s, "poteau", x0 - 2 - j * 4, y0 - 2, Dir::N);
                }
                break;
            }
        }
        ok
    }));
    let mut y = y0 - 2;
    while y <= y1 + 2 {
        let mut x = x0 - 2;
        while x <= x1 + 2 {
            p.both(|s| {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1), (1, 1)] {
                    if s.entity_at(Pos::new(x + dx, y + dy)).is_none()
                        && put(s, "poteau", x + dx, y + dy, Dir::N)
                    {
                        return true;
                    }
                }
                false
            });
            x += 6;
        }
        y += 6;
    }
}

/// Bras entre une case `from` et une case `to` (coffre cree si la case est libre).
fn arm(p: &mut Pair, ax: i32, ay: i32, d: Dir, chest_items: &[(u16, u32)]) {
    let (dx, dy) = d.delta();
    let src = Pos::new(ax - dx, ay - dy);
    let dst = Pos::new(ax + dx, ay + dy);
    let items = chest_items.to_vec();
    p.both(move |s| {
        if s.entity_at(Pos::new(ax, ay)).is_some() || !put(s, "bras", ax, ay, d) {
            return false;
        }
        for c in [src, dst] {
            if s.entity_at(c).is_none() {
                put(s, "coffre", c.x, c.y, Dir::N);
                for &(it, n) in &items {
                    s.debug_insert(c, it, n);
                }
            }
        }
        true
    });
}

fn fill(p: &mut Pair, x: i32, y: i32, lane: usize, it: u16, positions: &[u32]) {
    let pos = positions.to_vec();
    p.both(move |s| {
        pos.iter()
            .filter(|&&q| s.debug_place_on_belt(Pos::new(x, y), lane, it, q))
            .count()
    });
}

const COMPACT: [u32; 4] = [32, 96, 160, 224];

/// Reseau type : lignes droites, virages dans les deux sens, insertions laterales, fusions,
/// extremites, convoyeurs face a face, ligne de plus de 64 cases, boucles presque pleines,
/// spirale qui s'insere dans elle-meme, bras qui prennent et deposent.
fn typical_network(p: &mut Pair, ox: i32, oy: i32) {
    use Dir::*;
    let a = item(p, "minerai_a");
    let b = item(p, "minerai_b");
    let g = item(p, "engrenage");
    let pl = item(p, "plaque_a");
    // ligne droite qui se tasse au bout
    path(p, ox, oy, &[(E, 20)]);
    for x in 0..10 {
        fill(p, ox + x, oy, 0, a, &COMPACT);
        fill(p, ox + x, oy, 1, b, &[40, 200]);
    }
    // zigzag : virages a gauche et a droite, sortie libre vers une ligne laterale
    path(
        p,
        ox,
        oy + 3,
        &[
            (E, 5),
            (S, 3),
            (E, 4),
            (N, 5),
            (E, 3),
            (S, 2),
            (W, 2),
            (S, 3),
        ],
    );
    for x in 0..5 {
        fill(p, ox + x, oy + 3, 0, a, &COMPACT);
        fill(p, ox + x, oy + 3, 1, g, &[0, 128]);
    }
    // ligne nord avec insertions laterales des deux cotes et fusion en T
    path(p, ox + 30, oy + 12, &[(N, 12)]);
    path(p, ox + 24, oy + 9, &[(E, 6)]);
    path(p, ox + 36, oy + 7, &[(W, 6)]);
    path(p, ox + 24, oy + 5, &[(E, 6)]);
    path(p, ox + 36, oy + 5, &[(W, 6)]);
    for k in 0..6 {
        fill(p, ox + 24 + k, oy + 9, k as usize % 2, a, &COMPACT);
        fill(p, ox + 36 - k, oy + 7, 0, b, &COMPACT);
        fill(p, ox + 24 + k, oy + 5, 1, pl, &[20, 150]);
        fill(p, ox + 36 - k, oy + 5, 1, g, &COMPACT);
    }
    fill(p, ox + 30, oy + 12, 0, a, &[0, 64, 128, 192, 256]);
    // convoyeurs face a face
    path(p, ox, oy + 14, &[(E, 3)]);
    path(p, ox + 5, oy + 14, &[(W, 2)]);
    fill(p, ox, oy + 14, 0, a, &COMPACT);
    fill(p, ox + 5, oy + 14, 1, b, &COMPACT);
    // ligne de 80 cases (plusieurs lignes de transport), videe par un bras au bout
    path(p, ox, oy + 18, &[(E, 80)]);
    for x in 0..80 {
        fill(p, ox + x, oy + 18, 0, a, &COMPACT);
        if x % 3 == 0 {
            fill(p, ox + x, oy + 18, 1, b, &[10, 256]);
        }
    }
    arm(p, ox + 80, oy + 18, E, &[]);
    arm(p, ox + 40, oy + 17, S, &[(g, 30)]);
    arm(p, ox + 41, oy + 19, S, &[]);
    arm(p, ox + 70, oy + 19, N, &[(pl, 30), (a, 10)]);
    // bras d'un convoyeur a l'autre
    path(p, ox + 40, oy + 21, &[(W, 10)]);
    arm(p, ox + 35, oy + 20, S, &[]);
    // boucle 4 x 3 presque pleine (un objet manque par voie)
    let ring = [
        (ox + 50, oy, E),
        (ox + 51, oy, E),
        (ox + 52, oy, E),
        (ox + 53, oy, S),
        (ox + 53, oy + 1, S),
        (ox + 53, oy + 2, W),
        (ox + 52, oy + 2, W),
        (ox + 51, oy + 2, W),
        (ox + 50, oy + 2, N),
        (ox + 50, oy + 1, N),
    ];
    // ordre de pose different de l'ordre de marche : la tete de cycle n'est pas la premiere case
    for &(x, y, d) in ring.iter().skip(3).chain(ring.iter().take(3)) {
        assert!(p.both(|s| put(s, "convoyeur", x, y, d)));
    }
    for (i, &(x, y, _)) in ring.iter().enumerate() {
        let pos: Vec<u32> = COMPACT.iter().copied().filter(|_| i != 4).collect();
        fill(p, x, y, 0, if i % 2 == 0 { a } else { b }, &pos);
        let pos: Vec<u32> = (0..4)
            .filter(|&k| !(i == 7 && k == 2))
            .map(|k| 16 + k * 64)
            .collect();
        fill(p, x, y, 1, g, &pos);
    }
    // boucle alimentee par une insertion laterale et videe par un bras
    path(p, ox + 60, oy, &[(E, 3), (S, 3), (W, 3), (N, 3)]);
    path(p, ox + 66, oy + 1, &[(W, 3)]);
    for x in 0..3 {
        fill(p, ox + 66 - x, oy + 1, 0, b, &COMPACT);
    }
    arm(p, ox + 61, oy + 4, S, &[]);
    // spirales : la ligne s'insere sur son propre cote. Dans la seconde, la case qui insere
    // est posee en premier : c'est la tete du cycle, traitee avant le reste de sa chaine.
    path(p, ox + 70, oy + 6, &[(E, 2), (S, 1), (W, 1), (N, 1)]);
    let spiral = [(1, 1, N), (0, 0, E), (1, 0, E), (2, 0, S), (2, 1, W)];
    for &(x, y, d) in &spiral {
        assert!(p.both(|s| put(s, "convoyeur", ox + 75 + x, oy + 6 + y, d)));
    }
    for (sx, len) in [(70, 2), (75, 2)] {
        for x in 0..len {
            fill(p, ox + sx + x, oy + 6, 0, a, &COMPACT);
            fill(p, ox + sx + x, oy + 6, 1, b, &[0, 100, 256]);
        }
        fill(p, ox + sx + 1, oy + 7, 0, g, &[0, 128, 256]);
        fill(p, ox + sx + 2, oy + 7, 1, g, &[50, 200]);
    }
    power(p, ox, oy, ox + 82, oy + 22);
}

#[test]
fn lines_match_reference_on_typical_network() {
    let mut p = Pair::new(7);
    typical_network(&mut p, 200, 200);
    p.run(2500, "reseau type");
    // modifications en cours de partie : les lignes sont recalculees
    let (ox, oy) = (200, 200);
    p.both(|s| s.remove_at(Pos::new(ox + 10, oy)));
    p.both(|s| s.rotate_at(Pos::new(ox + 30, oy + 3)));
    p.both(|s| s.remove_at(Pos::new(ox + 52, oy + 2)));
    p.both(|s| put(s, "convoyeur", ox + 20, oy, Dir::S));
    p.run(600, "reseau type modifie");
    assert!(p.compared_ticks >= 2000);
}

/// Reseau genere : serpents de convoyeurs (virages, fusions, insertions, boucles, face a face),
/// objets poses n'importe ou (positions 0 et 256 comprises), bras et coffres, puis
/// modifications aleatoires en cours de partie.
fn random_network(p: &mut Pair, r: &mut Rng, ox: i32, oy: i32, size: i32) {
    let dirs = Dir::ALL;
    for _ in 0..size * size / 30 {
        let (mut x, mut y) = (r.below(size as u64) as i32, r.below(size as u64) as i32);
        let mut d = dirs[r.below(4) as usize];
        let len = 2 + r.below(30);
        for _ in 0..len {
            let (bx, by, bd) = (ox + x, oy + y, d);
            if !p.both(|s| {
                s.entity_at(Pos::new(bx, by)).is_none() && put(s, "convoyeur", bx, by, bd)
            }) {
                break;
            }
            if r.chance(20) {
                d = if r.chance(50) { d.left() } else { d.right() };
            }
            let (dx, dy) = d.delta();
            x += dx;
            y += dy;
            if x < 0 || y < 0 || x >= size || y >= size {
                break;
            }
        }
    }
    let belts: Vec<(Pos, Dir)> = p
        .new
        .entities()
        .filter(|e| matches!(e.data, Data::Belt(_)))
        .map(|e| (e.pos, e.dir))
        .collect();
    let n_items = p.new.content.items.len() as u64;
    for &(bp, _) in &belts {
        for lane in 0..2 {
            let tries = r.below(5);
            for _ in 0..tries {
                let it = r.below(n_items) as u16;
                let q = r.below(17) as u32 * 16;
                p.both(|s| s.debug_place_on_belt(bp, lane, it, q));
            }
        }
    }
    for _ in 0..belts.len() / 6 {
        let (bp, _) = belts[r.below(belts.len() as u64) as usize];
        let d = dirs[r.below(4) as usize];
        let (dx, dy) = d.delta();
        let (ax, ay) = (bp.x + dx, bp.y + dy);
        let it = r.below(n_items) as u16;
        if r.chance(50) {
            arm(p, ax, ay, d, &[]); // prend sur le convoyeur
        } else {
            arm(p, ax, ay, d.opposite(), &[(it, 20)]); // depose sur le convoyeur
        }
    }
    power(p, ox, oy, ox + size, oy + size);
}

fn random_edits(p: &mut Pair, r: &mut Rng, ox: i32, oy: i32, size: i32) {
    let belts: Vec<Pos> = p
        .new
        .entities()
        .filter(|e| matches!(e.data, Data::Belt(_)))
        .map(|e| e.pos)
        .collect();
    let n_items = p.new.content.items.len() as u64;
    for _ in 0..4 {
        let bp = belts[r.below(belts.len() as u64) as usize];
        match r.below(4) {
            0 => {
                p.both(|s| s.remove_at(bp));
            }
            1 => {
                p.both(|s| s.rotate_at(bp));
            }
            2 => {
                let (x, y) = (
                    ox + r.below(size as u64) as i32,
                    oy + r.below(size as u64) as i32,
                );
                let d = Dir::ALL[r.below(4) as usize];
                p.both(|s| s.entity_at(Pos::new(x, y)).is_none() && put(s, "convoyeur", x, y, d));
            }
            _ => {
                let it = r.below(n_items) as u16;
                let q = r.below(17) as u32 * 16;
                let lane = r.below(2) as usize;
                p.both(|s| s.debug_place_on_belt(bp, lane, it, q));
            }
        }
    }
}

#[test]
fn lines_match_reference_on_generated_networks() {
    for seed in [1u64, 2, 3] {
        let mut r = Rng(seed * 1000);
        let mut p = Pair::new(seed);
        let (ox, oy, size) = (300, -300, 36);
        random_network(&mut p, &mut r, ox, oy, size);
        let what = format!("reseau genere {seed}");
        assert!(p.new.belt_item_count() > 300, "{what} : trop peu d'objets");
        for _ in 0..8 {
            p.run(300, &what);
            random_edits(&mut p, &mut r, ox, oy, size);
        }
        assert!(p.compared_ticks >= 2000);
    }
}

/// Boucle de 4 x 3 cases (10 cases), dans un sens ou dans l'autre.
fn ring_4x3(clockwise: bool) -> Vec<(i32, i32, Dir)> {
    use Dir::*;
    let cw = vec![
        (0, 0, E),
        (1, 0, E),
        (2, 0, E),
        (3, 0, S),
        (3, 1, S),
        (3, 2, W),
        (2, 2, W),
        (1, 2, W),
        (0, 2, N),
        (0, 1, N),
    ];
    if clockwise {
        return cw;
    }
    // sens inverse : meme parcours a l'envers, directions retournees
    let mut v: Vec<(i32, i32, Dir)> = Vec::new();
    for i in 0..cw.len() {
        let (x, y, _) = cw[cw.len() - 1 - i];
        let (nx, ny, _) = cw[(2 * cw.len() - 2 - i) % cw.len()];
        let d = Dir::ALL
            .into_iter()
            .find(|d| d.delta() == (nx - x, ny - y))
            .unwrap();
        v.push((x, y, d));
    }
    v
}

#[test]
fn full_loop_circulates_and_every_item_goes_round() {
    for clockwise in [true, false] {
        let ring = ring_4x3(clockwise);
        let (ox, oy) = (-40, 40);
        let mut s = Simulation::new(42);
        let mut frozen = Simulation::new(42);
        frozen.use_reference_belts();
        for t in [&mut s, &mut frozen] {
            // pose dans un ordre quelconque
            for &(x, y, d) in ring.iter().rev() {
                assert!(put(t, "convoyeur", ox + x, oy + y, d));
            }
        }
        let a = s.content.item("minerai_a").unwrap();
        let b = s.content.item("minerai_b").unwrap();
        let mark0 = s.content.item("engrenage").unwrap();
        let mark1 = s.content.item("plaque_a").unwrap();
        for t in [&mut s, &mut frozen] {
            for (i, &(x, y, _)) in ring.iter().enumerate() {
                for k in 0..4u32 {
                    let p = Pos::new(ox + x, oy + y);
                    let i0 = if i == 0 && k == 0 { mark0 } else { a };
                    let i1 = if i == 5 && k == 3 { mark1 } else { b };
                    assert!(t.debug_place_on_belt(p, 0, i0, 32 + k * 64));
                    assert!(t.debug_place_on_belt(p, 1, i1, 32 + k * 64));
                }
            }
        }
        assert_eq!(s.belt_item_count(), 80, "boucle remplie a 100 %");
        let ids: Vec<EntityId> = ring
            .iter()
            .map(|&(x, y, _)| s.entity_at(Pos::new(ox + x, oy + y)).unwrap().id)
            .collect();
        // position d'un objet repere le long de la boucle, en unites
        let find = |s: &Simulation, lane: usize, it: u16| -> u32 {
            for (i, &id) in ids.iter().enumerate() {
                for bi in &s.belt_lanes(id)[lane] {
                    if bi.item == it {
                        return (i as u32 * LANE_LEN + bi.pos) % (10 * LANE_LEN);
                    }
                }
            }
            panic!("objet repere perdu");
        };
        let start = snapshot(&s);
        let speed = s.content.protos[s.content.proto("convoyeur").unwrap() as usize].speed;
        let lap = 10 * LANE_LEN / speed;
        let (mut p0, mut p1) = (find(&s, 0, mark0), find(&s, 1, mark1));
        let mut visited = [[false; 10]; 2];
        for _ in 0..lap {
            s.step();
            frozen.step();
            let (n0, n1) = (find(&s, 0, mark0), find(&s, 1, mark1));
            assert_eq!(
                n0,
                (p0 + speed) % (10 * LANE_LEN),
                "tous les objets avancent ensemble"
            );
            assert_eq!(n1, (p1 + speed) % (10 * LANE_LEN));
            visited[0][((n0 + LANE_LEN - 1) / LANE_LEN % 10) as usize] = true;
            visited[1][((n1 + LANE_LEN - 1) / LANE_LEN % 10) as usize] = true;
            (p0, p1) = (n0, n1);
            assert_eq!(s.belt_item_count(), 80);
            for id in &ids {
                for lane in s.belt_lanes(*id) {
                    for w in lane.windows(2) {
                        assert_eq!(w[0].pos - w[1].pos, SPACING, "ecart conserve");
                    }
                }
            }
        }
        assert!(visited.iter().flatten().all(|v| *v), "chaque case visitee");
        assert_eq!(
            snapshot(&s),
            start,
            "apres un tour, chaque objet est revenu a sa place"
        );
        assert!(s.mass_balance_ok());
        assert_eq!(s.ring_rotations(), 2 * lap as u64);
        // l'ancienne simulation reste figee (limite corrigee)
        assert_eq!(snapshot(&frozen), start);

        // un objet retire : la boucle n'est plus pleine et suit la regle ordinaire
        let before = s.ring_rotations();
        assert!(s.debug_take_from_belt(ids[3]).is_some());
        assert!(s.debug_take_from_belt(ids[3]).is_some());
        assert!(
            s.belt_lanes(ids[3]).iter().all(|l| l.len() == 3),
            "un objet par voie"
        );
        s.run(200);
        assert_eq!(
            s.ring_rotations(),
            before,
            "boucle ouverte : plus de rotation"
        );
        assert!(s.mass_balance_ok());
    }
}

#[test]
fn version1_saves_are_converted() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        (
            "demo",
            include_bytes!("../tests/data/save_v1_demo_t1234.bin"),
            include_bytes!("../tests/data/save_v1_demo_t4234.bin"),
        ),
        (
            "belts",
            include_bytes!("../tests/data/save_v1_belts_t1234.bin"),
            include_bytes!("../tests/data/save_v1_belts_t4234.bin"),
        ),
    ];
    for (name, early, late) in cases {
        assert_eq!(u32::from_le_bytes(early[4..8].try_into().unwrap()), 1);
        let mut s = Simulation::load_bytes(early).expect(name);
        assert_eq!(s.tick, 1234);
        assert!(s.mass_balance_ok(), "{name}");
        // la suite calculee par les lignes = la suite calculee par l'ancienne version
        s.run(3000);
        let t = Simulation::load_bytes(late).expect(name);
        assert_eq!(snapshot(&s), snapshot(&t), "{name}");
        // le reseau a corridors ne limite rien ici : meme partie qu'avec l'ancien calcul global
        assert_eq!(
            s.state_hash_without_power(),
            t.state_hash_without_power(),
            "{name}"
        );
        // reecrite en version 2, la partie se recharge a l'identique
        let bytes = s.save_bytes();
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            crate::sim::SAVE_VERSION
        );
        let u = Simulation::load_bytes(&bytes).unwrap();
        assert_eq!(u.state_hash(), s.state_hash(), "{name}");
        if name == "belts" {
            assert!(s.belt_item_count() > 50);
        }
    }
}

#[test]
fn blocked_lines_fall_asleep() {
    let mut s = Simulation::new(5);
    for y in 0..4 {
        for x in 0..100 {
            assert!(put(&mut s, "convoyeur", 500 + x, 500 + y * 2, Dir::E));
        }
    }
    let a = s.content.item("minerai_a").unwrap();
    crate::scenario::fill_belts(&mut s, a);
    s.run(5);
    let (lines, awake) = s.belt_line_stats();
    assert_eq!(lines, 4 * 2, "lignes de 64 cases au plus");
    assert_eq!(awake, 0, "lignes pleines et bloquees : rien a calculer");
    // un objet retire au bout reveille la ligne et ses voisines amont
    let end = s.entity_at(Pos::new(599, 500)).unwrap().id;
    assert!(s.debug_take_from_belt(end).is_some());
    s.step();
    assert!(s.belt_line_stats().1 > 0);
    s.run(200);
    assert_eq!(s.belt_line_stats().1, 0);
    assert!(s.mass_balance_ok());
}
