//! Reseaux de fluides (cahier v0.4 [V] : conduites segmentees, plusieurs fluides, debits
//! maximaux, pompes, vannes, reservoirs, saturation et blocage, temperature ; pas de
//! mecanique des fluides detaillee).
//!
//! - Un **segment** est une composante connexe de conduites (socle commun `networks.rs`) : un
//!   seul fluide, un volume, une temperature. Sa capacite est la somme de celles de ses conduites.
//! - Les segments sont delimites par les pompes, les vannes, les reservoirs et les machines :
//!   ces appareils ont des raccords, et c'est par eux seulement que le fluide passe d'un volume
//!   a l'autre, avec un debit plafonne :
//!   - pompe : de l'arriere vers l'avant, jamais l'inverse, si elle est alimentee ;
//!   - vanne : ouverte, egalise le remplissage de l'arriere vers l'avant ; fermee, bloque ;
//!   - reservoir : egalise son remplissage avec chaque segment raccorde, dans les deux sens ;
//!   - pompe a eau : au bord d'un lac, pousse de l'eau dans son raccord avant (extraction).
//! - Deux fluides differents ne se melangent jamais : une conduite qui relierait deux segments
//!   de fluides differents est refusee, et un transfert vers un volume d'un autre fluide n'a
//!   pas lieu.
//! - Temperature : melange pondere par les volumes, en entiers.
//! - Une fusion additionne les volumes ; une separation les repartit au prorata des capacites
//!   (reste a la partie de plus petit identifiant). La part d'une conduite demontee est comptee
//!   comme retiree (bilan matiere).

use crate::content::{FluidId, ProtoKind};
use crate::entity::*;
use crate::geom::{Dir, Pos};
use crate::networks::{NetId, NetIndex};
use crate::sim::{BuildError, FastMap, Simulation};
use std::collections::BTreeMap;

/// Etat des segments de conduites.
#[derive(Debug, Default)]
pub struct FluidNet {
    pub index: NetIndex,
    pub segs: BTreeMap<NetId, FluidBox>,
    cap: BTreeMap<NetId, u64>,
    /// Numero de la derniere modification de chaque volume (reveil des appareils). Cache
    /// non sauvegarde : apres chargement tout est eveille, ce qui ne change aucun resultat.
    stamp: FastMap<Cont, u64>,
    seq: u64,
    /// Passages d'appareils effectues (diagnostic, bancs).
    pub evaluations: u64,
}

impl FluidNet {
    pub fn capacity(&self, net: NetId) -> u64 {
        self.cap.get(&net).copied().unwrap_or(0)
    }
}

/// Role d'un raccord d'appareil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    In,
    Out,
    Both,
}

/// Raccord : case voisine raccordee, cote vers lequel il regarde, role.
#[derive(Debug, Clone, Copy)]
pub struct Port {
    pub tile: Pos,
    pub facing: Dir,
    pub role: Role,
}

/// Volume de fluide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cont {
    Seg(NetId),
    Ent(EntityId),
}

/// Appareil et volumes raccordes (cache recalcule quand la topologie change).
#[derive(Debug, Clone)]
pub(crate) struct Device {
    pub id: EntityId,
    pub kind: ProtoKind,
    pub rate: u64,
    pub ends: Vec<(Role, Option<Cont>)>,
    /// En veille : le dernier passage n'a rien deplace, et ne dependait que des volumes
    /// raccordes. Reveille des qu'un de ces volumes change (`stamp` > `seen`).
    pub asleep: bool,
    pub seen: u64,
}

fn is_pipe(k: ProtoKind) -> bool {
    matches!(k, ProtoKind::Pipe | ProtoKind::PipeUnderground)
}

impl Simulation {
    pub(crate) fn kind_of(&self, e: &Entity) -> ProtoKind {
        self.content.protos[e.proto as usize].kind
    }

    /// Une conduite accepte-t-elle une liaison sur ce cote ?
    fn pipe_side_ok(kind: ProtoKind, dir: Dir, side: Dir) -> bool {
        match kind {
            ProtoKind::Pipe => true,
            ProtoKind::PipeUnderground => side == dir,
            _ => false,
        }
    }

