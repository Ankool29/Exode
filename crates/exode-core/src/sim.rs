//! Simulation a pas fixe, deterministe, identique a l'ecran et hors ecran (v0.4 section 8).
//!
//! Ordre d'un tick :
//! 1. calcul de la satisfaction electrique a partir des demandes du tick precedent ;
//! 2. foreuses et machines (ordre des identifiants) ;
//! 3. bras mecaniques (ordre des identifiants) ;
//! 4. convoyeurs, de l'aval vers l'amont, regroupes en lignes de transport (`belts.rs`).
//!
//! Les commandes du joueur s'appliquent entre deux ticks, dans l'ordre ou elles arrivent.

use crate::belts::{self, BeltBackend, LineBelts, Topology};
use crate::content::{Content, ItemId, ProtoId, ProtoKind, RecipeId};
use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::world::{World, WorldParams, GENERATOR_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasherDefault;

/// Rythme provisoire de simulation (v0.4, decision 04 [P]).
pub const TICKS_PER_SECOND: u32 = 30;
pub const SAVE_MAGIC: &[u8; 4] = b"EXOD";
/// Version 2 : objets des convoyeurs ranges a part, case par case (lignes de transport).
/// Les sauvegardes de version 1 sont converties au chargement.
pub const SAVE_VERSION: u32 = 2;

/// Hacheur deterministe (FNV-1a) pour les tables de recherche. Ces tables ne sont jamais
/// parcourues : l'ordre de traitement vient toujours des BTreeMap.
#[derive(Default, Clone, Copy)]
pub struct Fnv(u64);
impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        self.0 = h;
    }
}
pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<Fnv>>;

pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    /// Compteurs exacts (servent aux jalons, v0.4 5.4).
    pub produced: BTreeMap<ItemId, u64>,
    pub consumed: BTreeMap<ItemId, u64>,
    /// Bilan matiere, en grammes.
    pub mass_extracted: u64,
    pub mass_lost: u64,
    pub mass_removed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    UnknownProto,
    OutOfWorld,
    Blocked(Pos),
    Occupied(Pos),
    NoResource,
}

#[derive(Debug, Clone)]
pub enum Command {
    Build { proto: ProtoId, pos: Pos, dir: Dir },
    Remove { pos: Pos },
    Rotate { pos: Pos },
    SetRecipe { pos: Pos, recipe: Option<RecipeId> },
    ClearFeature { pos: Pos },
    FillWater { pos: Pos },
}

#[derive(Debug, Clone, Default)]
struct PowerNet {
    supply: u64,
    demand: u64,
    /// Satisfaction en pour mille.
    satisfaction: u32,
}

/// Objets d'un convoyeur dans la sauvegarde : voie gauche, voie droite, de l'avant vers l'arriere.
type BeltLanes = [Vec<BeltItem>; 2];

#[derive(Serialize, Deserialize)]
struct SaveState {
    params: WorldParams,
    tick: u64,
    next_id: EntityId,
    entities: Vec<Entity>,
    /// Convoyeurs non vides, dans l'ordre des identifiants.
    belt_items: Vec<(EntityId, BeltLanes)>,
    chunks: Vec<((i32, i32), crate::world::Chunk)>,
    stats: Stats,
}

/// Format de la version 1 (lecture seule) : les objets etaient dans les donnees du convoyeur.
mod v1 {
    use super::*;

    #[derive(Deserialize)]
    pub struct SaveState {
        pub params: WorldParams,
        pub tick: u64,
        pub next_id: EntityId,
        pub entities: Vec<Entity>,
        pub chunks: Vec<((i32, i32), crate::world::Chunk)>,
        pub stats: Stats,
    }

    #[derive(Deserialize)]
    pub struct Belt {
        pub lanes: BeltLanes,
    }

    /// Meme ordre de variantes que `Data` (bincode les numerote).
    #[derive(Deserialize)]
    pub enum Data {
        Belt(Belt),
        Inserter(Inserter),
        Drill(Drill),
        Machine(Machine),
        Storage(Storage),
        Pole,
        Generator,
    }

    #[derive(Deserialize)]
    pub struct Entity {
        pub id: EntityId,
        pub proto: ProtoId,
        pub pos: Pos,
        pub dir: Dir,
        pub w: i32,
        pub h: i32,
        pub data: Data,
        pub status: Status,
        pub wants_power: bool,
    }

    /// Conversion vers le format courant.
    pub fn convert(st: SaveState) -> super::SaveState {
        let mut entities = Vec::with_capacity(st.entities.len());
        let mut belt_items = Vec::new();
        for e in st.entities {
            let data = match e.data {
                Data::Belt(b) => {
                    if !b.lanes[0].is_empty() || !b.lanes[1].is_empty() {
                        belt_items.push((e.id, b.lanes));
                    }
                    super::Data::Belt(super::Belt::default())
                }
                Data::Inserter(x) => super::Data::Inserter(x),
                Data::Drill(x) => super::Data::Drill(x),
                Data::Machine(x) => super::Data::Machine(x),
                Data::Storage(x) => super::Data::Storage(x),
                Data::Pole => super::Data::Pole,
                Data::Generator => super::Data::Generator,
            };
            entities.push(super::Entity {
                id: e.id,
                proto: e.proto,
                pos: e.pos,
                dir: e.dir,
                w: e.w,
                h: e.h,
                data,
                status: e.status,
                wants_power: e.wants_power,
            });
        }
        super::SaveState {
            params: st.params,
            tick: st.tick,
            next_id: st.next_id,
            entities,
            belt_items,
            chunks: st.chunks,
            stats: st.stats,
        }
    }
}

