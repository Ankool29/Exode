//! Socle commun des reseaux (electricite, fluides, chaleur ; cahier v0.4, audit n°2 [R]).
//!
//! Un reseau est une composante connexe d'un graphe de noeuds (entites). Les trois reseaux
//! partagent cette infrastructure mais gardent leurs propres regles :
//! - union a la construction (fusion des composantes voisines) ;
//! - reconstruction locale au retrait (seule l'ancienne composante est reparcourue) ;
//! - identifiant canonique : le plus petit identifiant d'entite de la composante. Il ne depend
//!   pas de l'ordre des operations : un reseau recalcule apres chargement a le meme numero ;
//! - numero de version par reseau, augmente a chaque changement (caches des regles).
//!
//! Tout ce qui est parcouru est ordonne (BTreeMap, BTreeSet) ; la table de hachage `net_of` ne
//! sert qu'aux recherches.

use crate::entity::EntityId;
use crate::sim::FastMap;
use std::collections::{BTreeMap, BTreeSet};

/// Identifiant canonique d'un reseau : plus petit identifiant d'entite de ses noeuds.
pub type NetId = EntityId;

#[derive(Debug, Default, Clone)]
pub struct NetIndex {
    adj: BTreeMap<EntityId, BTreeSet<EntityId>>,
    net_of: FastMap<EntityId, NetId>,
    members: BTreeMap<NetId, BTreeSet<EntityId>>,
    version: BTreeMap<NetId, u64>,
    counter: u64,
    /// Reseaux modifies depuis le dernier `take_changed` (identifiants actuels).
    changed: BTreeSet<NetId>,
}

impl NetIndex {
    pub fn new() -> NetIndex {
        NetIndex::default()
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.adj.contains_key(&id)
    }

    pub fn net_of(&self, id: EntityId) -> Option<NetId> {
        self.net_of.get(&id).copied()
    }

    pub fn members(&self, net: NetId) -> Option<&BTreeSet<EntityId>> {
        self.members.get(&net)
    }

    /// Reseaux dans l'ordre de leur identifiant.
    pub fn nets(&self) -> impl Iterator<Item = (&NetId, &BTreeSet<EntityId>)> {
        self.members.iter()
    }

    pub fn net_count(&self) -> usize {
        self.members.len()
    }

    pub fn neighbors(&self, id: EntityId) -> impl Iterator<Item = &EntityId> {
        self.adj.get(&id).into_iter().flatten()
    }

    pub fn version(&self, net: NetId) -> u64 {
        self.version.get(&net).copied().unwrap_or(0)
    }

    /// Reseaux modifies depuis le dernier appel (seuls ceux qui existent encore).
    pub fn take_changed(&mut self) -> BTreeSet<NetId> {
        let c = std::mem::take(&mut self.changed);
        c.into_iter()
            .filter(|n| self.members.contains_key(n))
            .collect()
    }

    fn touch(&mut self, net: NetId) {
        self.counter += 1;
        self.version.insert(net, self.counter);
        self.changed.insert(net);
    }

    /// Ajoute un noeud et ses liaisons vers des noeuds existants : fusion des composantes.
    pub fn add_node(&mut self, id: EntityId, neighbors: &[EntityId]) {
        if self.adj.contains_key(&id) {
            return;
        }
        self.adj.insert(id, BTreeSet::new());
        self.net_of.insert(id, id);
        self.members.insert(id, BTreeSet::from([id]));
        self.touch(id);
        for &n in neighbors {
            self.add_edge(id, n);
        }
    }

    /// Liaison entre deux noeuds existants.
    pub fn add_edge(&mut self, a: EntityId, b: EntityId) {
        if a == b || !self.adj.contains_key(&a) || !self.adj.contains_key(&b) {
            return;
        }
        if !self.adj.get_mut(&a).unwrap().insert(b) {
            return;
        }
        self.adj.get_mut(&b).unwrap().insert(a);
        let (na, nb) = (self.net_of[&a], self.net_of[&b]);
        if na == nb {
            self.touch(na);
            return;
        }
        // fusion : la plus petite composante rejoint la plus grande, identifiant = minimum
        let (big, small) = if self.members[&na].len() >= self.members[&nb].len() {
            (na, nb)
        } else {
            (nb, na)
        };
        let moved = self.members.remove(&small).unwrap();
        self.version.remove(&small);
        let mut set = self.members.remove(&big).unwrap();
        self.version.remove(&big);
        for &m in &moved {
            self.net_of.insert(m, big);
        }
        set.extend(moved);
        let id = na.min(nb);
        if id != big {
            for &m in &set {
                self.net_of.insert(m, id);
            }
        }
        self.members.insert(id, set);
        self.touch(id);
    }

    /// Retire une liaison : seule la composante concernee est reparcourue.
    pub fn remove_edge(&mut self, a: EntityId, b: EntityId) {
        let removed = match self.adj.get_mut(&a) {
            Some(s) => s.remove(&b),
            None => false,
        };
        if !removed {
            return;
        }
        self.adj.get_mut(&b).unwrap().remove(&a);
        let net = self.net_of[&a];
        self.split(net, &[a, b]);
    }

