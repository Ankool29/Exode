//! Electricite a corridors (cahier v0.4 [V] : capacites limitees, sous-stations, saturation
//! des infrastructures ; pas de tension, de frequence ni d'electromagnetisme).
//!
//! - Noeuds : poteaux, pylones et sous-stations. Deux noeuds a portee l'un de l'autre sont relies ;
//!   la capacite d'une liaison est la plus faible des deux (faible entre poteaux, elevee entre
//!   pylones). Les composantes sont tenues par le socle commun (`networks.rs`).
//! - Les generateurs et les consommateurs se raccordent au premier noeud (ordre des
//!   identifiants) dont la zone d'alimentation touche leur emprise.
//! - Repartition par flot maximal entier et deterministe (Dinic), en trois passes :
//!   1. part equitable de la demande du moment quand la production manque ;
//!   2. demande du moment complete (c'est cette passe qui donne la charge affichee et le
//!      corridor limitant) ;
//!   3. reserve jusqu'a la puissance nominale des consommateurs raccordes, pour qu'une machine
//!      qui demarre soit servie sans attendre le calcul suivant.
//! - Le flot est recalcule quand la topologie d'un reseau change, et pour tous les reseaux a
//!   frequence fixe (`FLOW_PERIOD`). Chaque tick, chaque zone (noeud) recoit
//!   min(demande, allocation) : les capacites et la production ne sont jamais depassees.

use crate::entity::EntityId;
use crate::networks::{NetId, NetIndex};
use crate::sim::FastMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Periode de recalcul du flot, en ticks (valeur provisoire [P]).
pub const FLOW_PERIOD: u64 = 30;

/// Taille des cases de la grille de recherche des voisins, en demi-cases.
const BUCKET: i32 = 64;

/// Noeud du reseau : coordonnees du centre en demi-cases (entiers), portee des cables,
/// capacite de liaison (W) et zone d'alimentation (x0, y0, x1, y1 inclus) s'il en a une.
#[derive(Debug, Clone, Copy)]
pub struct NodeSpec {
    pub c2: (i32, i32),
    pub reach: i32,
    pub capacity: u64,
    pub area: Option<(i32, i32, i32, i32)>,
}

/// Entite raccordee a un noeud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attached {
    pub id: EntityId,
    /// Puissance nominale (W) : consommee en marche, ou produite pour une source.
    pub power: u64,
    pub source: bool,
}

/// Resultat du calcul de flot d'un reseau. Fait partie de l'etat sauvegarde : il est garde
/// entre deux calculs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Solution {
    /// Puissance allouee a chaque noeud (W).
    pub alloc: BTreeMap<EntityId, u64>,
    /// Flot de la passe 2 sur chaque liaison (a < b, positif de a vers b) et liaison limitante.
    pub flows: BTreeMap<(EntityId, EntityId), (i64, bool)>,
    /// Demande et puissance servie lors du calcul (W).
    pub demand: u64,
    pub served: u64,
}

/// Etat sauvegarde du reseau electrique.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerSave {
    pub solutions: Vec<(NetId, Solution)>,
    pub due: Vec<NetId>,
}

/// Bilan d'un reseau pendant le dernier tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetTick {
    pub supply: u64,
    pub demand: u64,
    pub delivered: u64,
}

/// Plan plat de distribution, refait quand une solution ou un raccordement change :
/// le tick ne fait plus que parcourir des tableaux.
#[derive(Default)]
struct Plan {
    /// (reseau, premier noeud, fin) dans `nodes`.
    nets: Vec<(NetId, usize, usize)>,
    /// (noeud, allocation, puissance nominale, consommateurs [debut, fin), sources [debut, fin)).
    nodes: Vec<(EntityId, u64, u64, (usize, usize), (usize, usize))>,
    cons: Vec<(EntityId, u64)>,
    srcs: Vec<(EntityId, u64)>,
    slot: FastMap<EntityId, usize>,
    sat: Vec<u32>,
}

#[derive(Default)]
pub struct PowerGrid {
    pub index: NetIndex,
    nodes: BTreeMap<EntityId, NodeSpec>,
    buckets: FastMap<(i32, i32), Vec<EntityId>>,
    links: FastMap<(EntityId, EntityId), u64>,
    attached: BTreeMap<EntityId, Vec<Attached>>,
    attach_of: FastMap<EntityId, EntityId>,
    solutions: BTreeMap<NetId, Solution>,
    due: BTreeSet<NetId>,
    plan: Plan,
    plan_dirty: bool,
    pub last_tick: BTreeMap<NetId, NetTick>,
    /// Calculs de flot effectues (diagnostic, bancs).
    pub solves: u64,
}

