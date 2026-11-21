//! Convoyeurs : topologie et lignes de transport (banc F1).
//!
//! Les cases de convoyeur reliees par un transfert `Feed::Same` (tout droit ou virage) sont
//! regroupees en lignes. Chaque voie d'une ligne stocke ses objets par ecarts, de l'avant
//! (le bout de la ligne) vers l'arriere : `gap` = distance a l'objet precedent, ou au bout de la
//! ligne pour l'objet de tete. A chaque tick, seuls les ecarts ou un groupe rattrape un autre
//! changent ; un groupe compact qui avance a pleine vitesse ou une file arretee ne coutent rien.
//! Une voie qui n'a pas bouge s'endort et n'est reveillee que si son aval change (objet
//! retire, insertion possible, convoyeur modifie).
//!
//! Le comportement observable est celui de l'ancienne simulation case par case (conservee
//! comme reference dans `belts_ref`, verifiee par le test d'equivalence) :
//! - ordre de traitement de l'aval vers l'amont, tete de cycle comprise ;
//! - LANE_LEN = 256 unites par case, SPACING = 64, vitesse en unites par tick ;
//! - une case ne regarde que l'arriere de la case suivante ; insertion laterale au milieu (MID).
//!
//! Seule difference voulue : une boucle fermee remplie a 100 % circule (tous les objets
//! avancent ensemble) au lieu de rester figee.
//!
//! Hypothese : vitesse d'un convoyeur <= SPACING (verifiee au chargement du contenu).

use crate::content::ItemId;
use crate::entity::{BeltItem, EntityId, Feed, LANE_LEN, MID, SPACING};
use crate::geom::{Dir, Pos};
use crate::sim::FastMap;
use std::collections::{BTreeMap, VecDeque};

/// Longueur maximale d'une ligne, en cases. Une chaine plus longue est coupee en plusieurs
/// lignes (le comportement ne change pas) : cela borne le cout des recherches par case
/// (bras, insertion laterale, affichage).
pub const MAX_LINE_TILES: usize = 64;

// ---------------------------------------------------------------------- topologie

/// Topologie des convoyeurs, recalculee quand un convoyeur est pose, retire ou tourne.
pub(crate) struct Topology {
    /// (id, case, direction, vitesse), dans l'ordre des identifiants.
    pub belts: Vec<(EntityId, Pos, Dir, u32)>,
    /// Tables de recherche seulement (jamais parcourues).
    pub curve: FastMap<EntityId, Option<Dir>>,
    pub feed: FastMap<EntityId, Feed>,
    /// Ordre de traitement case par case : aval vers amont, cycles reperes.
    pub order: Vec<EntityId>,
}

/// Calcul des virages, des sorties et de l'ordre de traitement (regles d'origine inchangees ;
/// les tables qui ne servent qu'aux recherches sont des tables de hachage).
pub(crate) fn topology(
    belts: Vec<(EntityId, Pos, Dir, u32)>,
    belt_at: impl Fn(Pos) -> Option<(EntityId, Dir)>,
) -> Topology {
    // 1. virages
    let mut curve: FastMap<EntityId, Option<Dir>> = FastMap::default();
    for &(id, p, d, _) in &belts {
        let back = belt_at(p.step(d.opposite()))
            .map(|(_, bd)| bd == d)
            .unwrap_or(false);
        let from_left = belt_at(p.step(d.left()))
            .map(|(_, bd)| bd == d.right())
            .unwrap_or(false);
        let from_right = belt_at(p.step(d.right()))
            .map(|(_, bd)| bd == d.left())
            .unwrap_or(false);
        let c = if !back && from_left && !from_right {
            Some(d.left())
        } else if !back && from_right && !from_left {
            Some(d.right())
        } else {
            None
        };
        curve.insert(id, c);
    }
    // 2. sorties
    let mut feed: FastMap<EntityId, Feed> = FastMap::default();
    // amont de chaque case, dans l'ordre des identifiants
    let mut upstream: FastMap<EntityId, Vec<EntityId>> = FastMap::default();
    for &(id, p, d, _) in &belts {
        let f = match belt_at(p.step(d)) {
            Some((nid, nd)) if nd != d.opposite() => {
                if nd == d || curve[&nid] == Some(d.opposite()) {
                    Feed::Same(nid)
                } else {
                    let side = d.opposite();
                    Feed::Side(nid, if side == nd.left() { 0 } else { 1 })
                }
            }
            _ => Feed::None,
        };
        if let Feed::Same(n) | Feed::Side(n, _) = f {
            upstream.entry(n).or_default().push(id);
        }
        feed.insert(id, f);
    }
    // 3. ordre aval -> amont, cycles reperes
    let mut order = Vec::with_capacity(belts.len());
    let mut seen: FastMap<EntityId, bool> = FastMap::default();
    let push_upstream =
        |start: EntityId, order: &mut Vec<EntityId>, seen: &mut FastMap<EntityId, bool>| {
            let mut queue = VecDeque::new();
            queue.push_back(start);
            seen.insert(start, true);
            while let Some(b) = queue.pop_front() {
                order.push(b);
                if let Some(ups) = upstream.get(&b) {
                    for &u in ups {
                        if !seen.contains_key(&u) {
                            seen.insert(u, true);
                            queue.push_back(u);
                        }
                    }
                }
            }
        };
    for &(id, _, _, _) in &belts {
        if feed[&id] == Feed::None && !seen.contains_key(&id) {
            push_upstream(id, &mut order, &mut seen);
        }
    }
    for &(id, _, _, _) in &belts {
        if seen.contains_key(&id) {
            continue;
        }
        // remonte vers l'aval jusqu'a boucler : le premier noeud revu est la tete du cycle
        let mut walk: Vec<EntityId> = Vec::new();
        let mut cur = id;
        let head = loop {
            if let Some(i) = walk.iter().position(|&w| w == cur) {
                break walk[i];
            }
            if seen.contains_key(&cur) {
                break cur;
            }
            walk.push(cur);
            cur = match feed[&cur] {
                Feed::Same(n) | Feed::Side(n, _) => n,
                Feed::None => cur,
            };
        };
        if !seen.contains_key(&head) {
            push_upstream(head, &mut order, &mut seen);
        }
    }
    Topology {
        belts,
        curve,
        feed,
        order,
    }
}

