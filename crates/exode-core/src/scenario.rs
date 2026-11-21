//! Scenarios prets a l'emploi : demonstration de la scene P0 et charges de mesure.

use crate::entity::EntityId;
use crate::geom::{Dir, Pos};
use crate::sim::Simulation;

/// Scene P0 (GRAPHICS-01 section 8) : deux foreuses, deux fours avec bennes a scories,
/// un assembleur d'engrenages avec benne a chutes, un coffre, un generateur et des poteaux.
/// Construite pres de l'origine, sur les gisements garantis de la zone de depart.
pub fn build_demo(sim: &mut Simulation) -> Result<Vec<EntityId>, String> {
    build_demo_at(sim, 0, 0)
}

pub fn build_demo_at(sim: &mut Simulation, ox: i32, oy: i32) -> Result<Vec<EntityId>, String> {
    use Dir::*;
    let plan: &[(&str, i32, i32, Dir)] = &[
        // Chaine A : foreuse -> convoyeur -> four -> assembleur -> coffre
        ("foreuse", -11, -3, E),
        ("convoyeur", -9, -3, E),
        ("convoyeur", -8, -3, E),
        ("convoyeur", -7, -3, E),
        ("convoyeur", -6, -3, E),
        ("convoyeur", -5, -3, E),
        ("convoyeur", -4, -3, E),
        ("bras", -3, -3, E),
        ("four", -2, -4, E),
        ("bras", -2, -5, N),
        ("benne", -2, -6, N),
        ("bras", 0, -3, E),
        ("assembleur", 1, -4, E),
        ("bras", 1, -5, N),
        ("benne", 1, -6, N),
        ("bras", 3, -3, E),
        ("coffre", 4, -3, N),
        // Chaine B : foreuse -> convoyeur -> four -> coffre
        ("foreuse", -11, 5, E),
        ("convoyeur", -9, 5, E),
        ("convoyeur", -8, 5, E),
        ("convoyeur", -7, 5, E),
        ("convoyeur", -6, 5, E),
        ("convoyeur", -5, 5, E),
        ("convoyeur", -4, 5, E),
        ("bras", -3, 5, E),
        ("four", -2, 4, E),
        ("bras", -2, 6, S),
        ("benne", -2, 7, N),
        ("bras", 0, 5, E),
        ("coffre", 1, 5, N),
        // Energie (generateur provisoire et poteaux)
        ("generateur", -6, 0, N),
        ("poteau", -2, -1, N),
        ("poteau", 0, -6, N),
        ("poteau", -8, -1, N),
        ("poteau", -8, 7, N),
        ("poteau", -1, 8, N),
    ];
    let mut ids = Vec::new();
    for (key, x, y, d) in plan {
        let id = sim
            .build_key(key, ox + x, oy + y, *d)
            .map_err(|e| format!("{key} en ({}, {}) : {e:?}", ox + x, oy + y))?;
        ids.push(id);
    }
    let gear = sim.content.recipe("engrenage").unwrap();
    sim.set_recipe(Pos::new(ox + 1, oy - 4), Some(gear));
    Ok(ids)
}

/// Charge de mesure F1 : `lines` lignes droites de `length` convoyeurs, chacune alimentee
/// en continu par sa propre source (un coffre et un bras), et terminee par un coffre.
/// Les objets sont injectes directement pour isoler le cout des convoyeurs.
pub fn build_belt_bench(
    sim: &mut Simulation,
    lines: i32,
    length: i32,
    ox: i32,
    oy: i32,
) -> Result<(), String> {
    for l in 0..lines {
        let y = oy + l * 2;
        for x in 0..length {
            sim.build_key("convoyeur", ox + x, y, Dir::E)
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(())
}

/// Remplit chaque voie de chaque convoyeur au maximum (4 objets par voie et par case).
pub fn fill_belts(sim: &mut Simulation, item: u16) -> usize {
    let ids: Vec<Pos> = sim
        .entities()
        .filter(|e| matches!(e.data, crate::entity::Data::Belt(_)))
        .map(|e| e.pos)
        .collect();
    let mut n = 0;
    for p in ids {
        for lane in 0..2 {
            for k in 0..4 {
                if sim.debug_place_on_belt(p, lane, item, 32 + k * 64) {
                    n += 1;
                }
            }
        }
    }
    n
}