/// Representation des objets sur les convoyeurs.
enum BeltStore {
    Lines(LineBelts),
    /// Ancienne simulation case par case, pour le test d'equivalence.
    #[cfg(test)]
    Reference(crate::belts_ref::RefBelts),
}

macro_rules! belts {
    ($store:expr, $b:ident => $e:expr) => {
        match $store {
            BeltStore::Lines($b) => $e,
            #[cfg(test)]
            BeltStore::Reference($b) => $e,
        }
    };
}

pub struct Simulation {
    pub content: Content,
    pub world: World,
    pub tick: u64,
    next_id: EntityId,
    entities: BTreeMap<EntityId, Entity>,
    occ: FastMap<Pos, EntityId>,
    pub stats: Stats,
    belts: BeltStore,
    /// Objets lus dans une sauvegarde, poses a la prochaine reconstruction des lignes.
    pending_belt_items: Vec<(EntityId, BeltLanes)>,
    // caches
    belts_dirty: bool,
    power_dirty: bool,
    nets: Vec<PowerNet>,
    net_of: FastMap<EntityId, usize>,
    pole_links: Vec<(EntityId, EntityId)>,
    /// Entites raccordees hors poteaux : (id, reseau, puissance, generateur).
    powered: Vec<(EntityId, usize, u64, bool)>,
    /// Foreuses et machines, puis bras, dans l'ordre des identifiants.
    lists_dirty: bool,
    producer_ids: Vec<EntityId>,
    inserter_ids: Vec<EntityId>,
}

#[derive(Clone, Copy)]
enum TargetKind {
    WasteBin,
    Chest,
    Other,
}

/// Ce qu'une destination accepte.
enum Want {
    Nothing,
    Any,
    WasteOnly,
    Items(Vec<ItemId>),
}

impl Want {
    fn accepts(&self, item: ItemId, content: &Content) -> bool {
        match self {
            Want::Nothing => false,
            Want::Any => true,
            Want::WasteOnly => content.items[item as usize].waste,
            Want::Items(v) => v.contains(&item),
        }
    }
}

impl Simulation {
    pub fn new(seed: u64) -> Simulation {
        Simulation::with_content(Content::load_default(), seed, false)
    }

    pub fn with_content(content: Content, seed: u64, infinite_resources: bool) -> Simulation {
        let params = WorldParams {
            seed,
            generator_version: GENERATOR_VERSION,
            infinite_resources,
        };
        Simulation {
            content,
            world: World::new(params),
            tick: 0,
            next_id: 1,
            entities: BTreeMap::new(),
            occ: FastMap::default(),
            stats: Stats::default(),
            belts: BeltStore::Lines(LineBelts::new()),
            pending_belt_items: Vec::new(),
            belts_dirty: true,
            power_dirty: true,
            nets: Vec::new(),
            net_of: FastMap::default(),
            pole_links: Vec::new(),
            powered: Vec::new(),
            lists_dirty: true,
            producer_ids: Vec::new(),
            inserter_ids: Vec::new(),
        }
    }

    /// Remplace les lignes de transport par l'ancienne simulation case par case (tests).
    #[cfg(test)]
    pub(crate) fn use_reference_belts(&mut self) {
        assert_eq!(
            self.belt_item_count(),
            0,
            "a faire avant de poser des objets"
        );
        self.belts = BeltStore::Reference(Default::default());
        self.belts_dirty = true;
    }

    /// Nombre de rotations de boucles pleines (tests).
    #[cfg(test)]
    pub(crate) fn ring_rotations(&self) -> u64 {
        match &self.belts {
            BeltStore::Lines(l) => l.ring_rotations,
            BeltStore::Reference(_) => 0,
        }
    }

    /// Lignes de transport (diagnostic) : (lignes, voies eveillees).
    pub fn belt_line_stats(&self) -> (usize, usize) {
        match &self.belts {
            BeltStore::Lines(l) => (l.line_count(), l.awake_lanes()),
            #[cfg(test)]
            BeltStore::Reference(_) => (0, 0),
        }
    }