// ---------------------------------------------------------------------- interface commune

/// Operations dont la simulation a besoin, quelle que soit la representation des objets.
pub(crate) trait BeltBackend {
    /// Nouvelle topologie : les objets de chaque case sont conserves.
    fn rebuild(&mut self, topo: &Topology);
    /// Objets d'une case lus dans une sauvegarde, poses tels quels a la prochaine reconstruction.
    fn seed(&mut self, id: EntityId, lanes: [Vec<BeltItem>; 2]);
    /// Un tick de deplacement.
    fn step(&mut self);
    /// Objets d'une case, voie par voie, de l'avant vers l'arriere.
    fn lanes(&self, id: EntityId) -> [Vec<BeltItem>; 2];
    /// Pose avec controle d'ecart dans la case (regle d'origine `Belt::insert_at`).
    fn insert_at(&mut self, id: EntityId, lane: usize, item: ItemId, pos: u32) -> bool;
    /// Prend l'objet le plus en avant accepte (voie 0 prioritaire en cas d'egalite).
    fn take(&mut self, id: EntityId, accepts: &dyn Fn(ItemId) -> bool) -> Option<ItemId>;
    /// Retire tous les objets d'une case (convoyeur demonte).
    fn remove_tile(&mut self, id: EntityId) -> Vec<ItemId>;
    /// Parcourt tous les objets (comptes et bilan matiere).
    fn for_each_item(&self, f: &mut dyn FnMut(ItemId));
    /// Objets des cases demandees : (case, voie, objet).
    fn visit(&self, ids: &[EntityId], f: &mut dyn FnMut(EntityId, usize, BeltItem));
    fn item_count(&self) -> usize;
}

// ---------------------------------------------------------------------- lignes

/// Case et position dans la case d'un objet place a `x` sur une ligne.
/// Un objet a la frontiere de deux cases appartient a la case amont (position LANE_LEN),
/// sauf s'il a ete pose a la position 0 de la case aval et n'a pas encore bouge (`zero`).
#[inline]
fn tile_pos(x: u32, zero: bool) -> (u32, u32) {
    if x == 0 {
        (0, 0)
    } else if zero {
        (x / LANE_LEN, 0)
    } else if x % LANE_LEN == 0 {
        (x / LANE_LEN - 1, LANE_LEN)
    } else {
        (x / LANE_LEN, x % LANE_LEN)
    }
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    /// Distance a l'objet precedent (vers l'avant) ; pour l'objet de tete, au bout de la ligne.
    gap: u32,
    item: ItemId,
    /// Pose a la position 0 d'une case qui n'est pas la premiere, pas encore deplace.
    zero: bool,
}

#[inline]
fn tight(k: usize, gap: u32) -> u32 {
    (k > 0 && gap < SPACING) as u32
}

/// Une voie d'une ligne.
#[derive(Debug, Default)]
struct Lane {
    /// De l'avant vers l'arriere.
    items: VecDeque<Slot>,
    /// Somme des ecarts = longueur de la ligne - position de l'objet de queue (0 si vide).
    total: u32,
    /// Les objets 0..run forment un groupe compact (ecarts internes = SPACING).
    run: u32,
    /// Ecarts plus courts que SPACING et objets `zero` : ils imposent le calcul objet par objet.
    irregular: u32,
    awake: bool,
    /// Boucle pleine en rotation pendant ce tick.
    rotated: bool,
}