    /// Conduites reliees a une conduite (posee ou envisagee) : voisines en surface et
    /// partenaire souterrain.
    fn pipe_links_at(&self, pos: Pos, kind: ProtoKind, dir: Dir) -> Vec<EntityId> {
        let mut v = Vec::new();
        for side in Dir::ALL {
            if !Self::pipe_side_ok(kind, dir, side) {
                continue;
            }
            if let Some(n) = self.entity_at(pos.step(side)) {
                let nk = self.kind_of(n);
                if is_pipe(nk) && Self::pipe_side_ok(nk, n.dir, side.opposite()) {
                    v.push(n.id);
                }
            }
        }
        if kind == ProtoKind::PipeUnderground {
            if let Some(p) = self.underground_partner(pos, kind, dir) {
                v.push(p);
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Premiere conduite souterraine le long du tunnel ; partenaire si elle regarde vers nous.
    fn underground_partner(&self, pos: Pos, kind: ProtoKind, dir: Dir) -> Option<EntityId> {
        let reach = self
            .content
            .protos
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| p.reach)
            .unwrap_or(0);
        let back = dir.opposite();
        let mut p = pos;
        for _ in 0..reach {
            p = p.step(back);
            if let Some(n) = self.entity_at(p) {
                if self.kind_of(n) == ProtoKind::PipeUnderground {
                    return (n.dir == back).then_some(n.id);
                }
            }
        }
        None
    }

    /// Refuse une conduite qui melangerait deux fluides.
    pub(crate) fn check_fluid_mix(
        &self,
        kind: ProtoKind,
        pos: Pos,
        dir: Dir,
    ) -> Result<(), BuildError> {
        if !is_pipe(kind) {
            return Ok(());
        }
        let mut seen: Option<FluidId> = None;
        for n in self.pipe_links_at(pos, kind, dir) {
            let net = self.fluids.index.net_of(n).unwrap();
            if let Some(f) = self.fluids.segs.get(&net).and_then(|b| b.fluid) {
                if seen.map(|s| s != f).unwrap_or(false) {
                    return Err(BuildError::FluidMix);
                }
                seen = Some(f);
            }
        }
        Ok(())
    }

    fn recompute_cap(&mut self, net: NetId) {
        let members: Vec<EntityId> = self
            .fluids
            .index
            .members(net)
            .unwrap()
            .iter()
            .copied()
            .collect();
        let cap = members
            .iter()
            .map(|m| self.content.protos[self.entities[m].proto as usize].fluid_capacity)
            .sum();
        self.fluids.cap.insert(net, cap);
    }

    /// Conduite posee : fusion des segments voisins (volumes additionnes).
    pub(crate) fn fluid_add_pipe(&mut self, id: EntityId) {
        let e = &self.entities[&id];
        let links = self.pipe_links_at(e.pos, self.kind_of(e), e.dir);
        let mut merged = FluidBox::default();
        let mut taken: Vec<NetId> = links
            .iter()
            .map(|n| self.fluids.index.net_of(*n).unwrap())
            .collect();
        taken.sort_unstable();
        taken.dedup();
        for net in taken {
            if let Some(b) = self.fluids.segs.remove(&net) {
                if let Some(f) = b.fluid {
                    merged.add(f, b.amount, b.temp);
                }
            }
            self.fluids.cap.remove(&net);
        }
        self.fluids.index.add_node(id, &links);
        self.fluids.index.take_changed();
        let net = self.fluids.index.net_of(id).unwrap();
        self.fluids.segs.insert(net, merged);
        self.recompute_cap(net);
        self.fluid_dirty = true;
    }

    /// Conduite retiree : sa part du fluide est retiree, le reste reparti au prorata des
    /// capacites des segments restants. Renvoie la masse retiree (grammes).
    pub(crate) fn fluid_remove_pipe(&mut self, id: EntityId, own_cap: u64) -> u64 {
        let Some(net) = self.fluids.index.net_of(id) else {
            return 0;
        };
        let old = self.fluids.segs.remove(&net).unwrap_or_default();
        let old_cap = self.fluids.cap.remove(&net).unwrap_or(own_cap).max(1);
        self.fluids.index.remove_node(id);
        let parts: Vec<NetId> = self.fluids.index.take_changed().into_iter().collect();
        for &p in &parts {
            self.recompute_cap(p);
        }
        let share = (old.amount as u128 * own_cap as u128 / old_cap as u128) as u64;
        let remaining = old.amount - share;
        let rest_cap: u64 = parts.iter().map(|p| self.fluids.capacity(*p)).sum();
        let mut given = 0;
        let mut boxes: Vec<FluidBox> = Vec::new();
        for &p in &parts {
            let n = if rest_cap > 0 {
                (remaining as u128 * self.fluids.capacity(p) as u128 / rest_cap as u128) as u64
            } else {
                0
            };
            given += n;
            let mut b = FluidBox::default();
            if let Some(f) = old.fluid {
                b.add(f, n, old.temp);
            }
            boxes.push(b);
        }
        let mut removed = share;
        if let (Some(f), Some(first)) = (old.fluid, boxes.first_mut()) {
            first.add(f, remaining - given, old.temp);
        } else {
            removed += remaining - given;
        }
        for (p, b) in parts.into_iter().zip(boxes) {
            self.fluids.segs.insert(p, b);
        }
        self.fluid_dirty = true;
        old.fluid
            .map(|f| removed * self.content.fluids[f as usize].mass)
            .unwrap_or(0)
    }

    /// Apres chargement : conduites dans l'ordre des identifiants, puis volumes sauvegardes.
    pub(crate) fn fluid_rebuild_after_load(&mut self, segs: Vec<(NetId, FluidBox)>) {
        let pipes: Vec<(EntityId, Pos, ProtoKind, Dir)> = self
            .entities
            .values()
            .filter(|e| is_pipe(self.kind_of(e)))
            .map(|e| (e.id, e.pos, self.kind_of(e), e.dir))
            .collect();
        for (id, pos, kind, dir) in pipes {
            let links = self.pipe_links_at(pos, kind, dir);
            self.fluids.index.add_node(id, &links);
        }
        self.fluids.index.take_changed();
        let nets: Vec<NetId> = self.fluids.index.nets().map(|(n, _)| *n).collect();
        for n in nets {
            self.recompute_cap(n);
            self.fluids.segs.insert(n, FluidBox::default());
        }
        for (n, b) in segs {
            self.fluids.segs.insert(n, b);
        }
        self.fluid_dirty = true;
    }

    /// Raccords d'un appareil a fluide.
    pub fn fluid_ports(&self, e: &Entity) -> Vec<Port> {
        let (x, y) = (e.pos.x, e.pos.y);
        match self.kind_of(e) {
            ProtoKind::Pump | ProtoKind::Valve => vec![
                Port {
                    tile: e.back_tile(),
                    facing: e.dir.opposite(),
                    role: Role::In,
                },
                Port {
                    tile: e.front_tile(),
                    facing: e.dir,
                    role: Role::Out,
                },
            ],
            ProtoKind::WaterPump => vec![Port {
                tile: e.front_tile(),
                facing: e.dir,
                role: Role::Out,
            }],
            ProtoKind::Tank => vec![
                Port {
                    tile: Pos::new(x + e.w / 2, y - 1),
                    facing: Dir::N,
                    role: Role::Both,
                },
                Port {
                    tile: Pos::new(x + e.w, y + e.h / 2),
                    facing: Dir::E,
                    role: Role::Both,
                },
                Port {
                    tile: Pos::new(x + e.w / 2, y + e.h),
                    facing: Dir::S,
                    role: Role::Both,
                },
                Port {
                    tile: Pos::new(x - 1, y + e.h / 2),
                    facing: Dir::W,
                    role: Role::Both,
                },
            ],
            _ => Vec::new(),
        }
    }

    /// Volume propre d'un appareil (reservoir), s'il en a un.
    fn has_volume(kind: ProtoKind) -> bool {
        kind == ProtoKind::Tank
    }

    /// Ce qui est raccorde a un raccord : segment de conduites, ou volume d'un appareil dont
    /// un raccord fait face.
    fn resolve_port(&self, owner: &Entity, port: Port) -> Option<Cont> {
        let n = self.entity_at(port.tile)?;
        let nk = self.kind_of(n);
        if is_pipe(nk) {
            if Self::pipe_side_ok(nk, n.dir, port.facing.opposite()) {
                return self.fluids.index.net_of(n.id).map(Cont::Seg);
            }
            return None;
        }
        if Self::has_volume(nk) {
            let back = self
                .fluid_ports(n)
                .into_iter()
                .any(|q| owner.contains(q.tile) && q.facing == port.facing.opposite());
            if back {
                return Some(Cont::Ent(n.id));
            }
        }
        None
    }

    /// Masque des cotes raccordes d'une conduite (N=1, E=2, S=4, W=8) pour l'affichage.
    pub fn pipe_mask(&self, id: EntityId) -> u8 {
        let Some(e) = self.entities.get(&id) else {
            return 0;
        };
        let kind = self.kind_of(e);
        let mut m = 0;
        for (bit, side) in Dir::ALL.into_iter().enumerate() {
            if !Self::pipe_side_ok(kind, e.dir, side) {
                continue;
            }
            let Some(n) = self.entity_at(e.pos.step(side)) else {
                continue;
            };
            let nk = self.kind_of(n);
            let linked = if is_pipe(nk) {
                Self::pipe_side_ok(nk, n.dir, side.opposite())
            } else {
                self.fluid_ports(n)
                    .iter()
                    .any(|q| q.tile == e.pos && q.facing == side.opposite())
            };
            if linked {
                m |= 1 << bit;
            }
        }
        m
    }

    fn refresh_fluid_devices(&mut self) {
        if !self.fluid_dirty {
            return;
        }
        self.fluid_dirty = false;
        let mut devs = Vec::new();
        for e in self.entities.values() {
            let kind = self.kind_of(e);
            let ports = self.fluid_ports(e);
            if ports.is_empty() {
                continue;
            }
            let ends = ports
                .iter()
                .map(|&p| (p.role, self.resolve_port(e, p)))
                .collect();
            devs.push(Device {
                id: e.id,
                kind,
                rate: self.content.protos[e.proto as usize].flow_rate,
                ends,
                asleep: false,
                seen: 0,
            });
        }
        self.fluid_devices = devs;
    }

    /// Contenu et capacite d'un volume.
    pub fn cont_box(&self, c: Cont) -> (FluidBox, u64) {
        match c {
            Cont::Seg(n) => (
                self.fluids.segs.get(&n).copied().unwrap_or_default(),
                self.fluids.capacity(n),
            ),
            Cont::Ent(id) => {
                let e = &self.entities[&id];
                let cap = self.content.protos[e.proto as usize].fluid_capacity;
                match &e.data {
                    Data::Tank(b) => (*b, cap),
                    _ => (FluidBox::default(), 0),
                }
            }
        }
    }

    fn set_box(&mut self, c: Cont, b: FluidBox) {
        self.fluids.seq += 1;
        self.fluids.stamp.insert(c, self.fluids.seq);
        match c {
            Cont::Seg(n) => {
                self.fluids.segs.insert(n, b);
            }
            Cont::Ent(id) => {
                if let Data::Tank(t) = &mut self.entities.get_mut(&id).unwrap().data {
                    *t = b;
                }
            }
        }
    }

    /// Quantite qui peut passer d'un volume a l'autre (meme fluide ou volume vide), au plus `max`.
    fn movable_boxes(fa: &FluidBox, fb: &FluidBox, cb: u64, max: u64) -> u64 {
        match fa.fluid {
            Some(f) if fb.accepts(f) => max.min(fa.amount).min(cb.saturating_sub(fb.amount)),
            _ => 0,
        }
    }

    /// Deplace `m` unites de `a` vers `b` (boites deja lues).
    fn transfer_boxes(&mut self, a: Cont, mut fa: FluidBox, b: Cont, mut fb: FluidBox, m: u64) {
        if m == 0 || a == b {
            return;
        }
        let f = fa.fluid.unwrap();
        let t = fa.temp;
        fa.remove(m);
        fb.add(f, m, t);
        self.set_box(a, fa);
        self.set_box(b, fb);
    }

    /// Egalise le remplissage (volume / capacite) de `a` vers `b`, au plus `rate`.
    fn equalize(&mut self, a: Cont, b: Cont, rate: u64) -> u64 {
        if a == b {
            return 0;
        }
        let (fa, ca) = self.cont_box(a);
        let (fb, cb) = self.cont_box(b);
        if ca == 0 || cb == 0 {
            return 0;
        }
        let lhs = fa.amount as u128 * cb as u128;
        let rhs = fb.amount as u128 * ca as u128;
        if lhs <= rhs {
            return 0;
        }
        let mut m = ((lhs - rhs) / (ca as u128 + cb as u128)) as u64;
        if m == 0 && fa.amount >= ca {
            // un volume plein cede toujours au moins une unite : sinon l'arrondi entier
            // bloquerait un reservoir juste avant le plein (pas d'aller-retour possible :
            // seul un volume plein cede ainsi)
            m = 1;
        }
        let m = Self::movable_boxes(&fa, &fb, cb, m.min(rate));
        self.transfer_boxes(a, fa, b, fb, m);
        m
    }

    /// Un tick des reseaux de fluides : appareils dans l'ordre des identifiants.
    pub(crate) fn update_fluids(&mut self) {
        self.refresh_fluid_devices();
        let mut devs = std::mem::take(&mut self.fluid_devices);
        for d in devs.iter_mut() {
            if d.asleep {
                let woken = d.ends.iter().any(|(_, c)| {
                    c.map(|c| self.fluids.stamp.get(&c).copied().unwrap_or(0) > d.seen)
                        .unwrap_or(false)
                });
                if !woken {
                    continue;
                }
            }
            self.fluids.evaluations += 1;
            let rate = d.rate;
            let mut status = Status::Idle;
            let mut wants = false;
            // vrai si le resultat ne depend que des volumes raccordes et que rien n'a bouge
            let mut quiet = false;
            match d.kind {
                ProtoKind::Pump => match (d.ends[0].1, d.ends[1].1) {
                    (Some(a), Some(b)) => {
                        let (fa, _) = self.cont_box(a);
                        let (fb, cb) = self.cont_box(b);
                        let can = Self::movable_boxes(&fa, &fb, cb, rate);
                        if can == 0 || a == b {
                            quiet = true;
                            status = if fa.amount == 0 {
                                Status::NoInput
                            } else {
                                Status::OutputFull
                            };
                        } else {
                            wants = true;
                            let s = self.satisfaction_of(d.id) as u64;
                            self.transfer_boxes(a, fa, b, fb, can * s / 1000);
                            status = if s == 0 {
                                Status::NoPower
                            } else if s < 1000 {
                                Status::LowPower
                            } else {
                                Status::Working
                            };
                        }
                    }
                    _ => {
                        quiet = true;
                        status = Status::NoTarget;
                    }
                },
                ProtoKind::Valve => {
                    let open = matches!(self.entities[&d.id].data, Data::Valve { open: true });
                    let moved = match (open, d.ends[0].1, d.ends[1].1) {
                        (true, Some(a), Some(b)) => self.equalize(a, b, rate),
                        _ => 0,
                    };
                    quiet = moved == 0;
                    if moved > 0 {
                        status = Status::Working;
                    }
                }
                ProtoKind::Tank => {
                    let me = Cont::Ent(d.id);
                    let mut moved = 0;
                    for &(_, end) in &d.ends {
                        if let Some(c) = end {
                            let m = self.equalize(me, c, rate);
                            moved += if m == 0 {
                                self.equalize(c, me, rate)
                            } else {
                                m
                            };
                        }
                    }
                    quiet = moved == 0;
                    let (b, cap) = self.cont_box(me);
                    status = if b.amount >= cap {
                        Status::OutputFull
                    } else if b.amount > 0 {
                        Status::Working
                    } else {
                        Status::Idle
                    };
                }
                ProtoKind::WaterPump => {
                    let water = self.content.fluid("eau").expect("fluide eau");
                    match d.ends[0].1 {
                        Some(c) => {
                            let (b, cap) = self.cont_box(c);
                            let n = if b.accepts(water) {
                                rate.min(cap.saturating_sub(b.amount))
                            } else {
                                0
                            };
                            if n > 0 {
                                let fl = &self.content.fluids[water as usize];
                                let (mass, temp) = (fl.mass, fl.temperature);
                                let mut nb = b;
                                nb.add(water, n, temp);
                                self.set_box(c, nb);
                                self.stats.mass_extracted += n * mass;
                                status = Status::Working;
                            } else {
                                quiet = true;
                                status = Status::OutputFull;
                            }
                        }
                        None => {
                            quiet = true;
                            status = Status::NoTarget;
                        }
                    }
                }
                _ => {}
            }
            d.asleep = quiet;
            d.seen = self.fluids.seq;
            let e = self.entities.get_mut(&d.id).unwrap();
            e.status = status;
            e.wants_power = wants;
        }
        self.fluid_devices = devs;
    }

    /// Appareils a fluide eveilles (diagnostic, bancs).
    pub fn fluid_device_stats(&self) -> (usize, usize, u64) {
        let awake = self.fluid_devices.iter().filter(|d| !d.asleep).count();
        (self.fluid_devices.len(), awake, self.fluids.evaluations)
    }

    /// Masse des fluides des segments (grammes).
    pub(crate) fn fluid_segment_mass(&self) -> u64 {
        self.fluids
            .segs
            .values()
            .filter_map(|b| {
                b.fluid
                    .map(|f| b.amount * self.content.fluids[f as usize].mass)
            })
            .sum()
    }

    /// Volume raccorde a une entite pour le panneau : segment d'une conduite, ou volume propre.
    pub fn fluid_of(&self, id: EntityId) -> Option<(FluidBox, u64)> {
        let e = self.entities.get(&id)?;
        if let Some(net) = self.fluids.index.net_of(id) {
            return Some(self.cont_box(Cont::Seg(net)));
        }
        if Self::has_volume(self.kind_of(e)) {
            return Some(self.cont_box(Cont::Ent(id)));
        }
        None
    }

    /// Segment d'une conduite (identifiant canonique) et nombre de conduites.
    pub fn segment_of(&self, id: EntityId) -> Option<(NetId, usize)> {
        let net = self.fluids.index.net_of(id)?;
        Some((net, self.fluids.index.members(net)?.len()))
    }

    /// Ajout direct de fluide dans une conduite ou un reservoir (tests et bancs). Compte comme
    /// extrait. Renvoie la quantite posee.
    pub fn debug_fill(&mut self, p: Pos, fluid: FluidId, amount: u64) -> u64 {
        let Some(id) = self.entity_at(p).map(|e| e.id) else {
            return 0;
        };
        let c = match self.fluids.index.net_of(id) {
            Some(n) => Cont::Seg(n),
            None if Self::has_volume(self.kind_of(&self.entities[&id])) => Cont::Ent(id),
            None => return 0,
        };
        let (mut b, cap) = self.cont_box(c);
        if !b.accepts(fluid) {
            return 0;
        }
        let n = amount.min(cap - b.amount);
        let fl = &self.content.fluids[fluid as usize];
        let (mass, temp) = (fl.mass, fl.temperature);
        b.add(fluid, n, temp);
        self.set_box(c, b);
        self.stats.mass_extracted += n * mass;
        n
    }

    /// Ouvre ou ferme une vanne.
    pub fn toggle_at(&mut self, p: Pos) -> bool {
        let Some(id) = self.entity_at(p).map(|e| e.id) else {
            return false;
        };
        match &mut self.entities.get_mut(&id).unwrap().data {
            Data::Valve { open } => {
                *open = !*open;
                // les appareils endormis se reveillent
                self.fluid_dirty = true;
                true
            }
            _ => false,
        }
    }
}
