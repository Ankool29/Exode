//! Reseau de chaleur (cahier v0.4 [V] : production, transport, recuperation, echangeurs,
//! limites de transfert ; modele energetique simplifie).
//!
//! - Energies en W x tick (1 W pendant un tick), mesurees au-dessus de la temperature ambiante
//!   (15 degres) : temperature = 15 + energie / capacite thermique. Tout est entier.
//! - Les caloducs relies forment un reseau (socle commun `networks.rs`) qui porte une seule
//!   energie ; fusion et separation au prorata des capacites, comme les segments de fluides.
//!   Chaque caloduc perd un peu de chaleur vers l'air, proportionnellement a l'ecart de
//!   temperature : un long reseau chaud coute de l'energie.
//! - Chaudiere : brule un combustible (masse perdue : fumees) et produit sa puissance tant
//!   qu'elle n'a pas atteint sa temperature maximale.
//! - Raccords de chaleur (chaudiere, echangeur) : l'energie va du plus chaud vers le plus
//!   froid (egalisation des temperatures), au plus `heat_limit` par tick.
//! - Echangeur : au-dessus de sa temperature de fonctionnement, transforme de l'eau en vapeur
//!   (meme masse) en prenant a sa chaleur l'energie que porte la vapeur.
//! - La turbine (electricite) prend l'energie de la vapeur et l'evacue : masse perdue.

use crate::content::ProtoKind;
use crate::entity::*;
use crate::fluids::{side_port, Cont, Role};
use crate::geom::{Dir, Pos};
use crate::networks::{NetId, NetIndex};
use crate::sim::Simulation;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Temperature ambiante (degres), provisoire [O].
pub const AMBIENT: i32 = 15;
/// Pertes d'un caloduc vers l'air, en W par degre au-dessus de l'ambiante (provisoire [O]).
pub const PIPE_LOSS: u64 = 2;

/// Bilan d'energie thermique et de la vapeur (W x tick).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeatStats {
    /// Energie des combustibles mis a bruler.
    pub fuel_burned: u64,
    pub heat_produced: u64,
    pub heat_lost: u64,
    /// Chaleur prise par les echangeurs pour faire de la vapeur.
    pub heat_to_steam: u64,
    /// Energie de la vapeur evacuee par les turbines.
    pub steam_used: u64,
    /// Energie electrique produite par les turbines.
    pub turbine_output: u64,
    /// Combustible non brule d'une chaudiere demontee (energie).
    pub fuel_removed: u64,
}

#[derive(Debug, Default)]
pub struct HeatNet {
    pub index: NetIndex,
    pub segs: BTreeMap<NetId, u64>,
    cap: BTreeMap<NetId, u64>,
}

impl HeatNet {
    pub fn capacity(&self, net: NetId) -> u64 {
        self.cap.get(&net).copied().unwrap_or(0)
    }
}

/// Volume de chaleur : reseau de caloducs ou appareil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HCont {
    Seg(NetId),
    Ent(EntityId),
}

#[derive(Debug, Clone)]
pub(crate) struct HeatDevice {
    pub id: EntityId,
    pub limit: u64,
    pub end: Option<HCont>,
}

pub fn temperature(energy: u64, cap: u64) -> i32 {
    if cap == 0 {
        AMBIENT
    } else {
        AMBIENT + (energy / cap).min(i32::MAX as u64 / 2) as i32
    }
}

impl Simulation {
    /// Raccord de chaleur d'un appareil : arriere de la chaudiere, flanc droit de l'echangeur
    /// (orientation nord : S et E).
    pub fn heat_port(&self, e: &Entity) -> Option<crate::fluids::Port> {
        match self.kind_of(e) {
            ProtoKind::Boiler => Some(side_port(e, Dir::S, Role::Both, 0)),
            ProtoKind::Exchanger => Some(side_port(e, Dir::E, Role::Both, 0)),
            _ => None,
        }
    }

    fn heat_pipe_links(&self, pos: Pos) -> Vec<EntityId> {
        let mut v: Vec<EntityId> = Dir::ALL
            .into_iter()
            .filter_map(|d| self.entity_at(pos.step(d)))
            .filter(|n| self.kind_of(n) == ProtoKind::HeatPipe)
            .map(|n| n.id)
            .collect();
        v.sort_unstable();
        v
    }