fn key(a: EntityId, b: EntityId) -> (EntityId, EntityId) {
    (a.min(b), a.max(b))
}

impl PowerGrid {
    pub fn new() -> PowerGrid {
        PowerGrid::default()
    }

    pub fn node(&self, id: EntityId) -> Option<&NodeSpec> {
        self.nodes.get(&id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = (&EntityId, &NodeSpec)> {
        self.nodes.iter()
    }

    pub fn link_capacity(&self, a: EntityId, b: EntityId) -> Option<u64> {
        self.links.get(&key(a, b)).copied()
    }

    /// Noeud auquel une entite est raccordee.
    pub fn attached_to(&self, id: EntityId) -> Option<EntityId> {
        self.attach_of.get(&id).copied()
    }

    pub fn solution(&self, net: NetId) -> Option<&Solution> {
        self.solutions.get(&net)
    }

    /// Flot (passe 2) et limitation d'une liaison.
    pub fn link_flow(&self, a: EntityId, b: EntityId) -> Option<(i64, bool)> {
        let net = self.index.net_of(a)?;
        let (f, lim) = *self.solutions.get(&net)?.flows.get(&key(a, b))?;
        Some(if a < b { (f, lim) } else { (-f, lim) })
    }

    fn bucket_of(c2: (i32, i32)) -> (i32, i32) {
        (c2.0.div_euclid(BUCKET), c2.1.div_euclid(BUCKET))
    }

    /// Ajoute un noeud : liaisons vers les noeuds a portee (portee la plus courte des deux).
    pub fn add_node(&mut self, id: EntityId, spec: NodeSpec) {
        let r = spec.reach.max(0);
        let span = (2 * r) / BUCKET + 1;
        let (bx, by) = Self::bucket_of(spec.c2);
        let mut neighbors: Vec<EntityId> = Vec::new();
        for gy in by - span..=by + span {
            for gx in bx - span..=bx + span {
                if let Some(v) = self.buckets.get(&(gx, gy)) {
                    for &o in v {
                        let os = self.nodes[&o];
                        let reach = 2 * r.min(os.reach);
                        let (dx, dy) = ((spec.c2.0 - os.c2.0) as i64, (spec.c2.1 - os.c2.1) as i64);
                        if dx * dx + dy * dy <= (reach as i64) * (reach as i64) {
                            neighbors.push(o);
                        }
                    }
                }
            }
        }
        neighbors.sort_unstable();
        for &o in &neighbors {
            let cap = spec.capacity.min(self.nodes[&o].capacity);
            self.links.insert(key(id, o), cap);
        }
        self.nodes.insert(id, spec);
        self.buckets
            .entry(Self::bucket_of(spec.c2))
            .or_default()
            .push(id);
        self.index.add_node(id, &neighbors);
        self.attached.entry(id).or_default();
        self.plan_dirty = true;
    }

    pub fn remove_node(&mut self, id: EntityId) {
        let Some(spec) = self.nodes.remove(&id) else {
            return;
        };
        let nbs: Vec<EntityId> = self.index.neighbors(id).copied().collect();
        for o in nbs {
            self.links.remove(&key(id, o));
        }
        if let Some(v) = self.buckets.get_mut(&Self::bucket_of(spec.c2)) {
            v.retain(|&x| x != id);
        }
        self.index.remove_node(id);
        if let Some(lst) = self.attached.remove(&id) {
            for a in lst {
                self.attach_of.remove(&a.id);
            }
        }
        self.plan_dirty = true;
    }

    /// Premier noeud (ordre des identifiants) dont la zone touche l'emprise (x0, y0, x1, y1).
    pub fn find_supplier(&self, rect: (i32, i32, i32, i32)) -> Option<EntityId> {
        // la plus grande zone d'alimentation est petite devant BUCKET : voisinage immediat
        let c2 = (rect.0 + rect.2 + 1, rect.1 + rect.3 + 1);
        let (bx, by) = Self::bucket_of(c2);
        let mut best: Option<EntityId> = None;
        for gy in by - 1..=by + 1 {
            for gx in bx - 1..=bx + 1 {
                if let Some(v) = self.buckets.get(&(gx, gy)) {
                    for &o in v {
                        if let Some((x0, y0, x1, y1)) = self.nodes[&o].area {
                            let overlap =
                                rect.0 <= x1 && rect.2 >= x0 && rect.1 <= y1 && rect.3 >= y0;
                            if overlap && best.map(|b| o < b).unwrap_or(true) {
                                best = Some(o);
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// Nouveaux raccordements (entites dans l'ordre des identifiants). Les reseaux dont la
    /// liste change sont a recalculer.
    pub fn set_attachments(&mut self, list: Vec<(EntityId, Attached)>) {
        let mut next: BTreeMap<EntityId, Vec<Attached>> =
            self.nodes.keys().map(|&n| (n, Vec::new())).collect();
        self.attach_of.clear();
        for (node, a) in list {
            if let Some(v) = next.get_mut(&node) {
                v.push(a);
                self.attach_of.insert(a.id, node);
            }
        }
        for (node, v) in &next {
            if self.attached.get(node) != Some(v) {
                if let Some(net) = self.index.net_of(*node) {
                    self.due.insert(net);
                }
            }
        }
        self.attached = next;
        self.plan_dirty = true;
    }

    /// Reseaux modifies dans la topologie : a recalculer au prochain tick.
    pub fn mark_topology_changes(&mut self) {
        let changed = self.index.take_changed();
        self.due.extend(changed);
        let live: BTreeSet<NetId> = self.index.nets().map(|(n, _)| *n).collect();
        self.solutions.retain(|n, _| live.contains(n));
        self.due.retain(|n| live.contains(n));
    }

    /// Apres chargement : on reprend les solutions sauvegardees sans rien recalculer.
    pub fn restore(&mut self, save: PowerSave) {
        self.index.take_changed();
        self.solutions = save.solutions.into_iter().collect();
        self.due = save.due.into_iter().collect();
        self.plan_dirty = true;
    }

    pub fn save(&self) -> PowerSave {
        PowerSave {
            solutions: self
                .solutions
                .iter()
                .map(|(n, s)| (*n, s.clone()))
                .collect(),
            due: self.due.iter().copied().collect(),
        }
    }

    /// Satisfaction (pour mille) de la zone d'un noeud lors du dernier tick.
    pub fn node_satisfaction(&self, node: EntityId) -> u32 {
        self.plan
            .slot
            .get(&node)
            .and_then(|&i| self.plan.sat.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// Noeuds et satisfaction de leur zone, avec indication d'une charge raccordee.
    pub fn zone_states(&self) -> impl Iterator<Item = (EntityId, u32, bool)> + '_ {
        self.plan
            .nodes
            .iter()
            .zip(&self.plan.sat)
            .map(|(n, &s)| (n.0, s, n.2 > 0))
    }

    fn rebuild_plan(&mut self) {
        let old: FastMap<EntityId, u32> = self
            .plan
            .slot
            .iter()
            .map(|(&n, &i)| (n, self.plan.sat.get(i).copied().unwrap_or(0)))
            .collect();
        let mut p = Plan::default();
        for (&net, members) in self.index.nets() {
            let start = p.nodes.len();
            let sol = self.solutions.get(&net);
            for &node in members {
                let alloc = sol.and_then(|s| s.alloc.get(&node)).copied().unwrap_or(0);
                let (c0, s0) = (p.cons.len(), p.srcs.len());
                let mut rated = 0;
                for a in &self.attached[&node] {
                    if a.source {
                        p.srcs.push((a.id, a.power));
                    } else {
                        rated += a.power;
                        p.cons.push((a.id, a.power));
                    }
                }
                p.slot.insert(node, p.nodes.len());
                p.sat.push(old.get(&node).copied().unwrap_or(0));
                p.nodes
                    .push((node, alloc, rated, (c0, p.cons.len()), (s0, p.srcs.len())));
            }
            p.nets.push((net, start, p.nodes.len()));
        }
        self.plan = p;
        self.plan_dirty = false;
    }

    /// Un tick : calculs dus (changement de topologie, ou periode), puis application.
    /// `wants` : l'entite demande-t-elle de l'energie (etat du tick precedent) ;
    /// `supply` : puissance disponible d'une source pour ce tick.
    /// Renvoie l'energie distribuee pendant ce tick (W x tick).
    pub fn update(
        &mut self,
        tick: u64,
        wants: &dyn Fn(EntityId) -> bool,
        supply: &dyn Fn(EntityId, u64) -> u64,
    ) -> u64 {
        if tick % FLOW_PERIOD == 0 {
            self.due.extend(self.index.nets().map(|(n, _)| *n));
        }
        let due = std::mem::take(&mut self.due);
        for net in due {
            if self.index.members(net).is_some() {
                let sol = self.solve(net, wants, supply);
                self.solutions.insert(net, sol);
                self.solves += 1;
                self.plan_dirty = true;
            }
        }
        if self.plan_dirty {
            self.rebuild_plan();
        }
        let mut total = 0;
        self.last_tick.clear();
        let p = &mut self.plan;
        let mut want_buf: Vec<bool> = Vec::new();
        for &(net, n0, n1) in &p.nets {
            let mut nt = NetTick::default();
            for k in n0..n1 {
                let (_, alloc, rated, (c0, c1), (s0, s1)) = p.nodes[k];
                for &(id, pw) in &p.srcs[s0..s1] {
                    nt.supply += supply(id, pw);
                }
                want_buf.clear();
                let mut d = 0u64;
                for &(id, pw) in &p.cons[c0..c1] {
                    let w = wants(id);
                    want_buf.push(w);
                    if w {
                        d += pw;
                    }
                }
                let delivered = d.min(alloc);
                let sat = if d > 0 {
                    (delivered * 1000 / d) as u32
                } else if rated > 0 {
                    (alloc * 1000 / rated).min(1000) as u32
                } else {
                    0
                };
                p.sat[k] = sat;
                nt.demand += d;
                // energie effectivement prise : chaque consommateur recoit sat pour mille
                for (j, &(_, pw)) in p.cons[c0..c1].iter().enumerate() {
                    if want_buf[j] {
                        nt.delivered += pw * sat as u64 / 1000;
                    }
                }
            }
            total += nt.delivered;
            self.last_tick.insert(net, nt);
        }
        total
    }

    /// Flot entier du reseau `net`.
    fn solve(
        &self,
        net: NetId,
        wants: &dyn Fn(EntityId) -> bool,
        supply: &dyn Fn(EntityId, u64) -> u64,
    ) -> Solution {
        let members: Vec<EntityId> = self.index.members(net).unwrap().iter().copied().collect();
        let k = members.len();
        let idx: FastMap<EntityId, usize> =
            members.iter().enumerate().map(|(i, &m)| (m, i)).collect();
        let (src, snk) = (k, k + 1);
        let mut g = Dinic::new(k + 2);
        let mut gen = vec![0u64; k];
        let mut dem = vec![0u64; k];
        let mut rated = vec![0u64; k];
        for (i, m) in members.iter().enumerate() {
            for a in &self.attached[m] {
                if a.source {
                    gen[i] += supply(a.id, a.power);
                } else {
                    rated[i] += a.power;
                    if wants(a.id) {
                        dem[i] += a.power;
                    }
                }
            }
        }
        let s_total: u64 = gen.iter().sum();
        let d_total: u64 = dem.iter().sum();
        for i in 0..k {
            if gen[i] > 0 {
                g.add_arc(src, i, gen[i]);
            }
        }
        let mut link_arcs: Vec<(EntityId, EntityId, usize, u64)> = Vec::new();
        for (i, m) in members.iter().enumerate() {
            for o in self.index.neighbors(*m) {
                if o > m {
                    let cap = self.links[&key(*m, *o)];
                    let e = g.add_undirected(i, idx[o], cap);
                    link_arcs.push((*m, *o, e, cap));
                }
            }
        }
        let mut sink_arcs = vec![usize::MAX; k];
        for i in 0..k {
            if rated[i] > 0 || dem[i] > 0 {
                let c1 = if d_total <= s_total {
                    dem[i]
                } else {
                    (dem[i] as u128 * s_total as u128 / d_total as u128) as u64
                };
                sink_arcs[i] = g.add_arc(i, snk, c1);
            }
        }
        // passe 1 : part equitable ; passe 2 : demande complete
        g.max_flow(src, snk);
        for i in 0..k {
            if sink_arcs[i] != usize::MAX {
                let c1 = g.arc_cap(sink_arcs[i]);
                g.raise(sink_arcs[i], dem[i].saturating_sub(c1));
            }
        }
        g.max_flow(src, snk);
        let served: u64 = (0..k)
            .filter(|&i| sink_arcs[i] != usize::MAX)
            .map(|i| g.flow(sink_arcs[i]))
            .sum();
        // corridor limitant : coupe minimale quand ni la demande ni la production ne bornent
        let limited = served < d_total.min(s_total);
        let reach = if limited {
            g.reachable(src)
        } else {
            Vec::new()
        };
        let mut flows = BTreeMap::new();
        for &(a, b, e, cap) in &link_arcs {
            let f = g.undirected_flow(e, cap);
            let lim = limited && {
                let (ia, ib) = (idx[&a], idx[&b]);
                reach[ia] != reach[ib] && f.unsigned_abs() >= cap
            };
            flows.insert((a, b), (f, lim));
        }
        // passe 3 : reserve jusqu'a la puissance nominale
        for i in 0..k {
            if sink_arcs[i] != usize::MAX {
                let c = g.arc_cap(sink_arcs[i]);
                g.raise(sink_arcs[i], rated[i].max(dem[i]).saturating_sub(c));
            }
        }
        g.max_flow(src, snk);
        let mut alloc = BTreeMap::new();
        for i in 0..k {
            if sink_arcs[i] != usize::MAX {
                alloc.insert(members[i], g.flow(sink_arcs[i]));
            }
        }
        Solution {
            alloc,
            flows,
            demand: d_total,
            served,
        }
    }
}

// ---------------------------------------------------------------------- flot maximal

/// Flot maximal entier (Dinic). Arcs parcourus dans l'ordre d'insertion : resultat
/// deterministe.
struct Dinic {
    adj: Vec<Vec<usize>>,
    to: Vec<usize>,
    res: Vec<u64>,
    /// Capacite totale de l'arc (pour retrouver le flot).
    cap: Vec<u64>,
}

impl Dinic {
    fn new(n: usize) -> Dinic {
        Dinic {
            adj: vec![Vec::new(); n],
            to: Vec::new(),
            res: Vec::new(),
            cap: Vec::new(),
        }
    }

    fn push(&mut self, a: usize, b: usize, c: u64) -> usize {
        let e = self.to.len();
        self.to.push(b);
        self.res.push(c);
        self.cap.push(c);
        self.adj[a].push(e);
        e
    }

    /// Arc a -> b ; l'arc inverse est e ^ 1.
    fn add_arc(&mut self, a: usize, b: usize, c: u64) -> usize {
        let e = self.push(a, b, c);
        self.push(b, a, 0);
        e
    }

    /// Liaison sans sens : capacite c dans chaque sens, flot net borne par c.
    fn add_undirected(&mut self, a: usize, b: usize, c: u64) -> usize {
        let e = self.push(a, b, c);
        self.push(b, a, c);
        e
    }

    fn arc_cap(&self, e: usize) -> u64 {
        self.cap[e]
    }

    fn raise(&mut self, e: usize, extra: u64) {
        self.cap[e] += extra;
        self.res[e] += extra;
    }

    fn flow(&self, e: usize) -> u64 {
        self.cap[e] - self.res[e]
    }

    fn undirected_flow(&self, e: usize, c: u64) -> i64 {
        c as i64 - self.res[e] as i64
    }

    fn levels(&self, s: usize) -> Vec<i32> {
        let mut lv = vec![-1; self.adj.len()];
        lv[s] = 0;
        let mut q = VecDeque::from([s]);
        while let Some(x) = q.pop_front() {
            for &e in &self.adj[x] {
                let y = self.to[e];
                if self.res[e] > 0 && lv[y] < 0 {
                    lv[y] = lv[x] + 1;
                    q.push_back(y);
                }
            }
        }
        lv
    }

    fn reachable(&self, s: usize) -> Vec<bool> {
        self.levels(s).into_iter().map(|l| l >= 0).collect()
    }

    fn max_flow(&mut self, s: usize, t: usize) -> u64 {
        let mut total = 0;
        loop {
            let lv = self.levels(s);
            if lv[t] < 0 {
                return total;
            }
            let mut it = vec![0usize; self.adj.len()];
            loop {
                let f = self.augment(s, t, &lv, &mut it);
                if f == 0 {
                    break;
                }
                total += f;
            }
        }
    }

    /// Un chemin augmentant dans le graphe de niveaux (parcours iteratif).
    fn augment(&mut self, s: usize, t: usize, lv: &[i32], it: &mut [usize]) -> u64 {
        let mut path: Vec<usize> = Vec::new();
        let mut x = s;
        loop {
            if x == t {
                let f = path.iter().map(|&e| self.res[e]).min().unwrap_or(0);
                for &e in &path {
                    self.res[e] -= f;
                    self.res[e ^ 1] += f;
                }
                return f;
            }
            let mut advanced = false;
            while it[x] < self.adj[x].len() {
                let e = self.adj[x][it[x]];
                let y = self.to[e];
                if self.res[e] > 0 && lv[y] == lv[x] + 1 {
                    path.push(e);
                    x = y;
                    advanced = true;
                    break;
                }
                it[x] += 1;
            }
            if !advanced {
                // impasse : on recule
                match path.pop() {
                    Some(e) => {
                        x = self.to[e ^ 1];
                        it[x] += 1;
                    }
                    None => return 0,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dinic_small_graph() {
        let mut g = Dinic::new(4);
        g.add_arc(0, 1, 10);
        g.add_arc(0, 2, 5);
        g.add_undirected(1, 2, 3);
        g.add_arc(1, 3, 4);
        g.add_arc(2, 3, 9);
        assert_eq!(g.max_flow(0, 3), 12);
    }
}
