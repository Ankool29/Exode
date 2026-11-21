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

/// Degage le terrain sous une emprise de 3 x 3 puis construit (scenes de controle).
fn put_clear(sim: &mut Simulation, key: &str, x: i32, y: i32, d: Dir) -> Result<EntityId, String> {
    use crate::sim::Command;
    for dy in 0..3 {
        for dx in 0..3 {
            let p = Pos::new(x + dx, y + dy);
            sim.apply(Command::ClearFeature { pos: p });
            sim.apply(Command::FillWater { pos: p });
        }
    }
    sim.build_key(key, x, y, d)
        .map_err(|e| format!("{key} en ({x}, {y}) : {e:?}"))
}

/// Scene de controle electrique (etape 3) : une centrale sur une sous-station, une usine de
/// fours sur une autre, reliees par un seul poteau (corridor limitant) ; plus loin, une ligne
/// de pylones qui alimente une seconde usine sans limitation.
pub fn build_power_demo(sim: &mut Simulation, ox: i32, oy: i32) -> Result<(), String> {
    let a = sim.content.item("minerai_a").unwrap();
    for (row, pylons) in [(0, false), (24, true)] {
        let y = oy + row;
        put_clear(sim, "sous_station", ox, y, Dir::N)?;
        put_clear(sim, "generateur", ox - 5, y - 1, Dir::N)?;
        put_clear(sim, "sous_station", ox + 16, y, Dir::N)?;
        if pylons {
            put_clear(sim, "pylone", ox + 8, y + 3, Dir::N)?;
        } else {
            put_clear(sim, "poteau", ox + 8, y, Dir::N)?;
        }
        for k in 0..7 {
            put_clear(sim, "four", ox + 10 + 2 * k, y - 6, Dir::N)?;
            sim.debug_insert(Pos::new(ox + 10 + 2 * k, y - 6), a, 8);
        }
        for k in 0..3 {
            put_clear(sim, "four", ox + 12 + 2 * k, y + 5, Dir::N)?;
            sim.debug_insert(Pos::new(ox + 12 + 2 * k, y + 5), a, 8);
        }
    }
    Ok(())
}

/// Scene de controle des fluides (etape 3) : reservoir plein, conduites, pompe, vanne fermee
/// puis ouverte, conduite souterraine sous des coffres, second reservoir ; poteau et
/// generateur pour la pompe.
pub fn build_fluid_demo(sim: &mut Simulation, ox: i32, oy: i32) -> Result<(), String> {
    use Dir::*;
    let water = sim.content.fluid("eau").ok_or("fluide eau")?;
    put_clear(sim, "reservoir", ox, oy, N)?;
    put_clear(sim, "conduite", ox + 3, oy + 1, N)?;
    put_clear(sim, "conduite", ox + 4, oy + 1, N)?;
    put_clear(sim, "pompe", ox + 5, oy + 1, E)?;
    put_clear(sim, "conduite", ox + 7, oy + 1, N)?;
    put_clear(sim, "conduite", ox + 7, oy + 2, N)?;
    put_clear(sim, "conduite", ox + 7, oy + 3, N)?;
    put_clear(sim, "conduite_souterraine", ox + 8, oy + 3, W)?;
    for k in 0..4 {
        put_clear(sim, "coffre", ox + 9 + k, oy + 3, N)?;
    }
    put_clear(sim, "conduite_souterraine", ox + 13, oy + 3, E)?;
    put_clear(sim, "conduite", ox + 14, oy + 3, N)?;
    put_clear(sim, "vanne", ox + 15, oy + 3, E)?;
    put_clear(sim, "conduite", ox + 16, oy + 3, N)?;
    put_clear(sim, "reservoir", ox + 17, oy + 2, N)?;
    put_clear(sim, "generateur", ox + 1, oy + 5, N)?;
    put_clear(sim, "poteau", ox + 5, oy + 3, N)?;
    sim.debug_fill(Pos::new(ox, oy), water, 5000);
    Ok(())
}