/// Objets d'une case trouves dans une voie.
struct Span {
    /// Indice du premier objet de la case (ou indice d'insertion si la case est vide).
    i0: usize,
    cnt: usize,
    xs: [u32; 8],
    /// Position de l'objet juste devant la case, et de celui juste derriere.
    x_ahead: Option<u32>,
    x_behind: Option<u32>,
}

impl Lane {
    fn x_rear(&self, len: u32) -> u32 {
        len - self.total
    }

    fn extend_run(&mut self) {
        let n = self.items.len() as u32;
        if n == 0 {
            self.run = 0;
            return;
        }
        if self.run == 0 {
            self.run = 1;
        }
        while self.run < n && self.items[self.run as usize].gap == SPACING {
            self.run += 1;
        }
    }

    fn recount(&mut self) {
        self.irregular = 0;
        for (k, s) in self.items.iter().enumerate() {
            self.irregular += tight(k, s.gap) + s.zero as u32;
        }
        self.run = 0;
        self.extend_run();
    }

    /// Insere a l'indice `idx`, a la position `x`, entre les objets en `x_ahead` et `x_behind`.
    fn insert(
        &mut self,
        len: u32,
        idx: usize,
        item: ItemId,
        x: u32,
        zero: bool,
        x_ahead: Option<u32>,
        x_behind: Option<u32>,
    ) {
        let n = self.items.len();
        let gap = match x_ahead {
            Some(a) => a - x,
            None => len - x,
        };
        if idx < n {
            let old = self.items[idx].gap;
            self.irregular -= tight(idx, old);
            let ng = x - x_behind.expect("objet derriere");
            self.items[idx].gap = ng;
            self.irregular += tight(idx + 1, ng);
        } else {
            self.total = len - x;
        }
        self.irregular += tight(idx, gap) + zero as u32;
        self.items.insert(idx, Slot { gap, item, zero });
        if idx == 0 {
            self.run = 1;
        } else if (idx as u32) < self.run {
            self.run = idx as u32;
        }
        self.extend_run();
    }

    fn push_back(&mut self, len: u32, item: ItemId, x: u32) {
        let n = self.items.len();
        let ahead = (n > 0).then(|| self.x_rear(len));
        self.insert(len, n, item, x, false, ahead, None);
    }

    /// Retire l'objet `idx` sans deplacer les autres.
    fn remove(&mut self, idx: usize) -> Slot {
        let n = self.items.len();
        let s = self.items[idx];
        self.irregular -= tight(idx, s.gap) + s.zero as u32;
        if idx + 1 < n {
            let g = self.items[idx + 1].gap;
            self.irregular -= tight(idx + 1, g);
            let ng = g + s.gap;
            self.items[idx + 1].gap = ng;
            self.irregular += tight(idx, ng);
        } else if idx == 0 {
            self.total = 0;
        } else {
            self.total -= s.gap;
        }
        self.items.remove(idx);
        if idx == 0 {
            self.run = self.run.saturating_sub(1);
        } else if (idx as u32) < self.run {
            self.run = idx as u32;
        }
        self.extend_run();
        s
    }

    /// Retire l'objet de tete qui vient de depasser le bout de la ligne de `over` unites,
    /// les autres ecarts etant deja a jour.
    fn pop_over(&mut self, over: u32) -> Slot {
        let s = self.items.pop_front().expect("objet de tete");
        self.irregular -= s.zero as u32;
        if let Some(f) = self.items.front_mut() {
            let g = f.gap;
            self.irregular -= tight(1, g);
            f.gap = g - over;
        } else {
            self.total = 0;
        }
        self.run = self.run.saturating_sub(1);
        self.extend_run();
        s
    }

    /// Objets de la case `t` (parcours depuis le bout le plus proche).
    fn locate(&self, len: u32, t: u32) -> Span {
        let n = self.items.len();
        let mut sp = Span {
            i0: 0,
            cnt: 0,
            xs: [0; 8],
            x_ahead: None,
            x_behind: None,
        };
        if n == 0 {
            return sp;
        }
        let tiles = len / LANE_LEN;
        if t * 2 >= tiles {
            // depuis l'avant
            let mut k = 0;
            let mut x = len - self.items[0].gap;
            loop {
                if tile_pos(x, self.items[k].zero).0 <= t {
                    break;
                }
                sp.x_ahead = Some(x);
                k += 1;
                if k == n {
                    sp.i0 = n;
                    return sp;
                }
                x -= self.items[k].gap;
            }
            sp.i0 = k;
            loop {
                if tile_pos(x, self.items[k].zero).0 < t {
                    sp.x_behind = Some(x);
                    break;
                }
                if sp.cnt < sp.xs.len() {
                    sp.xs[sp.cnt] = x;
                    sp.cnt += 1;
                }
                k += 1;
                if k == n {
                    break;
                }
                x -= self.items[k].gap;
            }
        } else {
            // depuis l'arriere
            let mut k = n - 1;
            let mut x = self.x_rear(len);
            loop {
                if tile_pos(x, self.items[k].zero).0 >= t {
                    break;
                }
                sp.x_behind = Some(x);
                if k == 0 {
                    sp.i0 = 0;
                    return sp;
                }
                x += self.items[k].gap;
                k -= 1;
            }
            let end = k + 1;
            let mut rev = [0u32; 8];
            let mut cnt = 0;
            loop {
                if tile_pos(x, self.items[k].zero).0 > t {
                    sp.x_ahead = Some(x);
                    break;
                }
                if cnt < rev.len() {
                    rev[cnt] = x;
                    cnt += 1;
                }
                if k == 0 {
                    break;
                }
                x += self.items[k].gap;
                k -= 1;
            }
            sp.cnt = cnt;
            sp.i0 = end - cnt;
            for j in 0..cnt {
                sp.xs[j] = rev[cnt - 1 - j];
            }
        }
        debug_assert!(sp.cnt < 8, "trop d'objets dans une case");
        sp
    }

