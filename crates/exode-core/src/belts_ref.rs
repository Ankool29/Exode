//! Reference pour les tests : l'ancienne simulation des convoyeurs, case par case.
//!
//! Code d'origine de `sim.rs` (`Belt::insert_at`, `move_lane`, prise par les bras), deplace
//! ici sans changement de regle. Chaque objet de chaque voie est deplace a chaque tick.

use crate::belts::{BeltBackend, Topology};
use crate::content::ItemId;
use crate::entity::{BeltItem, EntityId, Feed, LANE_LEN, MID, SPACING};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct RefBelt {
    /// Objets tries de l'avant (pos la plus grande) vers l'arriere.
    pub lanes: [Vec<BeltItem>; 2],
    pub feed: Feed,
    pub speed: u32,
}

impl RefBelt {
    pub fn rear(&self, lane: usize) -> Option<u32> {
        self.lanes[lane].last().map(|b| b.pos)
    }
    /// Insertion avec controle d'ecart. Renvoie vrai si l'objet a ete pose.
    pub fn insert_at(&mut self, lane: usize, item: ItemId, pos: u32) -> bool {
        let l = &mut self.lanes[lane];
        let idx = l.iter().position(|b| b.pos < pos).unwrap_or(l.len());
        if idx > 0 && l[idx - 1].pos < pos + SPACING {
            return false;
        }
        if idx < l.len() && l[idx].pos + SPACING > pos {
            return false;
        }
        l.insert(idx, BeltItem { item, pos });
        true
    }
}

#[derive(Default)]
pub struct RefBelts {
    belts: BTreeMap<EntityId, RefBelt>,
    order: Vec<EntityId>,
}

impl RefBelts {
    /// Deplace les objets d'une voie. Traitement de l'aval vers l'amont : la case suivante
    /// a deja bouge, ce qui permet a une ligne compacte d'avancer d'un seul bloc.
    ///
    /// Limite connue : une boucle fermee remplie a 100 %, sans aucun vide, reste immobile.
    fn move_lane(&mut self, id: EntityId, lane: usize) {
        let speed = self.belts[&id].speed;
        let feed = match self.belts.get(&id) {
            Some(b) if !b.lanes[lane].is_empty() => b.feed,
            _ => return,
        };
        // limite de l'objet de tete, dans le repere de cette case (peut depasser LANE_LEN)
        let (limit, target_lane, next) = match feed {
            Feed::Same(n) => {
                let q = self.belts[&n].rear(lane);
                let lim = match q {
                    Some(q) => LANE_LEN + q - SPACING,
                    None => LANE_LEN + speed,
                };
                (lim, lane, Some(n))
            }
            Feed::Side(n, l) => (LANE_LEN, l as usize, Some(n)),
            Feed::None => (LANE_LEN, lane, None),
        };
        let mut transferred: Option<(ItemId, u32)> = None;
        {
            let b = self.belts.get_mut(&id).unwrap();
            let items = &mut b.lanes[lane];
            let mut ahead = limit as i64;
            let mut drop_front = false;
            for (k, it) in items.iter_mut().enumerate() {
                let mut target = (it.pos as i64 + speed as i64).min(ahead).max(it.pos as i64);
                if k == 0 && target > LANE_LEN as i64 && matches!(feed, Feed::Same(_)) {
                    transferred = Some((it.item, (target - LANE_LEN as i64) as u32));
                    drop_front = true;
                } else {
                    target = target.min(LANE_LEN as i64);
                    it.pos = target as u32;
                }
                ahead = target - SPACING as i64;
            }
            if drop_front {
                items.remove(0);
            }
        }
        if let (Some((item, entry)), Some(n)) = (transferred, next) {
            let nb = self.belts.get_mut(&n).unwrap();
            nb.lanes[target_lane].push(BeltItem { item, pos: entry });
        }
        // insertion laterale : l'objet de tete attend en bout de case
        if let Feed::Side(n, l) = feed {
            let front = self.belts[&id].lanes[lane].first().copied();
            if let Some(f) = front.filter(|f| f.pos >= LANE_LEN) {
                let ok = self
                    .belts
                    .get_mut(&n)
                    .unwrap()
                    .insert_at(l as usize, f.item, MID);
                if ok {
                    self.belts.get_mut(&id).unwrap().lanes[lane].remove(0);
                }
            }
        }
    }
}

impl BeltBackend for RefBelts {
    fn rebuild(&mut self, topo: &Topology) {
        let mut old = std::mem::take(&mut self.belts);
        for &(id, _, _, speed) in &topo.belts {
            let mut b = old.remove(&id).unwrap_or_default();
            b.feed = topo.feed[&id];
            b.speed = speed;
            self.belts.insert(id, b);
        }
        self.order = topo.order.clone();
    }

    fn seed(&mut self, id: EntityId, lanes: [Vec<BeltItem>; 2]) {
        self.belts.entry(id).or_default().lanes = lanes;
    }

    fn step(&mut self) {
        let order = std::mem::take(&mut self.order);
        for &id in &order {
            for lane in 0..2 {
                self.move_lane(id, lane);
            }
        }
        self.order = order;
    }

    fn lanes(&self, id: EntityId) -> [Vec<BeltItem>; 2] {
        self.belts
            .get(&id)
            .map(|b| b.lanes.clone())
            .unwrap_or_default()
    }

    fn insert_at(&mut self, id: EntityId, lane: usize, item: ItemId, pos: u32) -> bool {
        match self.belts.get_mut(&id) {
            Some(b) => b.insert_at(lane, item, pos),
            None => false,
        }
    }

    fn take(&mut self, id: EntityId, accepts: &dyn Fn(ItemId) -> bool) -> Option<ItemId> {
        let b = self.belts.get_mut(&id)?;
        // objet le plus en avant accepte, voie 0 prioritaire en cas d'egalite
        let mut best: Option<(u32, usize, usize)> = None;
        for lane in 0..2 {
            for (idx, it) in b.lanes[lane].iter().enumerate() {
                if accepts(it.item) {
                    if best.map(|bb| it.pos > bb.0).unwrap_or(true) {
                        best = Some((it.pos, lane, idx));
                    }
                    break;
                }
            }
        }
        let (_, lane, idx) = best?;
        Some(b.lanes[lane].remove(idx).item)
    }

    fn remove_tile(&mut self, id: EntityId) -> Vec<ItemId> {
        self.belts
            .remove(&id)
            .map(|b| b.lanes.iter().flatten().map(|it| it.item).collect())
            .unwrap_or_default()
    }

    fn for_each_item(&self, f: &mut dyn FnMut(ItemId)) {
        for b in self.belts.values() {
            for it in b.lanes.iter().flatten() {
                f(it.item);
            }
        }
    }

    fn visit(&self, ids: &[EntityId], f: &mut dyn FnMut(EntityId, usize, BeltItem)) {
        for &id in ids {
            if let Some(b) = self.belts.get(&id) {
                for lane in 0..2 {
                    for it in &b.lanes[lane] {
                        f(id, lane, *it);
                    }
                }
            }
        }
    }

    fn item_count(&self) -> usize {
        self.belts
            .values()
            .map(|b| b.lanes[0].len() + b.lanes[1].len())
            .sum()
    }
}
