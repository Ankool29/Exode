//! Pont GDExtension : expose le noyau EXODE a Godot sous la forme d'une classe `ExodeSim`.
//!
//! Regles (cahier v0.4 section 9) :
//! - l'interface envoie des commandes ; elle ne modifie jamais l'etat directement ;
//! - le rendu lit des vues de la zone visible ;
//! - les coordonnees de rendu sont relatives a une origine proche de la camera (origine
//!   flottante), pour eviter les tremblements loin de l'origine du monde.

use exode_core::content::ProtoKind;
use exode_core::entity::{ArmState, Data, LANE_LEN};
use exode_core::world::{Chunk, CHUNK, CHUNK_AREA, NO_RESOURCE};
use exode_core::{scenario, Dir, Pos, Simulation, TICKS_PER_SECOND};
use godot::prelude::*;
use std::collections::BTreeMap;
use std::time::Instant;

struct ExodeExtension;

#[gdextension]
unsafe impl ExtensionLibrary for ExodeExtension {}

#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct ExodeSim {
    sim: Simulation,
    last_step_ms: f64,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for ExodeSim {
    fn init(base: Base<RefCounted>) -> Self {
        ExodeSim {
            sim: Simulation::new(1),
            last_step_ms: 0.0,
            base,
        }
    }
}

fn dir_of(d: i64) -> Dir {
    Dir::from_index(d)
}

fn strings(v: impl IntoIterator<Item = String>) -> PackedStringArray {
    let mut out = PackedStringArray::new();
    for s in v {
        out.push(&GString::from(s.as_str()));
    }
    out
}

#[godot_api]
impl ExodeSim {
    // ------------------------------------------------------------ partie

    /// Nouvelle partie. `infinite` : mode ressources infinies.
    #[func]
    fn new_game(&mut self, seed: i64, infinite: bool) {
        self.sim =
            Simulation::with_content(exode_core::Content::load_default(), seed as u64, infinite);
    }

    /// Construit la scene de demonstration P0 pres de l'origine.
    #[func]
    fn build_demo(&mut self) -> bool {
        match scenario::build_demo(&mut self.sim) {
            Ok(_) => true,
            Err(e) => {
                godot_warn!("scene de demonstration : {e}");
                false
            }
        }
    }

    #[func]
    fn step(&mut self, ticks: i64) {
        let t = Instant::now();
        for _ in 0..ticks.max(0) {
            self.sim.step();
        }
        if ticks > 0 {
            self.last_step_ms = t.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        }
    }

    #[func]
    fn last_step_ms(&self) -> f64 {
        self.last_step_ms
    }
    #[func]
    fn tick(&self) -> i64 {
        self.sim.tick as i64
    }
    #[func]
    fn ticks_per_second(&self) -> i64 {
        TICKS_PER_SECOND as i64
    }
    #[func]
    fn seed(&self) -> i64 {
        self.sim.world.params.seed as i64
    }
    #[func]
    fn state_hash(&self) -> GString {
        GString::from(format!("{:016x}", self.sim.state_hash()).as_str())
    }
    #[func]
    fn mass_balance_ok(&self) -> bool {
        self.sim.mass_balance_ok()
    }
    #[func]
    fn entity_count(&self) -> i64 {
        self.sim.entity_count() as i64
    }
    #[func]
    fn belt_item_count(&self) -> i64 {
        self.sim.belt_item_count() as i64
    }
    /// Libere les chunks regenerables (protection memoire, decision 08).
    #[func]
    fn drop_unmodified_chunks(&mut self) -> i64 {
        self.sim.world.drop_unmodified() as i64
    }
    #[func]
    fn loaded_chunks(&self) -> i64 {
        self.sim.world.loaded_chunks() as i64
    }

    // ------------------------------------------------------------ contenu