    /// Pose dans la case `t` a la position `p`, avec la regle d'ecart de la case.
    fn insert_in_tile(&mut self, len: u32, t: u32, item: ItemId, p: u32) -> bool {
        let sp = self.locate(len, t);
        let pos_of = |j: usize| tile_pos(sp.xs[j], self.items[sp.i0 + j].zero).1;
        let local = (0..sp.cnt).find(|&j| pos_of(j) < p).unwrap_or(sp.cnt);
        if local > 0 && pos_of(local - 1) < p + SPACING {
            return false;
        }
        if local < sp.cnt && pos_of(local) + SPACING > p {
            return false;
        }
        let x = t * LANE_LEN + p;
        let ahead = if local > 0 {
            Some(sp.xs[local - 1])
        } else {
            sp.x_ahead
        };
        let behind = if local < sp.cnt {
            Some(sp.xs[local])
        } else {
            sp.x_behind
        };
        self.insert(len, sp.i0 + local, item, x, p == 0 && t > 0, ahead, behind);
        true
    }

    /// Positions de tous les objets, de l'avant vers l'arriere.
    fn positions(&self, len: u32) -> Vec<u32> {
        let mut v = Vec::with_capacity(self.items.len());
        let mut x = len as i64;
        for s in &self.items {
            x -= s.gap as i64;
            v.push(x as u32);
        }
        v
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// Rien en sortie : l'objet de tete s'arrete au bout.
    Wall,
    /// Insertion laterale au milieu de la case `tile` de la ligne `line`, voie `lane`.
    Side { line: u32, tile: u32, lane: u8 },
    /// Meme voie, au debut de la ligne `line`. `deferred` : coupure d'un cycle, la ligne
    /// suivante est traitee apres celle-ci (on lit son arriere avant son deplacement).
    Same { line: u32, deferred: bool },
}

#[derive(Debug)]
struct Line {
    /// Cases, du debut (amont) vers le bout (aval).
    tiles: Vec<EntityId>,
    /// Longueur en unites (cases x LANE_LEN).
    len: u32,
    speed: u32,
    end: End,
    lanes: [Lane; 2],
    /// Lignes dont la sortie depend de chaque voie de celle-ci : (ligne, masque des voies).
    feeders: [Vec<(u32, u8)>; 2],
}

/// Boucle fermee faite uniquement de transferts `Same`.
#[derive(Debug)]
struct Ring {
    /// Lignes dans le sens de marche.
    lines: Vec<u32>,
    tiles: u32,
}

/// Convoyeurs regroupes en lignes de transport.
#[derive(Default)]
pub struct LineBelts {
    /// Dans l'ordre de traitement (aval d'abord).
    lines: Vec<Line>,
    tile_of: FastMap<EntityId, (u32, u32)>,
    rings: Vec<Ring>,
    seeded: Vec<(EntityId, [Vec<BeltItem>; 2])>,
    /// Ticks de rotation de boucles pleines (voie par voie), pour les tests.
    pub(crate) ring_rotations: u64,
}

impl LineBelts {
    pub fn new() -> LineBelts {
        LineBelts::default()
    }

    /// Nombre de lignes (diagnostic et bancs).
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Voies eveillees (diagnostic et bancs).
    pub fn awake_lanes(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.lanes.iter().filter(|ln| ln.awake).count())
            .sum()
    }

    /// Une voie a change : elle reste eveillee et reveille les lignes qui en dependent.
    fn mark_changed(&mut self, li: usize, lane: usize) {
        self.lines[li].lanes[lane].awake = true;
        for k in 0..self.lines[li].feeders[lane].len() {
            let (f, mask) = self.lines[li].feeders[lane][k];
            for l in 0..2 {
                if mask & (1 << l) != 0 {
                    self.lines[f as usize].lanes[l].awake = true;
                }
            }
        }
    }