    fn recompute_heat_cap(&mut self, net: NetId) {
        let cap = self
            .heat
            .index
            .members(net)
            .unwrap()
            .iter()
            .map(|m| self.content.protos[self.entities[m].proto as usize].heat_capacity)
            .sum();
        self.heat.cap.insert(net, cap);
    }

    pub(crate) fn heat_add_pipe(&mut self, id: EntityId) {
        let links = self.heat_pipe_links(self.entities[&id].pos);
        let mut nets: Vec<NetId> = links
            .iter()
            .map(|n| self.heat.index.net_of(*n).unwrap())
            .collect();
        nets.sort_unstable();
        nets.dedup();
        let mut energy = 0;
        for n in nets {
            energy += self.heat.segs.remove(&n).unwrap_or(0);
            self.heat.cap.remove(&n);
        }
        self.heat.index.add_node(id, &links);
        self.heat.index.take_changed();
        let net = self.heat.index.net_of(id).unwrap();
        self.heat.segs.insert(net, energy);
        self.recompute_heat_cap(net);
        self.heat_dirty = true;
    }

    /// Caloduc retire : sa part de chaleur est perdue, le reste reparti au prorata.
    pub(crate) fn heat_remove_pipe(&mut self, id: EntityId, own_cap: u64) {
        let Some(net) = self.heat.index.net_of(id) else {
            return;
        };
        let old = self.heat.segs.remove(&net).unwrap_or(0);
        let old_cap = self.heat.cap.remove(&net).unwrap_or(own_cap).max(1);
        self.heat.index.remove_node(id);
        let parts: Vec<NetId> = self.heat.index.take_changed().into_iter().collect();
        for &p in &parts {
            self.recompute_heat_cap(p);
        }
        let share = (old as u128 * own_cap as u128 / old_cap as u128) as u64;
        let remaining = old - share;
        let rest: u64 = parts.iter().map(|p| self.heat.capacity(*p)).sum();
        let mut given = 0;
        for &p in &parts {
            let n = if rest > 0 {
                (remaining as u128 * self.heat.capacity(p) as u128 / rest as u128) as u64
            } else {
                0
            };
            given += n;
            self.heat.segs.insert(p, n);
        }
        let mut lost = share;
        match parts.first() {
            Some(p) => *self.heat.segs.get_mut(p).unwrap() += remaining - given,
            None => lost += remaining - given,
        }
        self.heat_stats.heat_lost += lost;
        self.heat_dirty = true;
    }

    pub(crate) fn heat_rebuild_after_load(&mut self, segs: Vec<(NetId, u64)>) {
        let pipes: Vec<(EntityId, Pos)> = self
            .entities
            .values()
            .filter(|e| self.kind_of(e) == ProtoKind::HeatPipe)
            .map(|e| (e.id, e.pos))
            .collect();
        for (id, pos) in pipes {
            let links = self.heat_pipe_links(pos);
            self.heat.index.add_node(id, &links);
        }
        self.heat.index.take_changed();
        let nets: Vec<NetId> = self.heat.index.nets().map(|(n, _)| *n).collect();
        for n in nets {
            self.recompute_heat_cap(n);
            self.heat.segs.insert(n, 0);
        }
        for (n, e) in segs {
            self.heat.segs.insert(n, e);
        }
        self.heat_dirty = true;
    }

    /// Masque des cotes relies d'un caloduc (N=1, E=2, S=4, W=8) pour l'affichage.
    pub fn heat_pipe_mask(&self, id: EntityId) -> u8 {
        let Some(e) = self.entities.get(&id) else {
            return 0;
        };
        let mut m = 0;
        for (bit, side) in Dir::ALL.into_iter().enumerate() {
            let Some(n) = self.entity_at(e.pos.step(side)) else {
                continue;
            };
            let linked = self.kind_of(n) == ProtoKind::HeatPipe
                || self
                    .heat_port(n)
                    .map(|p| p.tile == e.pos && p.facing == side.opposite())
                    .unwrap_or(false);
            if linked {
                m |= 1 << bit;
            }
        }
        m
    }

    fn resolve_heat(&self, owner: &Entity) -> Option<HCont> {
        let port = self.heat_port(owner)?;
        let n = self.entity_at(port.tile)?;
        if self.kind_of(n) == ProtoKind::HeatPipe {
            return self.heat.index.net_of(n.id).map(HCont::Seg);
        }
        let back = self.heat_port(n)?;
        (owner.contains(back.tile) && back.facing == port.facing.opposite())
            .then_some(HCont::Ent(n.id))
    }