    #[func]
    fn proto_keys(&self) -> PackedStringArray {
        strings(self.sim.content.protos.iter().map(|p| p.key.clone()))
    }
    #[func]
    fn proto_names(&self) -> PackedStringArray {
        strings(self.sim.content.protos.iter().map(|p| p.name.clone()))
    }
    /// Type de chaque batiment : belt, inserter, drill, machine, chest, waste_bin, pole, generator.
    #[func]
    fn proto_kinds(&self) -> PackedStringArray {
        strings(self.sim.content.protos.iter().map(|p| {
            match p.kind {
                ProtoKind::Belt => "belt",
                ProtoKind::Inserter => "inserter",
                ProtoKind::Drill => "drill",
                ProtoKind::Machine => "machine",
                ProtoKind::Chest => "chest",
                ProtoKind::WasteBin => "waste_bin",
                ProtoKind::Pole => "pole",
                ProtoKind::Generator => "generator",
            }
            .to_string()
        }))
    }
    /// Largeur et hauteur de chaque batiment, a plat : [w0, h0, w1, h1, ...].
    #[func]
    fn proto_sizes(&self) -> PackedInt32Array {
        let mut v = PackedInt32Array::new();
        for p in &self.sim.content.protos {
            v.push(p.w);
            v.push(p.h);
        }
        v
    }
    #[func]
    fn proto_supply_radius(&self, proto: i64) -> i64 {
        self.sim
            .content
            .protos
            .get(proto as usize)
            .map(|p| p.supply_radius as i64)
            .unwrap_or(0)
    }
    #[func]
    fn item_keys(&self) -> PackedStringArray {
        strings(self.sim.content.items.iter().map(|i| i.key.clone()))
    }
    #[func]
    fn item_names(&self) -> PackedStringArray {
        strings(self.sim.content.items.iter().map(|i| i.name.clone()))
    }
    #[func]
    fn item_colors(&self) -> PackedStringArray {
        strings(self.sim.content.items.iter().map(|i| i.color.clone()))
    }
    #[func]
    fn item_is_waste(&self) -> PackedInt32Array {
        let mut v = PackedInt32Array::new();
        for i in &self.sim.content.items {
            v.push(i.waste as i32);
        }
        v
    }
    #[func]
    fn resource_colors(&self) -> PackedStringArray {
        strings(self.sim.content.resources.iter().map(|r| r.color.clone()))
    }
    #[func]
    fn recipe_keys(&self) -> PackedStringArray {
        strings(self.sim.content.recipes.iter().map(|r| r.key.clone()))
    }
    #[func]
    fn recipe_categories(&self) -> PackedStringArray {
        strings(self.sim.content.recipes.iter().map(|r| r.category.clone()))
    }
    /// Categorie de recettes d'un batiment ("" si aucune).
    #[func]
    fn proto_category(&self, proto: i64) -> GString {
        let c = self
            .sim
            .content
            .protos
            .get(proto as usize)
            .and_then(|p| p.category.clone())
            .unwrap_or_default();
        GString::from(c.as_str())
    }
    /// Description lisible d'une recette : "4 Minerai A -> 4 Plaque A + 1 Scories".
    #[func]
    fn recipe_label(&self, recipe: i64) -> GString {
        let c = &self.sim.content;
        let Some(r) = c.recipes.get(recipe as usize) else {
            return GString::new();
        };
        let fmt = |l: &Vec<(u16, u32)>| {
            l.iter()
                .map(|(i, n)| format!("{n} {}", c.items[*i as usize].name))
                .collect::<Vec<_>>()
                .join(" + ")
        };
        GString::from(format!("{} -> {}", fmt(&r.inputs), fmt(&r.outputs)).as_str())
    }

    // ------------------------------------------------------------ terrain

    /// Donnees d'un chunk de 32 x 32 cases, 4 plans de 1024 octets :
    /// sol, obstacle, type de ressource (255 = aucune), richesse (0 = vide, 1 a 3).
    #[func]
    fn chunk_data(&mut self, cx: i64, cy: i64) -> PackedByteArray {
        let c: &Chunk = self.sim.world.chunk((cx as i32, cy as i32));
        let mut v = Vec::with_capacity(CHUNK_AREA * 4);
        v.extend_from_slice(&c.ground);
        v.extend_from_slice(&c.feature);
        v.extend_from_slice(&c.res_kind);
        for (k, a) in c.res_kind.iter().zip(c.res_amount.iter()) {
            let bucket = if *k == NO_RESOURCE || *a == 0 {
                0
            } else if *a < 400 {
                1
            } else if *a < 900 {
                2
            } else {
                3
            };
            v.push(bucket);
        }
        PackedByteArray::from(v.as_slice())
    }

    #[func]
    fn chunk_size(&self) -> i64 {
        CHUNK as i64
    }