    // ------------------------------------------------------------------ acces

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }
    pub fn entity_at(&self, p: Pos) -> Option<&Entity> {
        self.occ.get(&p).and_then(|id| self.entities.get(id))
    }
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
    pub fn pole_links(&mut self) -> &[(EntityId, EntityId)] {
        self.refresh_power();
        &self.pole_links
    }
    /// Satisfaction electrique (pour mille) d'une entite, 0 si non raccordee.
    pub fn satisfaction_of(&self, id: EntityId) -> u32 {
        self.net_of
            .get(&id)
            .map(|n| self.nets[*n].satisfaction)
            .unwrap_or(0)
    }
    /// (production W, demande W, satisfaction pour mille) de chaque reseau.
    pub fn power_nets(&self) -> Vec<(u64, u64, u32)> {
        self.nets
            .iter()
            .map(|n| (n.supply, n.demand, n.satisfaction))
            .collect()
    }

    fn proto_kind(&self, e: &Entity) -> ProtoKind {
        self.content.protos[e.proto as usize].kind
    }

    // ------------------------------------------------------------------ construction

    fn footprint(&self, proto: ProtoId, dir: Dir) -> (i32, i32) {
        let p = &self.content.protos[proto as usize];
        match dir {
            Dir::E | Dir::W => (p.h, p.w),
            _ => (p.w, p.h),
        }
    }

    /// Verifie une pose sans rien modifier (sert aussi au fantome de construction).
    pub fn can_build(&mut self, proto: ProtoId, pos: Pos, dir: Dir) -> Result<(), BuildError> {
        if proto as usize >= self.content.protos.len() {
            return Err(BuildError::UnknownProto);
        }
        let (w, h) = self.footprint(proto, dir);
        let mut has_resource = false;
        for dy in 0..h {
            for dx in 0..w {
                let p = pos.offset(dx, dy);
                if !p.in_world() {
                    return Err(BuildError::OutOfWorld);
                }
                if self.occ.contains_key(&p) {
                    return Err(BuildError::Occupied(p));
                }
                if !self.world.buildable(p) {
                    return Err(BuildError::Blocked(p));
                }
                if self.world.resource(p).is_some() {
                    has_resource = true;
                }
            }
        }
        if self.content.protos[proto as usize].kind == ProtoKind::Drill && !has_resource {
            return Err(BuildError::NoResource);
        }
        Ok(())
    }

    pub fn build(&mut self, proto: ProtoId, pos: Pos, dir: Dir) -> Result<EntityId, BuildError> {
        self.can_build(proto, pos, dir)?;
        let (w, h) = self.footprint(proto, dir);
        let kind = self.content.protos[proto as usize].kind;
        let data = match kind {
            ProtoKind::Belt => Data::Belt(Belt::default()),
            ProtoKind::Inserter => Data::Inserter(Inserter::default()),
            ProtoKind::Drill => Data::Drill(Drill::default()),
            ProtoKind::Machine => Data::Machine(Machine::default()),
            ProtoKind::Chest | ProtoKind::WasteBin => Data::Storage(Storage::default()),
            ProtoKind::Pole => Data::Pole,
            ProtoKind::Generator => Data::Generator,
        };
        let id = self.next_id;
        self.next_id += 1;
        let e = Entity {
            id,
            proto,
            pos,
            dir,
            w,
            h,
            data,
            status: Status::Idle,
            wants_power: false,
        };
        for t in e.tiles() {
            self.occ.insert(t, id);
        }
        self.entities.insert(id, e);
        if kind == ProtoKind::Belt {
            self.belts_dirty = true;
        }
        self.power_dirty = true;
        self.lists_dirty = true;
        Ok(id)
    }

    pub fn build_key(
        &mut self,
        key: &str,
        x: i32,
        y: i32,
        dir: Dir,
    ) -> Result<EntityId, BuildError> {
        let proto = self.content.proto(key).ok_or(BuildError::UnknownProto)?;
        self.build(proto, Pos::new(x, y), dir)
    }

    /// Retire l'entite presente sur la case. Le contenu est compte comme retire
    /// (en attendant l'inventaire du joueur et la deconstruction logistique).
    pub fn remove_at(&mut self, p: Pos) -> bool {
        let Some(id) = self.occ.get(&p).copied() else {
            return false;
        };
        let e = self.entities.remove(&id).expect("occupation incoherente");
        for t in e.tiles() {
            self.occ.remove(&t);
        }
        self.stats.mass_removed += self.entity_mass(&e);
        if matches!(e.data, Data::Belt(_)) {
            let items = belts!(&mut self.belts, b => b.remove_tile(id));
            let c = &self.content;
            self.stats.mass_removed += items.iter().map(|i| c.items[*i as usize].mass).sum::<u64>();
            self.belts_dirty = true;
        }
        self.power_dirty = true;
        self.lists_dirty = true;
        true
    }

    pub fn rotate_at(&mut self, p: Pos) -> bool {
        let Some(id) = self.occ.get(&p).copied() else {
            return false;
        };
        let e = self.entities.get_mut(&id).unwrap();
        if e.w != e.h {
            return false;
        }
        e.dir = e.dir.cw();
        if matches!(e.data, Data::Belt(_)) {
            self.belts_dirty = true;
        }
        true
    }

    pub fn set_recipe(&mut self, p: Pos, recipe: Option<RecipeId>) -> bool {
        let Some(id) = self.occ.get(&p).copied() else {
            return false;
        };
        let cat = {
            let e = &self.entities[&id];
            self.content.protos[e.proto as usize].category.clone()
        };
        if let Some(r) = recipe {
            if Some(&self.content.recipes[r as usize].category) != cat.as_ref() {
                return false;
            }
        }
        // Les entrees non utilisees et le travail en cours sont rendus comme retires.
        let mut removed = 0;
        let e = self.entities.get_mut(&id).unwrap();
        if let Data::Machine(m) = &mut e.data {
            if m.recipe == recipe {
                return true;
            }
            for (i, n) in std::mem::take(&mut m.inputs) {
                removed += self.content.items[i as usize].mass * n as u64;
            }
            if m.running {
                if let Some(r) = m.recipe {
                    removed += self
                        .content
                        .stack_mass(&self.content.recipes[r as usize].inputs);
                }
            }
            m.running = false;
            m.progress = 0;
            m.recipe = recipe;
        } else {
            return false;
        }
        self.stats.mass_removed += removed;
        true
    }

    pub fn apply(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Build { proto, pos, dir } => self.build(proto, pos, dir).is_ok(),
            Command::Remove { pos } => self.remove_at(pos),
            Command::Rotate { pos } => self.rotate_at(pos),
            Command::SetRecipe { pos, recipe } => self.set_recipe(pos, recipe),
            Command::ClearFeature { pos } => {
                !self.occ.contains_key(&pos) && self.world.clear_feature(pos)
            }
            Command::FillWater { pos } => {
                !self.occ.contains_key(&pos) && self.world.fill_water(pos)
            }
        }
    }

    /// Ajout direct d'objets (outil de developpement et tests).
    pub fn debug_insert(&mut self, p: Pos, item: ItemId, n: u32) -> u32 {
        let Some(id) = self.occ.get(&p).copied() else {
            return 0;
        };
        let mut done = 0;
        for _ in 0..n {
            if self.insert_into(id, item, Dir::S, false) {
                done += 1;
            }
        }
        // objets crees hors extraction : on les compte comme extraits pour garder le bilan
        self.stats.mass_extracted += self.content.items[item as usize].mass * done as u64;
        done
    }

    /// Pose directe d'un objet sur une voie de convoyeur (tests et bancs de mesure).
    pub fn debug_place_on_belt(&mut self, p: Pos, lane: usize, item: ItemId, pos: u32) -> bool {
        let Some(id) = self.occ.get(&p).copied() else {
            return false;
        };
        if !matches!(self.entities[&id].data, Data::Belt(_)) {
            return false;
        }
        self.refresh_belts();
        let ok = belts!(&mut self.belts, b => b.insert_at(id, lane, item, pos.min(LANE_LEN)));
        if ok {
            self.stats.mass_extracted += self.content.items[item as usize].mass;
        }
        ok
    }

    /// Retire l'objet le plus en avant d'un convoyeur, compte comme retire (tests).
    #[cfg(test)]
    pub(crate) fn debug_take_from_belt(&mut self, id: EntityId) -> Option<ItemId> {
        self.refresh_belts();
        let it = belts!(&mut self.belts, b => b.take(id, &|_| true))?;
        self.stats.mass_removed += self.content.items[it as usize].mass;
        Some(it)
    }

    /// Nombre total d'objets sur les convoyeurs.
    pub fn belt_item_count(&self) -> usize {
        belts!(&self.belts, b => b.item_count())
    }

    /// Objets d'un convoyeur : voie gauche, voie droite, de l'avant vers l'arriere.
    pub fn belt_lanes(&self, id: EntityId) -> [Vec<BeltItem>; 2] {
        belts!(&self.belts, b => b.lanes(id))
    }

    /// Objets des convoyeurs demandes : (convoyeur, voie, objet). Chaque ligne de transport
    /// n'est parcourue qu'une fois (affichage de la zone visible).
    pub fn visit_belt_items(&self, ids: &[EntityId], mut f: impl FnMut(EntityId, usize, BeltItem)) {
        belts!(&self.belts, b => b.visit(ids, &mut f))
    }

    // ------------------------------------------------------------------ bilan matiere

    fn entity_mass(&self, e: &Entity) -> u64 {
        let c = &self.content;
        let m = |i: ItemId| c.items[i as usize].mass;
        match &e.data {
            // objets des convoyeurs : comptes a part (lignes de transport)
            Data::Belt(_) => 0,
            Data::Inserter(ins) => ins.held.map(m).unwrap_or(0),
            Data::Drill(d) => d.buffer.map(m).unwrap_or(0),
            Data::Machine(mc) => {
                let mut s: u64 = mc
                    .inputs
                    .iter()
                    .map(|(i, n)| m(*i) * *n as u64)
                    .sum::<u64>()
                    + mc.outputs
                        .iter()
                        .map(|(i, n)| m(*i) * *n as u64)
                        .sum::<u64>();
                if mc.running {
                    if let Some(r) = mc.recipe {
                        s += c.stack_mass(&c.recipes[r as usize].inputs);
                    }
                }
                s
            }
            Data::Storage(st) => st.items.iter().map(|(i, n)| m(*i) * *n as u64).sum(),
            Data::Pole | Data::Generator => 0,
        }
    }

    /// Masse de matiere presente dans les installations (grammes).
    pub fn mass_in_world(&self) -> u64 {
        let c = &self.content;
        let mut belt_mass = 0u64;
        belts!(&self.belts, b => b.for_each_item(&mut |i| belt_mass += c.items[i as usize].mass));
        self.entities
            .values()
            .map(|e| self.entity_mass(e))
            .sum::<u64>()
            + belt_mass
    }

    /// Bilan de conservation : extrait = present + perdu + retire.
    pub fn mass_balance_ok(&self) -> bool {
        self.stats.mass_extracted
            == self.mass_in_world() + self.stats.mass_lost + self.stats.mass_removed
    }

    // ------------------------------------------------------------------ energie

    fn refresh_power(&mut self) {
        if !self.power_dirty {
            return;
        }
        self.power_dirty = false;
        let poles: Vec<(EntityId, Pos, i32, i32)> = self
            .entities
            .values()
            .filter(|e| self.proto_kind(e) == ProtoKind::Pole)
            .map(|e| {
                let p = &self.content.protos[e.proto as usize];
                (e.id, e.pos, p.supply_radius, p.wire_reach)
            })
            .collect();
        // union-find sur les poteaux
        let n = poles.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        self.pole_links.clear();
        for i in 0..n {
            for j in 0..i {
                let (dx, dy) = (poles[i].1.x - poles[j].1.x, poles[i].1.y - poles[j].1.y);
                let reach = poles[i].3.min(poles[j].3);
                if dx * dx + dy * dy <= reach * reach {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                    self.pole_links.push((poles[j].0, poles[i].0));
                }
            }
        }
        let mut root_to_net: BTreeMap<usize, usize> = BTreeMap::new();
        self.nets.clear();
        self.net_of.clear();
        let mut pole_net = vec![0usize; n];
        for (i, pn) in pole_net.iter_mut().enumerate() {
            let r = find(&mut parent, i);
            let next = root_to_net.len();
            *pn = *root_to_net.entry(r).or_insert(next);
        }
        self.nets = vec![PowerNet::default(); root_to_net.len()];
        for (i, p) in poles.iter().enumerate() {
            self.net_of.insert(p.0, pole_net[i]);
        }
        // raccordement des autres entites : premier poteau (ordre des id) dont la zone touche l'emprise
        self.powered.clear();
        for e in self.entities.values() {
            let pk = &self.content.protos[e.proto as usize];
            let consumer = pk.power > 0 && pk.kind != ProtoKind::Pole;
            if !consumer {
                continue;
            }
            for (i, p) in poles.iter().enumerate() {
                let r = p.2;
                let (x0, y0, x1, y1) = (p.1.x - r, p.1.y - r, p.1.x + r, p.1.y + r);
                let overlap = e.pos.x <= x1
                    && e.pos.x + e.w - 1 >= x0
                    && e.pos.y <= y1
                    && e.pos.y + e.h - 1 >= y0;
                if overlap {
                    self.net_of.insert(e.id, pole_net[i]);
                    self.powered.push((
                        e.id,
                        pole_net[i],
                        pk.power,
                        pk.kind == ProtoKind::Generator,
                    ));
                    break;
                }
            }
        }
    }

    fn update_power(&mut self) {
        self.refresh_power();
        for n in &mut self.nets {
            n.supply = 0;
            n.demand = 0;
        }
        // entites raccordees seulement (les poteaux ne consomment rien)
        for &(id, net, power, generator) in &self.powered {
            if generator {
                self.nets[net].supply += power;
            } else if self.entities[&id].wants_power {
                self.nets[net].demand += power;
            }
        }
        for n in &mut self.nets {
            n.satisfaction = if n.demand == 0 {
                if n.supply > 0 {
                    1000
                } else {
                    0
                }
            } else {
                ((n.supply * 1000) / n.demand).min(1000) as u32
            };
        }
    }

    // ------------------------------------------------------------------ transferts

    /// Ce qu'accepte l'entite `id` pour un objet arrivant.
    fn want_of(&self, id: EntityId) -> Want {
        let Some(e) = self.entities.get(&id) else {
            return Want::Nothing;
        };
        let pk = &self.content.protos[e.proto as usize];
        match &e.data {
            Data::Belt(_) => Want::Any,
            Data::Storage(st) => {
                if st.total() >= pk.capacity {
                    Want::Nothing
                } else if pk.kind == ProtoKind::WasteBin {
                    Want::WasteOnly
                } else {
                    Want::Any
                }
            }
            Data::Machine(m) => {
                let cat = pk.category.as_deref().unwrap_or("");
                let empty = m.inputs.values().all(|n| *n == 0) && !m.running;
                if pk.auto_recipe && (m.recipe.is_none() || empty) {
                    let v = self
                        .content
                        .recipes
                        .iter()
                        .filter(|r| r.category == cat && r.inputs.len() == 1)
                        .map(|r| r.inputs[0].0)
                        .collect();
                    return Want::Items(v);
                }
                let Some(r) = m.recipe else {
                    return Want::Nothing;
                };
                let rec = &self.content.recipes[r as usize];
                let v = rec
                    .inputs
                    .iter()
                    .filter(|(i, n)| m.inputs.get(i).copied().unwrap_or(0) < n * 2)
                    .map(|(i, _)| *i)
                    .collect::<Vec<_>>();
                if v.is_empty() {
                    Want::Nothing
                } else {
                    Want::Items(v)
                }
            }
            _ => Want::Nothing,
        }
    }

    /// Depose un objet dans une entite. `approach` = sens de deplacement de l'objet.
    /// Sur un convoyeur : `near` = voie la plus proche de la source (foreuses, qui deposent
    /// juste devant elles) ; sinon voie eloignee (bras, qui deposent de l'autre cote).
    fn insert_into(&mut self, id: EntityId, item: ItemId, approach: Dir, near: bool) -> bool {
        if !self.want_of(id).accepts(item, &self.content) {
            return false;
        }
        let auto = {
            let e = &self.entities[&id];
            let pk = &self.content.protos[e.proto as usize];
            if pk.auto_recipe {
                pk.category.clone()
            } else {
                None
            }
        };
        let auto_recipe = auto.and_then(|cat| self.content.auto_recipe_for(&cat, item));
        let e = self.entities.get_mut(&id).unwrap();
        match &mut e.data {
            Data::Belt(_) => {
                let bd = e.dir;
                let from_side = approach.opposite();
                // voie 0 = gauche dans le sens de marche, voie 1 = droite
                let lane = if from_side == bd.left() {
                    if near { 0 } else { 1 }
                } else if from_side == bd.right() {
                    if near { 1 } else { 0 }
                } else if from_side == bd.opposite() {
                    1
                } else {
                    0
                };
                self.refresh_belts();
                belts!(&mut self.belts, b => b.insert_at(id, lane, item, MID))
            }
            Data::Storage(st) => {
                st.add(item, 1);
                true
            }
            Data::Machine(m) => {
                if let Some(r) = auto_recipe {
                    let empty = m.inputs.values().all(|n| *n == 0) && !m.running;
                    if m.recipe != Some(r) && empty {
                        m.inputs.clear();
                        m.recipe = Some(r);
                        m.progress = 0;
                    }
                }
                *m.inputs.entry(item).or_insert(0) += 1;
                true
            }
            _ => false,
        }
    }

    /// Prend un objet accepte par `want` dans l'entite `id`.
    ///
    /// Regle provisoire des dechets [R] : depuis une machine, un bras ne met des dechets
    /// que dans une benne ou sur un convoyeur ; un coffre ordinaire ne recoit que des produits.
    fn take_from(&mut self, id: EntityId, want: &Want, target: TargetKind) -> Option<ItemId> {
        if matches!(self.entities.get(&id)?.data, Data::Belt(_)) {
            // objet le plus en avant accepte, voie 0 prioritaire en cas d'egalite
            self.refresh_belts();
            let content = &self.content;
            return belts!(&mut self.belts, b => b.take(id, &|i| want.accepts(i, content)));
        }
        let content = &self.content;
        let e = self.entities.get_mut(&id)?;
        match &mut e.data {
            Data::Belt(_) => None,
            Data::Storage(st) => {
                let item = st
                    .items
                    .keys()
                    .copied()
                    .find(|i| want.accepts(*i, content))?;
                st.take(item);
                Some(item)
            }
            Data::Machine(m) => {
                let pick = |waste: bool| {
                    m.outputs
                        .iter()
                        .find(|(i, n)| {
                            **n > 0
                                && content.items[**i as usize].waste == waste
                                && want.accepts(**i, content)
                        })
                        .map(|(i, _)| *i)
                };
                let item = match target {
                    TargetKind::WasteBin => pick(true),
                    TargetKind::Chest => pick(false),
                    TargetKind::Other => pick(false).or_else(|| pick(true)),
                }?;
                let c = m.outputs.get_mut(&item).unwrap();
                *c -= 1;
                if *c == 0 {
                    m.outputs.remove(&item);
                }
                Some(item)
            }
            Data::Drill(d) => {
                let item = d.buffer?;
                if want.accepts(item, content) {
                    d.buffer = None;
                    Some(item)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------------ tick

    pub fn step(&mut self) {
        self.update_power();
        self.refresh_lists();
        self.refresh_belts();
        let producers = std::mem::take(&mut self.producer_ids);
        for &id in &producers {
            match self.entities[&id].data {
                Data::Drill(_) => self.update_drill(id),
                Data::Machine(_) => self.update_machine(id),
                _ => {}
            }
        }
        self.producer_ids = producers;
        let inserters = std::mem::take(&mut self.inserter_ids);
        for &id in &inserters {
            self.update_inserter(id);
        }
        self.inserter_ids = inserters;
        self.update_belts();
        self.tick += 1;
    }

    /// Listes des entites actives, dans l'ordre des identifiants.
    fn refresh_lists(&mut self) {
        if !self.lists_dirty {
            return;
        }
        self.lists_dirty = false;
        self.producer_ids.clear();
        self.inserter_ids.clear();
        for e in self.entities.values() {
            match e.data {
                Data::Drill(_) | Data::Machine(_) => self.producer_ids.push(e.id),
                Data::Inserter(_) => self.inserter_ids.push(e.id),
                _ => {}
            }
        }
    }

    pub fn run(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.step();
        }
    }

    fn update_drill(&mut self, id: EntityId) {
        let s = self.satisfaction_of(id);
        let (front, dir, tiles, period) = {
            let e = &self.entities[&id];
            (
                e.front_tile(),
                e.dir,
                e.tiles().collect::<Vec<_>>(),
                self.content.protos[e.proto as usize].speed as u64,
            )
        };
        // sortie bloquee ?
        let buffer = match &self.entities[&id].data {
            Data::Drill(d) => d.buffer,
            _ => None,
        };
        if let Some(item) = buffer {
            let delivered = self
                .occ
                .get(&front)
                .copied()
                .map(|t| self.insert_into(t, item, dir, true))
                .unwrap_or(false);
            let e = self.entities.get_mut(&id).unwrap();
            if delivered {
                if let Data::Drill(d) = &mut e.data {
                    d.buffer = None;
                }
            } else {
                e.status = Status::OutputFull;
                e.wants_power = false;
                return;
            }
        }
        let source = tiles
            .iter()
            .copied()
            .find(|t| self.world.resource(*t).is_some());
        let Some(tile) = source else {
            let e = self.entities.get_mut(&id).unwrap();
            e.status = Status::NoResource;
            e.wants_power = false;
            return;
        };
        let mut mined = None;
        {
            let e = self.entities.get_mut(&id).unwrap();
            e.wants_power = true;
            e.status = if s == 0 {
                Status::NoPower
            } else if s < 1000 {
                Status::LowPower
            } else {
                Status::Working
            };
            if let Data::Drill(d) = &mut e.data {
                d.progress += s as u64;
                if d.progress >= period * 1000 {
                    d.progress -= period * 1000;
                    mined = Some(());
                }
            }
        }
        if mined.is_some() {
            if let Some(kind) = self.world.mine_one(tile) {
                let item = self.content.resources[kind as usize].item;
                self.stats.mass_extracted += self.content.items[item as usize].mass;
                *self.stats.produced.entry(item).or_insert(0) += 1;
                let delivered = self
                    .occ
                    .get(&front)
                    .copied()
                    .map(|t| self.insert_into(t, item, dir, true))
                    .unwrap_or(false);
                if !delivered {
                    if let Data::Drill(d) = &mut self.entities.get_mut(&id).unwrap().data {
                        d.buffer = Some(item);
                    }
                }
            }
        }
    }

    fn output_cap(&self, item: ItemId, amount: u32) -> u32 {
        if self.content.items[item as usize].waste {
            amount * 5
        } else {
            (amount * 4).max(8)
        }
    }

    fn update_machine(&mut self, id: EntityId) {
        let s = self.satisfaction_of(id);
        let (recipe, speed) = {
            let e = &self.entities[&id];
            let Data::Machine(m) = &e.data else { return };
            (m.recipe, self.content.protos[e.proto as usize].speed as u64)
        };
        let Some(r) = recipe else {
            let e = self.entities.get_mut(&id).unwrap();
            e.status = Status::NoRecipe;
            e.wants_power = false;
            return;
        };
        let rec = self.content.recipes[r as usize].clone();
        let caps: Vec<(ItemId, u32, bool)> = rec
            .outputs
            .iter()
            .map(|(i, n)| {
                (
                    *i,
                    self.output_cap(*i, *n),
                    self.content.items[*i as usize].waste,
                )
            })
            .collect();
        let loss = self.content.recipe_loss(r);
        let e = self.entities.get_mut(&id).unwrap();
        let Data::Machine(m) = &mut e.data else {
            return;
        };

        if !m.running {
            // place en sortie ?
            for (k, (item, n)) in rec.outputs.iter().enumerate() {
                let have = m.outputs.get(item).copied().unwrap_or(0);
                if have + n > caps[k].1 {
                    e.status = if caps[k].2 {
                        Status::WasteFull
                    } else {
                        Status::OutputFull
                    };
                    e.wants_power = false;
                    return;
                }
            }
            let enough = rec
                .inputs
                .iter()
                .all(|(i, n)| m.inputs.get(i).copied().unwrap_or(0) >= *n);
            if !enough {
                e.status = Status::NoInput;
                e.wants_power = false;
                return;
            }
            for (i, n) in &rec.inputs {
                let c = m.inputs.get_mut(i).unwrap();
                *c -= n;
                if *c == 0 {
                    m.inputs.remove(i);
                }
                *self.stats.consumed.entry(*i).or_insert(0) += *n as u64;
            }
            m.running = true;
            m.progress = 0;
        }
        e.wants_power = true;
        if s == 0 {
            e.status = Status::NoPower;
            return;
        }
        e.status = if s < 1000 {
            Status::LowPower
        } else {
            Status::Working
        };
        m.progress += speed * s as u64 / 1000;
        if m.progress >= rec.time as u64 * 1000 {
            m.progress = 0;
            m.running = false;
            for (i, n) in &rec.outputs {
                *m.outputs.entry(*i).or_insert(0) += n;
                *self.stats.produced.entry(*i).or_insert(0) += *n as u64;
            }
            self.stats.mass_lost += loss;
        }
    }

    fn update_inserter(&mut self, id: EntityId) {
        let s = self.satisfaction_of(id);
        let (pos, dir, half) = {
            let e = &self.entities[&id];
            (e.pos, e.dir, self.content.protos[e.proto as usize].speed)
        };
        let src = self.occ.get(&pos.step(dir.opposite())).copied();
        let dst = self.occ.get(&pos.step(dir)).copied();
        let (state, held) = match &self.entities[&id].data {
            Data::Inserter(i) => (i.state, i.held),
            _ => return,
        };
        let mut new_status = Status::Working;
        match state {
            ArmState::Idle => {
                let mut picked = None;
                match (src, dst) {
                    (Some(src), Some(dst)) if held.is_none() => {
                        let want = self.want_of(dst);
                        let target =
                            match self.content.protos[self.entities[&dst].proto as usize].kind {
                                ProtoKind::WasteBin => TargetKind::WasteBin,
                                ProtoKind::Chest => TargetKind::Chest,
                                _ => TargetKind::Other,
                            };
                        picked = self.take_from(src, &want, target);
                        if picked.is_none() {
                            new_status = Status::NoInput;
                        }
                    }
                    _ => new_status = Status::NoTarget,
                }
                let e = self.entities.get_mut(&id).unwrap();
                e.wants_power = false;
                if let (Some(item), Data::Inserter(ins)) = (picked, &mut e.data) {
                    ins.held = Some(item);
                    ins.state = ArmState::ToDrop;
                    ins.timer = 0;
                    e.wants_power = true;
                }
            }
            ArmState::ToDrop | ArmState::Returning => {
                let e = self.entities.get_mut(&id).unwrap();
                e.wants_power = true;
                if s == 0 {
                    new_status = Status::NoPower;
                }
                if let Data::Inserter(ins) = &mut e.data {
                    ins.timer += s;
                    if ins.timer >= half * 1000 {
                        ins.timer = 0;
                        ins.state = if state == ArmState::ToDrop {
                            ArmState::Dropping
                        } else {
                            ArmState::Idle
                        };
                    }
                }
            }
            ArmState::Dropping => {
                let item = held.expect("bras en depot sans objet");
                let ok = dst.map(|d| self.insert_into(d, item, dir, false)).unwrap_or(false);
                let e = self.entities.get_mut(&id).unwrap();
                e.wants_power = false;
                if let Data::Inserter(ins) = &mut e.data {
                    if ok {
                        ins.held = None;
                        ins.state = ArmState::Returning;
                        ins.timer = 0;
                    } else {
                        new_status = Status::OutputFull;
                    }
                }
            }
        }
        self.entities.get_mut(&id).unwrap().status = new_status;
    }

    // ------------------------------------------------------------------ convoyeurs

    fn belt_at(&self, p: Pos) -> Option<(EntityId, Dir)> {
        let id = *self.occ.get(&p)?;
        let e = &self.entities[&id];
        matches!(e.data, Data::Belt(_)).then_some((id, e.dir))
    }

    /// Recalcule la topologie et les lignes de transport apres une pose, un retrait ou une
    /// rotation de convoyeur. Les objets de chaque case sont conserves.
    fn refresh_belts(&mut self) {
        if !self.belts_dirty {
            return;
        }
        self.belts_dirty = false;
        let belts: Vec<(EntityId, Pos, Dir, u32)> = self
            .entities
            .values()
            .filter(|e| matches!(e.data, Data::Belt(_)))
            .map(|e| {
                (
                    e.id,
                    e.pos,
                    e.dir,
                    self.content.protos[e.proto as usize].speed,
                )
            })
            .collect();
        let topo: Topology = belts::topology(belts, |p| self.belt_at(p));
        for (id, e) in self.entities.iter_mut() {
            if let Data::Belt(b) = &mut e.data {
                b.feed = topo.feed[id];
                b.curve_from = topo.curve[id];
            }
        }
        let pending = std::mem::take(&mut self.pending_belt_items);
        belts!(&mut self.belts, b => {
            for (id, lanes) in pending {
                b.seed(id, lanes);
            }
            b.rebuild(&topo);
        });
    }

    fn update_belts(&mut self) {
        self.refresh_belts();
        belts!(&mut self.belts, b => b.step());
    }

    // ------------------------------------------------------------------ sauvegarde

    /// Objets de tous les convoyeurs non vides, dans l'ordre des identifiants.
    fn belt_items_by_tile(&self) -> Vec<(EntityId, BeltLanes)> {
        let ids: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| matches!(e.data, Data::Belt(_)))
            .map(|e| e.id)
            .collect();
        let mut by_id: BTreeMap<EntityId, BeltLanes> = BTreeMap::new();
        self.visit_belt_items(&ids, |id, lane, it| {
            by_id.entry(id).or_default()[lane].push(it)
        });
        by_id.into_iter().collect()
    }

    fn save_state(&self) -> SaveState {
        SaveState {
            params: self.world.params,
            tick: self.tick,
            next_id: self.next_id,
            entities: self.entities.values().cloned().collect(),
            belt_items: self.belt_items_by_tile(),
            chunks: self
                .world
                .modified_chunks()
                .map(|(k, c)| (*k, c.clone()))
                .collect(),
            stats: self.stats.clone(),
        }
    }

    /// Sauvegarde binaire versionnee.
    pub fn save_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(SAVE_MAGIC);
        out.extend_from_slice(&SAVE_VERSION.to_le_bytes());
        out.extend(bincode::serialize(&self.save_state()).expect("serialisation"));
        out
    }

    pub fn load_bytes(bytes: &[u8]) -> Result<Simulation, String> {
        if bytes.len() < 8 || &bytes[0..4] != SAVE_MAGIC {
            return Err("fichier de sauvegarde invalide".into());
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let st: SaveState = match version {
            1 => {
                let old: v1::SaveState =
                    bincode::deserialize(&bytes[8..]).map_err(|e| e.to_string())?;
                v1::convert(old)
            }
            SAVE_VERSION => bincode::deserialize(&bytes[8..]).map_err(|e| e.to_string())?,
            _ => {
                return Err(format!(
                    "version de sauvegarde {version} non prise en charge"
                ))
            }
        };
        if st.params.generator_version != GENERATOR_VERSION {
            return Err(format!(
                "generateur v{} requis",
                st.params.generator_version
            ));
        }
        let mut sim = Simulation::with_content(
            Content::load_default(),
            st.params.seed,
            st.params.infinite_resources,
        );
        sim.tick = st.tick;
        sim.next_id = st.next_id;
        sim.stats = st.stats;
        for (k, c) in st.chunks {
            sim.world.insert_chunk(k, c);
        }
        for e in st.entities {
            for t in e.tiles() {
                sim.occ.insert(t, e.id);
            }
            sim.entities.insert(e.id, e);
        }
        sim.pending_belt_items = st.belt_items;
        sim.belts_dirty = true;
        sim.power_dirty = true;
        sim.lists_dirty = true;
        sim.refresh_belts();
        // Les demandes d'energie sont dans l'etat sauvegarde : le reseau se reconstruit a l'identique.
        Ok(sim)
    }

    /// Empreinte de l'etat de jeu. Independante des chunks seulement affiches (camera).
    pub fn state_hash(&self) -> u64 {
        fnv64(&bincode::serialize(&self.save_state()).expect("serialisation"))
    }
}