    fn refresh_heat_devices(&mut self) {
        if !self.heat_dirty {
            return;
        }
        self.heat_dirty = false;
        let mut devs = Vec::new();
        for e in self.entities.values() {
            if self.heat_port(e).is_some() {
                devs.push(HeatDevice {
                    id: e.id,
                    limit: self.content.protos[e.proto as usize].heat_limit,
                    end: self.resolve_heat(e),
                });
            }
        }
        self.heat_devices = devs;
    }

    /// Energie et capacite thermique d'un volume de chaleur.
    pub fn heat_box(&self, c: HCont) -> (u64, u64) {
        match c {
            HCont::Seg(n) => (
                self.heat.segs.get(&n).copied().unwrap_or(0),
                self.heat.capacity(n),
            ),
            HCont::Ent(id) => {
                let e = &self.entities[&id];
                let cap = self.content.protos[e.proto as usize].heat_capacity;
                match &e.data {
                    Data::Boiler(b) => (b.heat, cap),
                    Data::Exchanger(x) => (x.heat, cap),
                    _ => (0, 0),
                }
            }
        }
    }

    fn set_heat(&mut self, c: HCont, v: u64) {
        match c {
            HCont::Seg(n) => {
                self.heat.segs.insert(n, v);
            }
            HCont::Ent(id) => match &mut self.entities.get_mut(&id).unwrap().data {
                Data::Boiler(b) => b.heat = v,
                Data::Exchanger(x) => x.heat = v,
                _ => {}
            },
        }
    }

    /// Egalise les temperatures de `a` vers `b` (du plus chaud au plus froid), au plus `limit`.
    fn heat_flow(&mut self, a: HCont, b: HCont, limit: u64) -> u64 {
        if a == b {
            return 0;
        }
        let (ea, ca) = self.heat_box(a);
        let (eb, cb) = self.heat_box(b);
        if ca == 0 || cb == 0 {
            return 0;
        }
        let lhs = ea as u128 * cb as u128;
        let rhs = eb as u128 * ca as u128;
        if lhs <= rhs {
            return 0;
        }
        let m = (((lhs - rhs) / (ca as u128 + cb as u128)) as u64).min(limit);
        self.set_heat(a, ea - m);
        self.set_heat(b, eb + m);
        m
    }