    /// Retire un noeud et ses liaisons : reconstruction locale de son ancienne composante.
    pub fn remove_node(&mut self, id: EntityId) {
        let Some(nbs) = self.adj.remove(&id) else {
            return;
        };
        for n in &nbs {
            self.adj.get_mut(n).unwrap().remove(&id);
        }
        let net = self.net_of.remove(&id).unwrap();
        self.members.get_mut(&net).unwrap().remove(&id);
        if self.members[&net].is_empty() {
            self.members.remove(&net);
            self.version.remove(&net);
            return;
        }
        let seeds: Vec<EntityId> = nbs.into_iter().collect();
        self.split(net, &seeds);
    }

    /// Reparcourt l'ancienne composante `net` a partir des noeuds `seeds` (qui en font partie).
    fn split(&mut self, net: NetId, seeds: &[EntityId]) {
        let mut old = self.members.remove(&net).unwrap();
        self.version.remove(&net);
        let mut parts: Vec<BTreeSet<EntityId>> = Vec::new();
        for &s in seeds {
            if !old.contains(&s) {
                continue;
            }
            // parcours en largeur limite a l'ancienne composante
            let mut part = BTreeSet::new();
            let mut stack = vec![s];
            old.remove(&s);
            part.insert(s);
            while let Some(x) = stack.pop() {
                for &y in &self.adj[&x] {
                    if old.remove(&y) {
                        part.insert(y);
                        stack.push(y);
                    }
                }
            }
            parts.push(part);
        }
        debug_assert!(old.is_empty(), "noeud hors de portee des graines");
        for part in parts {
            let id = *part.iter().next().unwrap();
            for &m in &part {
                self.net_of.insert(m, id);
            }
            self.members.insert(id, part);
            self.touch(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Composantes recalculees depuis zero : reference pour l'index incremental.
    fn components(edges: &[(EntityId, EntityId)], nodes: &[EntityId]) -> BTreeMap<EntityId, NetId> {
        let mut idx = NetIndex::new();
        for &n in nodes {
            idx.add_node(n, &[]);
        }
        for &(a, b) in edges {
            idx.add_edge(a, b);
        }
        nodes.iter().map(|&n| (n, idx.net_of(n).unwrap())).collect()
    }

    #[test]
    fn incremental_matches_rebuild_and_ids_are_canonical() {
        let mut idx = NetIndex::new();
        // chaine 5-3-9-1 puis 7 isole
        idx.add_node(5, &[]);
        idx.add_node(3, &[5]);
        idx.add_node(9, &[3]);
        idx.add_node(1, &[9]);
        idx.add_node(7, &[]);
        assert_eq!(idx.net_of(5), Some(1));
        assert_eq!(idx.net_of(7), Some(7));
        assert_eq!(idx.net_count(), 2);
        // retrait au milieu : deux reseaux, identifiants = minimum de chaque partie
        idx.remove_node(3);
        assert_eq!(idx.net_of(5), Some(5));
        assert_eq!(idx.net_of(9), Some(1));
        let changed = idx.take_changed();
        assert!(changed.contains(&5) && changed.contains(&1));
        let nodes = [5, 9, 1, 7];
        let reference = components(&[(9, 1)], &nodes);
        for n in nodes {
            assert_eq!(idx.net_of(n), Some(reference[&n]));
        }
        let v = idx.version(1);
        idx.add_edge(5, 7);
        assert_eq!(idx.version(1), v, "reseau non touche");
        assert_eq!(idx.net_of(7), Some(5));
        idx.remove_edge(5, 7);
        assert_eq!(idx.net_of(7), Some(7));
    }

    #[test]
    fn random_operations_match_rebuild() {
        let mut idx = NetIndex::new();
        let mut edges: BTreeSet<(EntityId, EntityId)> = BTreeSet::new();
        let mut nodes: BTreeSet<EntityId> = BTreeSet::new();
        let mut h = 12345u64;
        for step in 0..3000u64 {
            h = crate::rng::mix64(h ^ step);
            let a = (h % 60) as EntityId + 1;
            let b = ((h >> 20) % 60) as EntityId + 1;
            match (h >> 40) % 4 {
                0 | 1 => {
                    if !nodes.contains(&a) {
                        let nb: Vec<EntityId> =
                            nodes.iter().copied().filter(|n| (n + a) % 7 == 0).collect();
                        idx.add_node(a, &nb);
                        nodes.insert(a);
                        for n in nb {
                            edges.insert((a.min(n), a.max(n)));
                        }
                    } else if nodes.contains(&b) && a != b {
                        idx.add_edge(a, b);
                        edges.insert((a.min(b), a.max(b)));
                    }
                }
                2 => {
                    if nodes.remove(&a) {
                        idx.remove_node(a);
                        edges.retain(|&(x, y)| x != a && y != a);
                    }
                }
                _ => {
                    if edges.remove(&(a.min(b), a.max(b))) {
                        idx.remove_edge(a, b);
                    }
                }
            }
        }
        let nl: Vec<EntityId> = nodes.iter().copied().collect();
        let el: Vec<_> = edges.iter().copied().collect();
        let reference = components(&el, &nl);
        for n in &nl {
            assert_eq!(idx.net_of(*n), Some(reference[n]));
        }
        assert_eq!(
            idx.net_count(),
            reference.values().collect::<BTreeSet<_>>().len()
        );
    }
}