    /// Deplace une voie d'une ligne. Renvoie vrai si quelque chose a change.
    fn process_lane(&mut self, li: usize, lane: usize) -> bool {
        let (len, speed, end) = {
            let l = &self.lines[li];
            (l.len, l.speed, l.end)
        };
        if self.lines[li].lanes[lane].items.is_empty() {
            return false;
        }
        let x0 = len - self.lines[li].lanes[lane].items[0].gap;
        let limit: i64 = match end {
            End::Same { line, .. } => {
                let t = &self.lines[line as usize];
                let tl = &t.lanes[lane];
                if tl.items.is_empty() {
                    (len + speed) as i64
                } else {
                    len as i64 + tl.x_rear(t.len) as i64 - SPACING as i64
                }
            }
            _ => len as i64,
        };
        let d0 = (limit - x0 as i64).clamp(0, speed as i64) as u32;
        let mut changed = d0 > 0;
        let transfer = matches!(end, End::Same { .. }) && x0 + d0 > len;
        if let (true, End::Same { line, .. }) = (transfer, end) {
            // l'objet passe sur la ligne suivante, a l'arriere ; si celle-ci n'a pas encore
            // bouge (cycle), il bougera encore avec elle pendant ce tick, comme avant
            let item = self.lines[li].lanes[lane].items[0].item;
            let j = line as usize;
            let jl = self.lines[j].len;
            self.lines[j].lanes[lane].push_back(jl, item, x0 + d0 - len);
            if j != li {
                self.mark_changed(j, lane);
            }
        }
        let ln = &mut self.lines[li].lanes[lane];
        if ln.irregular > 0 {
            changed |= Self::walk_exact(ln, len, speed, limit, transfer);
        } else {
            changed |= Self::walk_fast(ln, speed, d0, transfer);
        }
        if transfer {
            ln.pop_over(x0 + d0 - len);
        }
        // insertion laterale : l'objet de tete attend en bout de ligne
        if let End::Side {
            line,
            tile,
            lane: tl,
        } = end
        {
            if let Some(f) = self.lines[li].lanes[lane].items.front().copied() {
                if f.gap == 0 {
                    let j = line as usize;
                    let jl = self.lines[j].len;
                    if self.lines[j].lanes[tl as usize].insert_in_tile(jl, tile, f.item, MID) {
                        self.lines[li].lanes[lane].remove(0);
                        self.mark_changed(j, tl as usize);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    /// Cas courant (aucun ecart court, aucun objet `zero`) : seul le deficit de mouvement
    /// est propage, a travers les groupes compacts, jusqu'au premier ecart qui l'absorbe.
    fn walk_fast(ln: &mut Lane, speed: u32, d0: u32, transfer: bool) -> bool {
        let n = ln.items.len();
        if !transfer {
            ln.items[0].gap -= d0;
        }
        let mut e = speed - d0;
        let mut consumed = false;
        let mut k = ln.run as usize;
        while e > 0 && k < n {
            let g = ln.items[k].gap;
            let c = e.min(g - SPACING);
            ln.items[k].gap = g - c;
            e -= c;
            consumed = true;
            if g - c == SPACING {
                ln.run = k as u32 + 1;
                ln.extend_run();
                k = ln.run as usize;
            } else {
                break;
            }
        }
        let d_rear = if k >= n { speed - e } else { speed };
        if n > 1 || !transfer {
            ln.total -= d_rear;
        }
        consumed
    }

    /// Calcul objet par objet, identique a la regle d'origine.
    fn walk_exact(ln: &mut Lane, len: u32, speed: u32, limit: i64, transfer: bool) -> bool {
        let old = ln.positions(len);
        let n = old.len();
        let mut new = vec![0i64; n];
        let mut changed = false;
        let mut ahead = limit;
        for k in 0..n {
            let x = old[k] as i64;
            let mut target = (x + speed as i64).min(ahead).max(x);
            if !(k == 0 && transfer) {
                target = target.min(len as i64);
            }
            if target != x {
                ln.items[k].zero = false;
                changed = true;
            }
            new[k] = target;
            ahead = target - SPACING as i64;
        }
        if !transfer {
            ln.items[0].gap = (len as i64 - new[0]) as u32;
        }
        for k in 1..n {
            ln.items[k].gap = (new[k - 1] - new[k]) as u32;
        }
        ln.total = if n > 1 || !transfer {
            (len as i64 - new[n - 1]) as u32
        } else {
            0
        };
        ln.recount();
        changed
    }

    fn ring_full(&self, r: usize, lane: usize) -> bool {
        let ring = &self.rings[r];
        let speed = self.lines[ring.lines[0] as usize].speed;
        let mut count = 0usize;
        for &l in &ring.lines {
            let line = &self.lines[l as usize];
            let ln = &line.lanes[lane];
            if ln.items.is_empty() || ln.irregular > 0 || line.speed != speed {
                return false;
            }
            count += ln.items.len();
        }
        if count as u32 * SPACING != ring.tiles * LANE_LEN {
            return false;
        }
        let m = ring.lines.len();
        for i in 0..m {
            let a = &self.lines[ring.lines[i] as usize].lanes[lane];
            let b_line = &self.lines[ring.lines[(i + 1) % m] as usize];
            let b = &b_line.lanes[lane];
            if a.items[0].gap + b.x_rear(b_line.len) < SPACING {
                return false;
            }
        }
        true
    }

    /// Boucle pleine : tous les objets avancent ensemble de la vitesse du convoyeur.
    fn rotate_ring(&mut self, r: usize, lane: usize) {
        let ring_lines = self.rings[r].lines.clone();
        let m = ring_lines.len();
        let mut moved: Vec<Option<(ItemId, u32)>> = Vec::with_capacity(m);
        for &l in &ring_lines {
            let line = &mut self.lines[l as usize];
            let (len, speed) = (line.len, line.speed);
            let ln = &mut line.lanes[lane];
            let x0 = len - ln.items[0].gap;
            ln.total -= speed;
            if x0 + speed > len {
                let s = ln.pop_over(x0 + speed - len);
                moved.push(Some((s.item, x0 + speed - len)));
            } else {
                ln.items[0].gap -= speed;
                moved.push(None);
            }
        }
        for i in 0..m {
            if let Some((item, x)) = moved[i] {
                let l = ring_lines[(i + 1) % m] as usize;
                let len = self.lines[l].len;
                self.lines[l].lanes[lane].push_back(len, item, x);
            }
        }
        self.ring_rotations += 1;
        for &l in &ring_lines {
            self.lines[l as usize].lanes[lane].rotated = true;
            self.mark_changed(l as usize, lane);
        }
    }

    fn lookup(&self, id: EntityId) -> Option<(usize, u32)> {
        self.tile_of.get(&id).map(|&(l, t)| (l as usize, t))
    }
}

impl BeltBackend for LineBelts {
    fn rebuild(&mut self, topo: &Topology) {
        let mut old_lines = std::mem::take(&mut self.lines);
        let old_tile_of = std::mem::take(&mut self.tile_of);
        // objets lus dans une sauvegarde, case par case
        let mut store: FastMap<EntityId, [Vec<BeltItem>; 2]> = FastMap::default();
        for (id, lanes) in self.seeded.drain(..) {
            store.insert(id, lanes);
        }
        self.rings.clear();

        let mut rank: FastMap<EntityId, usize> = FastMap::default();
        for (i, &id) in topo.order.iter().enumerate() {
            rank.insert(id, i);
        }
        let mut speed: FastMap<EntityId, u32> = FastMap::default();
        for &(id, _, _, s) in &topo.belts {
            speed.insert(id, s);
        }
        // tete de cycle : seule case traitee avant sa sortie
        let is_head = |b: EntityId| match topo.feed[&b] {
            Feed::Same(n) | Feed::Side(n, _) => rank[&b] < rank[&n],
            Feed::None => false,
        };
        // liens de chaine : transfert Same, hors tete de cycle, meme vitesse
        let mut linked_pred: FastMap<EntityId, EntityId> = FastMap::default();
        for &(id, _, _, s) in &topo.belts {
            if let Feed::Same(n) = topo.feed[&id] {
                if !is_head(id) && speed[&n] == s {
                    linked_pred.insert(n, id);
                }
            }
        }
        // une tete qui insere sur le cote forme sa propre ligne : son insertion a lieu
        // avant le deplacement du reste de sa chaine
        for &(id, _, _, _) in &topo.belts {
            if matches!(topo.feed[&id], Feed::Side(..)) && is_head(id) {
                linked_pred.remove(&id);
            }
        }
        // chaines, coupees en lignes de MAX_LINE_TILES cases au plus
        let mut chains: Vec<Vec<EntityId>> = Vec::new();
        for &(id, _, _, _) in &topo.belts {
            if linked_pred.contains_key(&id) {
                continue;
            }
            let mut chain = vec![id];
            let mut cur = id;
            while let Feed::Same(n) = topo.feed[&cur] {
                if linked_pred.get(&n) != Some(&cur) {
                    break;
                }
                chain.push(n);
                cur = n;
            }
            for piece in chain.chunks(MAX_LINE_TILES) {
                chains.push(piece.to_vec());
            }
        }
        // ordre de traitement : rang de la derniere case dans l'ordre case par case
        chains.sort_by_key(|c| rank[c.last().unwrap()]);
        for (li, c) in chains.iter().enumerate() {
            for (t, &id) in c.iter().enumerate() {
                self.tile_of.insert(id, (li as u32, t as u32));
            }
        }
        // anciennes lignes : reprises telles quelles si leurs cases n'ont pas change,
        // sinon leurs objets sont repris case par case
        let mut reused: Vec<Option<[Lane; 2]>> = Vec::with_capacity(chains.len());
        let mut split_old: FastMap<usize, bool> = FastMap::default();
        for c in &chains {
            let same = match old_tile_of.get(&c[0]) {
                Some(&(oli, 0)) => old_lines[oli as usize].tiles == *c,
                _ => false,
            };
            if same {
                let oli = old_tile_of[&c[0]].0 as usize;
                reused.push(Some(std::mem::take(&mut old_lines[oli].lanes)));
                continue;
            }
            reused.push(None);
            for id in c {
                if let Some(&(oli, _)) = old_tile_of.get(id) {
                    split_old.insert(oli as usize, true);
                }
            }
        }
        for (oli, line) in old_lines.iter().enumerate() {
            if !split_old.contains_key(&oli) {
                continue;
            }
            for lane in 0..2 {
                let ln = &line.lanes[lane];
                let xs = ln.positions(line.len);
                for (k, s) in ln.items.iter().enumerate() {
                    let (t, pos) = tile_pos(xs[k], s.zero);
                    store.entry(line.tiles[t as usize]).or_default()[lane]
                        .push(BeltItem { item: s.item, pos });
                }
            }
        }
        drop(old_lines);
        for (c, lanes) in chains.into_iter().zip(reused) {
            let last = *c.last().unwrap();
            let end = match topo.feed[&last] {
                Feed::None => End::Wall,
                Feed::Side(n, l) => {
                    let (line, tile) = self.tile_of[&n];
                    End::Side {
                        line,
                        tile,
                        lane: l,
                    }
                }
                Feed::Same(n) => {
                    let (line, tile) = self.tile_of[&n];
                    debug_assert_eq!(tile, 0, "transfert Same hors debut de ligne");
                    End::Same {
                        line,
                        deferred: is_head(last),
                    }
                }
            };
            let fresh = lanes.is_none();
            self.lines.push(Line {
                len: c.len() as u32 * LANE_LEN,
                speed: speed[&last],
                tiles: c,
                end,
                lanes: lanes.unwrap_or_default(),
                feeders: [Vec::new(), Vec::new()],
            });
            if !fresh {
                continue;
            }
            // objets, de l'aval vers l'amont
            let line = self.lines.last_mut().unwrap();
            for lane in 0..2 {
                let len = line.len;
                for t in (0..line.tiles.len()).rev() {
                    let Some(items) = store.get(&line.tiles[t]) else {
                        continue;
                    };
                    for it in &items[lane] {
                        let x = t as u32 * LANE_LEN + it.pos;
                        let ln = &mut line.lanes[lane];
                        let n = ln.items.len();
                        let ahead = (n > 0).then(|| ln.x_rear(len));
                        ln.insert(len, n, it.item, x, it.pos == 0 && t > 0, ahead, None);
                    }
                }
            }
        }
        // dependances pour le reveil
        for f in 0..self.lines.len() {
            match self.lines[f].end {
                End::Same { line, .. } => {
                    self.lines[line as usize].feeders[0].push((f as u32, 0b01));
                    self.lines[line as usize].feeders[1].push((f as u32, 0b10));
                }
                End::Side { line, lane, .. } => {
                    self.lines[line as usize].feeders[lane as usize].push((f as u32, 0b11));
                }
                End::Wall => {}
            }
        }
        // boucles fermees faites uniquement de transferts Same
        for start in 0..self.lines.len() {
            let End::Same {
                line: first,
                deferred: true,
            } = self.lines[start].end
            else {
                continue;
            };
            let mut path = vec![first];
            let mut cur = first as usize;
            let closed = loop {
                if cur == start {
                    break true;
                }
                match self.lines[cur].end {
                    End::Same {
                        line,
                        deferred: false,
                    } if path.len() <= self.lines.len() => {
                        path.push(line);
                        cur = line as usize;
                    }
                    _ => break false,
                }
            };
            if closed {
                let tiles = path
                    .iter()
                    .map(|&l| self.lines[l as usize].tiles.len() as u32)
                    .sum();
                self.rings.push(Ring { lines: path, tiles });
            }
        }
        for line in &mut self.lines {
            for ln in &mut line.lanes {
                ln.awake = true;
                ln.rotated = false;
            }
        }
    }

    fn seed(&mut self, id: EntityId, lanes: [Vec<BeltItem>; 2]) {
        self.seeded.push((id, lanes));
    }

    fn step(&mut self) {
        for r in 0..self.rings.len() {
            for lane in 0..2 {
                if self.ring_full(r, lane) {
                    self.rotate_ring(r, lane);
                }
            }
        }
        for li in 0..self.lines.len() {
            for lane in 0..2 {
                let ln = &mut self.lines[li].lanes[lane];
                if ln.rotated {
                    ln.rotated = false;
                    continue;
                }
                if !ln.awake {
                    continue;
                }
                if self.process_lane(li, lane) {
                    self.mark_changed(li, lane);
                } else {
                    self.lines[li].lanes[lane].awake = false;
                }
            }
        }
    }

    fn lanes(&self, id: EntityId) -> [Vec<BeltItem>; 2] {
        let mut out = [Vec::new(), Vec::new()];
        self.visit(&[id], &mut |_, lane, it| out[lane].push(it));
        out
    }

    fn insert_at(&mut self, id: EntityId, lane: usize, item: ItemId, pos: u32) -> bool {
        let Some((li, t)) = self.lookup(id) else {
            return false;
        };
        let len = self.lines[li].len;
        let ok = self.lines[li].lanes[lane].insert_in_tile(len, t, item, pos);
        if ok {
            self.mark_changed(li, lane);
        }
        ok
    }

    fn take(&mut self, id: EntityId, accepts: &dyn Fn(ItemId) -> bool) -> Option<ItemId> {
        let (li, t) = self.lookup(id)?;
        let len = self.lines[li].len;
        // objet le plus en avant accepte, voie 0 prioritaire en cas d'egalite
        let mut best: Option<(u32, usize, usize)> = None;
        for lane in 0..2 {
            let ln = &self.lines[li].lanes[lane];
            let sp = ln.locate(len, t);
            for j in 0..sp.cnt {
                let s = ln.items[sp.i0 + j];
                if accepts(s.item) {
                    let pos = tile_pos(sp.xs[j], s.zero).1;
                    if best.map(|b| pos > b.0).unwrap_or(true) {
                        best = Some((pos, lane, sp.i0 + j));
                    }
                    break;
                }
            }
        }
        let (_, lane, idx) = best?;
        let s = self.lines[li].lanes[lane].remove(idx);
        self.mark_changed(li, lane);
        Some(s.item)
    }

    fn remove_tile(&mut self, id: EntityId) -> Vec<ItemId> {
        let mut out = Vec::new();
        let Some((li, t)) = self.lookup(id) else {
            return out;
        };
        let len = self.lines[li].len;
        for lane in 0..2 {
            let sp = self.lines[li].lanes[lane].locate(len, t);
            for _ in 0..sp.cnt {
                out.push(self.lines[li].lanes[lane].remove(sp.i0).item);
            }
            self.mark_changed(li, lane);
        }
        self.tile_of.remove(&id);
        out
    }

    fn for_each_item(&self, f: &mut dyn FnMut(ItemId)) {
        for line in &self.lines {
            for ln in &line.lanes {
                for s in &ln.items {
                    f(s.item);
                }
            }
        }
    }

    fn visit(&self, ids: &[EntityId], f: &mut dyn FnMut(EntityId, usize, BeltItem)) {
        // regroupement par ligne : chaque ligne n'est parcourue qu'une fois
        let mut by_line: BTreeMap<usize, Vec<(u32, EntityId)>> = BTreeMap::new();
        for &id in ids {
            if let Some((li, t)) = self.lookup(id) {
                by_line.entry(li).or_default().push((t, id));
            }
        }
        for (li, mut tiles) in by_line {
            let line = &self.lines[li];
            tiles.sort();
            if tiles.len() == 1 {
                let (t, id) = tiles[0];
                for lane in 0..2 {
                    let ln = &line.lanes[lane];
                    let sp = ln.locate(line.len, t);
                    for j in 0..sp.cnt {
                        let s = ln.items[sp.i0 + j];
                        let pos = tile_pos(sp.xs[j], s.zero).1;
                        f(id, lane, BeltItem { item: s.item, pos });
                    }
                }
                continue;
            }
            for lane in 0..2 {
                let ln = &line.lanes[lane];
                let mut x = line.len as i64;
                for s in &ln.items {
                    x -= s.gap as i64;
                    let (t, pos) = tile_pos(x as u32, s.zero);
                    if let Ok(k) = tiles.binary_search_by_key(&t, |e| e.0) {
                        f(tiles[k].1, lane, BeltItem { item: s.item, pos });
                    }
                }
            }
        }
    }

    fn item_count(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.lanes[0].items.len() + l.lanes[1].items.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_positions() {
        assert_eq!(tile_pos(0, false), (0, 0));
        assert_eq!(tile_pos(256, false), (0, 256));
        assert_eq!(tile_pos(256, true), (1, 0));
        assert_eq!(tile_pos(300, false), (1, 44));
    }

    #[test]
    fn lane_insert_remove_keeps_gaps() {
        let len = 4 * LANE_LEN;
        let mut ln = Lane::default();
        assert!(ln.insert_in_tile(len, 3, 1, 128));
        assert!(ln.insert_in_tile(len, 1, 2, 128));
        assert!(ln.insert_in_tile(len, 2, 3, 0));
        assert!(!ln.insert_in_tile(len, 2, 4, 30));
        assert!(ln.insert_in_tile(len, 0, 5, 0));
        assert_eq!(ln.positions(len), vec![3 * 256 + 128, 512, 256 + 128, 0]);
        assert_eq!(ln.irregular, 1, "objet pose en 0 d'une case aval");
        ln.remove(1);
        assert_eq!(ln.positions(len), vec![3 * 256 + 128, 256 + 128, 0]);
        assert_eq!(ln.irregular, 0);
        assert_eq!(ln.total, len);
    }
}