    /// Un tick de chaleur : chaudieres, raccords, pertes des caloducs, echangeurs.
    pub(crate) fn update_heat(&mut self) {
        self.refresh_heat_devices();
        let devs = std::mem::take(&mut self.heat_devices);
        // 1. chaudieres
        for d in &devs {
            let (proto, data) = {
                let e = &self.entities[&d.id];
                (e.proto, e.data.clone())
            };
            let Data::Boiler(mut b) = data else {
                continue;
            };
            let p = &self.content.protos[proto as usize];
            let max = p.heat_capacity * (p.temperature - AMBIENT).max(0) as u64;
            let room = max.saturating_sub(b.heat);
            if b.burn == 0 && b.fuel > 0 && room > 0 {
                let it = b.fuel_item.expect("combustible en attente");
                b.fuel -= 1;
                if b.fuel == 0 {
                    b.fuel_item = None;
                }
                let item = &self.content.items[it as usize];
                b.burn = item.fuel;
                self.heat_stats.fuel_burned += item.fuel;
                // la matiere du combustible part en fumee
                self.stats.mass_lost += item.mass;
                *self.stats.consumed.entry(it).or_insert(0) += 1;
            }
            let prod = p.heat_power.min(b.burn).min(room);
            b.burn -= prod;
            b.heat += prod;
            self.heat_stats.heat_produced += prod;
            let status = if prod > 0 {
                Status::Working
            } else if room == 0 {
                Status::OutputFull
            } else {
                Status::NoInput
            };
            let e = self.entities.get_mut(&d.id).unwrap();
            e.data = Data::Boiler(b);
            e.status = status;
        }
        // 2. raccords : du plus chaud au plus froid, dans l'ordre des identifiants
        for d in &devs {
            if let Some(end) = d.end {
                let me = HCont::Ent(d.id);
                if self.heat_flow(me, end, d.limit) == 0 {
                    self.heat_flow(end, me, d.limit);
                }
            }
        }
        // 3. pertes des caloducs vers l'air
        let nets: Vec<(NetId, u64)> = self.heat.segs.iter().map(|(n, e)| (*n, *e)).collect();
        for (n, e) in nets {
            let cap = self.heat.capacity(n);
            let pipes = self.heat.index.members(n).map(|m| m.len()).unwrap_or(0) as u64;
            if cap == 0 || e == 0 {
                continue;
            }
            let loss = ((e / cap) * pipes * PIPE_LOSS).min(e);
            self.heat.segs.insert(n, e - loss);
            self.heat_stats.heat_lost += loss;
        }
        // 4. echangeurs : eau -> vapeur
        let steam = self.content.fluid("vapeur");
        let water = self.content.fluid("eau");
        if let (Some(steam), Some(water)) = (steam, water) {
            let epu = self.content.fluids[steam as usize].energy.max(1);
            let steam_t = self.content.fluids[steam as usize].temperature;
            for d in &devs {
                let (proto, data) = {
                    let e = &self.entities[&d.id];
                    (e.proto, e.data.clone())
                };
                let Data::Exchanger(x) = data else {
                    continue;
                };
                let p = &self.content.protos[proto as usize];
                let threshold = p.heat_capacity * (p.temperature - AMBIENT).max(0) as u64;
                let avail = x.heat.saturating_sub(threshold);
                let space = p.fluid_capacity.saturating_sub(x.steam.amount);
                let water_in = if x.water.fluid == Some(water) {
                    x.water.amount
                } else {
                    0
                };
                let units = p
                    .flow_rate
                    .min(water_in)
                    .min(if x.steam.accepts(steam) { space } else { 0 })
                    .min(avail / epu);
                let status = if units > 0 {
                    Status::Working
                } else if water_in == 0 || avail < epu {
                    Status::NoInput
                } else {
                    Status::OutputFull
                };
                if units > 0 {
                    let mut w = x.water;
                    w.remove(units);
                    let mut s = x.steam;
                    s.add(steam, units, steam_t);
                    self.set_box(Cont::Ent(d.id, 0), w);
                    self.set_box(Cont::Ent(d.id, 1), s);
                    self.set_heat(HCont::Ent(d.id), x.heat - units * epu);
                    self.heat_stats.heat_to_steam += units * epu;
                }
                self.entities.get_mut(&d.id).unwrap().status = status;
            }
        }
        self.heat_devices = devs;
    }

    /// Chaleur stockee (W x tick) : caloducs, chaudieres, echangeurs.
    pub fn heat_stored(&self) -> u64 {
        let segs: u64 = self.heat.segs.values().sum();
        let ents: u64 = self
            .entities
            .values()
            .map(|e| match &e.data {
                Data::Boiler(b) => b.heat,
                Data::Exchanger(x) => x.heat,
                _ => 0,
            })
            .sum();
        segs + ents
    }

    /// Bilan de la chaleur : produite = stockee + perdue + transformee en vapeur ; et
    /// combustible brule = chaleur produite + reste a bruler.
    pub fn heat_balance_ok(&self) -> bool {
        let h = &self.heat_stats;
        let burning: u64 = self
            .entities
            .values()
            .map(|e| match &e.data {
                Data::Boiler(b) => b.burn,
                _ => 0,
            })
            .sum();
        h.heat_produced == self.heat_stored() + h.heat_lost + h.heat_to_steam
            && h.fuel_burned == h.heat_produced + burning + h.fuel_removed
    }

    /// Temperature et energie d'un caloduc (son reseau), d'une chaudiere ou d'un echangeur.
    pub fn heat_of(&self, id: EntityId) -> Option<(i32, u64, u64)> {
        let e = self.entities.get(&id)?;
        let c = match self.heat.index.net_of(id) {
            Some(n) => HCont::Seg(n),
            None if self.heat_port(e).is_some() => HCont::Ent(id),
            None => return None,
        };
        let (en, cap) = self.heat_box(c);
        Some((temperature(en, cap), en, cap))
    }

    /// Reseau d'un caloduc et nombre de caloducs.
    pub fn heat_network_of(&self, id: EntityId) -> Option<(NetId, usize)> {
        let n = self.heat.index.net_of(id)?;
        Some((n, self.heat.index.members(n)?.len()))
    }
}