    // ------------------------------------------------------------ commandes

    #[func]
    fn can_build(&mut self, proto: i64, x: i64, y: i64, dir: i64) -> bool {
        self.sim
            .can_build(proto as u16, Pos::new(x as i32, y as i32), dir_of(dir))
            .is_ok()
    }
    /// Raison d'un refus de construction, ou "" si la pose est possible.
    #[func]
    fn build_error(&mut self, proto: i64, x: i64, y: i64, dir: i64) -> GString {
        match self
            .sim
            .can_build(proto as u16, Pos::new(x as i32, y as i32), dir_of(dir))
        {
            Ok(()) => GString::new(),
            Err(e) => GString::from(match e {
                exode_core::sim::BuildError::UnknownProto => "Batiment inconnu",
                exode_core::sim::BuildError::OutOfWorld => "Hors des limites du monde",
                exode_core::sim::BuildError::Blocked(_) => "Terrain bloque (eau, arbre ou rocher)",
                exode_core::sim::BuildError::Occupied(_) => "Case occupee",
                exode_core::sim::BuildError::NoResource => "Pas de gisement sous la foreuse",
            }),
        }
    }
    /// Pose un batiment. Renvoie son identifiant, ou -1.
    #[func]
    fn build(&mut self, proto: i64, x: i64, y: i64, dir: i64) -> i64 {
        self.sim
            .build(proto as u16, Pos::new(x as i32, y as i32), dir_of(dir))
            .map(|id| id as i64)
            .unwrap_or(-1)
    }
    #[func]
    fn remove_at(&mut self, x: i64, y: i64) -> bool {
        self.sim.remove_at(Pos::new(x as i32, y as i32))
    }
    #[func]
    fn rotate_at(&mut self, x: i64, y: i64) -> bool {
        self.sim.rotate_at(Pos::new(x as i32, y as i32))
    }
    #[func]
    fn clear_feature(&mut self, x: i64, y: i64) -> bool {
        self.sim.apply(exode_core::Command::ClearFeature {
            pos: Pos::new(x as i32, y as i32),
        })
    }
    #[func]
    fn fill_water(&mut self, x: i64, y: i64) -> bool {
        self.sim.apply(exode_core::Command::FillWater {
            pos: Pos::new(x as i32, y as i32),
        })
    }
    /// Choisit la recette d'une machine (-1 = aucune).
    #[func]
    fn set_recipe_at(&mut self, x: i64, y: i64, recipe: i64) -> bool {
        let r = if recipe < 0 {
            None
        } else {
            Some(recipe as u16)
        };
        self.sim.set_recipe(Pos::new(x as i32, y as i32), r)
    }

    // ------------------------------------------------------------ vues pour le rendu

    /// Entites touchant le rectangle de cases [x0, x1] x [y0, y1], 10 entiers par entite :
    /// id, batiment, x, y, largeur, hauteur, direction, etat, extra1, extra2.
    /// extra1/extra2 : convoyeur = virage (0 droit, 1 depuis la gauche, 2 depuis la droite) ;
    /// bras = objet tenu (-1) et avancement signe (pour mille, negatif au retour) ;
    /// machine = recette (-1) et avancement pour mille ; foreuse = -1 et avancement.
    #[func]
    fn entities_in_rect(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> PackedInt32Array {
        let mut v = PackedInt32Array::new();
        let (x0, y0, x1, y1) = (x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        for e in self.sim.entities() {
            if e.pos.x > x1 || e.pos.y > y1 || e.pos.x + e.w - 1 < x0 || e.pos.y + e.h - 1 < y0 {
                continue;
            }
            let (ex1, ex2) = match &e.data {
                Data::Belt(b) => {
                    let c = match b.curve_from {
                        Some(side) if side == e.dir.left() => 1,
                        Some(_) => 2,
                        None => 0,
                    };
                    (c, 0)
                }
                Data::Inserter(i) => {
                    let half = self.sim.content.protos[e.proto as usize].speed.max(1) as i32 * 1000;
                    let p = (i.timer as i32 * 1000 / half).min(1000);
                    let prog = match i.state {
                        ArmState::Idle => 0,
                        ArmState::ToDrop => p,
                        ArmState::Dropping => 1000,
                        ArmState::Returning => -(1000 - p).max(1),
                    };
                    (i.held.map(|h| h as i32).unwrap_or(-1), prog)
                }
                Data::Machine(m) => {
                    let prog = match (m.recipe, m.running) {
                        (Some(r), true) => {
                            let t = self.sim.content.recipes[r as usize].time as u64 * 1000;
                            (m.progress * 1000 / t.max(1)) as i32
                        }
                        _ => 0,
                    };
                    (m.recipe.map(|r| r as i32).unwrap_or(-1), prog)
                }
                Data::Drill(d) => {
                    let period =
                        self.sim.content.protos[e.proto as usize].speed.max(1) as u64 * 1000;
                    (-1, (d.progress * 1000 / period) as i32)
                }
                _ => (-1, 0),
            };
            for x in [
                e.id as i32,
                e.proto as i32,
                e.pos.x,
                e.pos.y,
                e.w,
                e.h,
                e.dir.index() as i32,
                e.status as i32,
                ex1,
                ex2,
            ] {
                v.push(x);
            }
        }
        v
    }

    /// Objets sur les convoyeurs du rectangle, 3 flottants par objet : x, y (en cases,
    /// relatifs a l'origine ox, oy) et type d'objet. Virages compris.
    #[func]
    fn belt_items_in_rect(
        &self,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        ox: i64,
        oy: i64,
    ) -> PackedFloat32Array {
        let mut v = PackedFloat32Array::new();
        let (x0, y0, x1, y1) = (x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        const LANE_OFF: f64 = 12.0 / 64.0;
        // convoyeurs visibles, puis leurs objets (chaque ligne de transport lue une fois)
        let mut visible: BTreeMap<u32, (Pos, Dir, Option<Dir>)> = BTreeMap::new();
        for e in self.sim.entities() {
            let Data::Belt(b) = &e.data else { continue };
            if e.pos.x < x0 || e.pos.x > x1 || e.pos.y < y0 || e.pos.y > y1 {
                continue;
            }
            visible.insert(e.id, (e.pos, e.dir, b.curve_from));
        }
        let ids: Vec<u32> = visible.keys().copied().collect();
        self.sim.visit_belt_items(&ids, |id, lane, it| {
            let (pos, dir, curve_from) = visible[&id];
            let cx = (pos.x as i64 - ox) as f64 + 0.5;
            let cy = (pos.y as i64 - oy) as f64 + 0.5;
            let (dx, dy) = dir.delta();
            let (dx, dy) = (dx as f64, dy as f64);
            let (lx, ly) = {
                let (a, b) = dir.left().delta();
                (a as f64, b as f64)
            };
            let t = it.pos as f64 / LANE_LEN as f64;
            let (px, py) = match curve_from {
                None => {
                    let s = if lane == 0 { LANE_OFF } else { -LANE_OFF };
                    (cx + dx * (t - 0.5) + lx * s, cy + dy * (t - 0.5) + ly * s)
                }
                Some(side) => {
                    let (ox_, oy_) = side.delta();
                    let (ox_, oy_) = (ox_ as f64, oy_ as f64);
                    let pivx = cx + (ox_ + dx) * 0.5;
                    let pivy = cy + (oy_ + dy) * 0.5;
                    let a0 = (-dy).atan2(-dx);
                    let a1 = (-oy_).atan2(-ox_);
                    let mut da = a1 - a0;
                    while da > std::f64::consts::PI {
                        da -= std::f64::consts::TAU;
                    }
                    while da < -std::f64::consts::PI {
                        da += std::f64::consts::TAU;
                    }
                    let sign = lx * -ox_ + ly * -oy_;
                    let r = if lane == 0 {
                        0.5 + LANE_OFF * sign
                    } else {
                        0.5 - LANE_OFF * sign
                    };
                    let a = a0 + da * t;
                    (pivx + a.cos() * r, pivy + a.sin() * r)
                }
            };
            v.push(px as f32);
            v.push(py as f32);
            v.push(it.item as f32);
        });
        v
    }

    /// Liaisons entre poteaux du rectangle : x1, y1, x2, y2 (cases).
    #[func]
    fn pole_links_in_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) -> PackedInt32Array {
        let links: Vec<_> = self.sim.pole_links().to_vec();
        let mut v = PackedInt32Array::new();
        for (a, b) in links {
            let (Some(ea), Some(eb)) = (self.sim.entity(a), self.sim.entity(b)) else {
                continue;
            };
            let inside = |p: Pos| {
                p.x >= x0 as i32 && p.x <= x1 as i32 && p.y >= y0 as i32 && p.y <= y1 as i32
            };
            if inside(ea.pos) || inside(eb.pos) {
                for x in [ea.pos.x, ea.pos.y, eb.pos.x, eb.pos.y] {
                    v.push(x);
                }
            }
        }
        v
    }

    /// Fiche d'une entite pour le panneau d'information (vide si rien sur la case).
    #[func]
    fn entity_info(&self, x: i64, y: i64) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(e) = self.sim.entity_at(Pos::new(x as i32, y as i32)) else {
            return d;
        };
        let c = &self.sim.content;
        let proto = &c.protos[e.proto as usize];
        let item_list = |m: &std::collections::BTreeMap<u16, u32>| {
            m.iter()
                .map(|(i, n)| format!("{} x{}", c.items[*i as usize].name, n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        d.set("id", e.id as i64);
        d.set("proto", e.proto as i64);
        d.set("name", &GString::from(proto.name.as_str()));
        d.set("x", e.pos.x as i64);
        d.set("y", e.pos.y as i64);
        d.set("status", e.status as i64);
        d.set("power", self.sim.satisfaction_of(e.id) as i64);
        d.set(
            "needs_power",
            proto.power > 0 && proto.kind != ProtoKind::Generator,
        );
        d.set(
            "category",
            &GString::from(proto.category.clone().unwrap_or_default().as_str()),
        );
        let (recipe, contents) = match &e.data {
            Data::Machine(m) => (
                m.recipe.map(|r| r as i64).unwrap_or(-1),
                format!(
                    "Entrees : {}\nSorties : {}",
                    item_list(&m.inputs),
                    item_list(&m.outputs)
                ),
            ),
            Data::Storage(s) => (
                -1,
                format!(
                    "Contenu ({}/{}) : {}",
                    s.total(),
                    proto.capacity,
                    item_list(&s.items)
                ),
            ),
            Data::Belt(_) => {
                let lanes = self.sim.belt_lanes(e.id);
                (
                    -1,
                    format!(
                        "Voie gauche : {} objets, voie droite : {} objets",
                        lanes[0].len(),
                        lanes[1].len()
                    ),
                )
            }
            Data::Drill(dr) => (
                -1,
                format!(
                    "Tampon : {}",
                    dr.buffer
                        .map(|i| c.items[i as usize].name.clone())
                        .unwrap_or("vide".into())
                ),
            ),
            Data::Inserter(i) => (
                -1,
                format!(
                    "Tient : {}",
                    i.held
                        .map(|i| c.items[i as usize].name.clone())
                        .unwrap_or("rien".into())
                ),
            ),
            _ => (-1, String::new()),
        };
        d.set("recipe", recipe);
        d.set("contents", &GString::from(contents.as_str()));
        d
    }

    /// Resume electrique : production, demande (W) et satisfaction minimale (pour mille).
    #[func]
    fn power_summary(&self) -> PackedInt64Array {
        let nets = self.sim.power_nets();
        let supply: u64 = nets.iter().map(|n| n.0).sum();
        let demand: u64 = nets.iter().map(|n| n.1).sum();
        let sat = nets.iter().map(|n| n.2).min().unwrap_or(0);
        PackedInt64Array::from(&[supply as i64, demand as i64, sat as i64, nets.len() as i64][..])
    }

    /// Nombre d'objets produits depuis le debut (compteurs exacts).
    #[func]
    fn produced(&self, item: i64) -> i64 {
        self.sim
            .stats
            .produced
            .get(&(item as u16))
            .copied()
            .unwrap_or(0) as i64
    }

    // ------------------------------------------------------------ sauvegarde

    #[func]
    fn save_bytes(&self) -> PackedByteArray {
        PackedByteArray::from(self.sim.save_bytes().as_slice())
    }

    #[func]
    fn load_bytes(&mut self, bytes: PackedByteArray) -> bool {
        match Simulation::load_bytes(bytes.as_slice()) {
            Ok(s) => {
                self.sim = s;
                true
            }
            Err(e) => {
                godot_warn!("chargement impossible : {e}");
                false
            }
        }
    }
}
