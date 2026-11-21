//! Monde procedural par chunks creux (v0.4 section 4).
//!
//! - Terrain plat : herbe, terre, sable, eau ; obstacles : arbres, rochers.
//! - Generation progressive, pure fonction de (graine, version du generateur, coordonnees).
//! - Seuls les chunks modifies sont sauvegardes ; les autres se regenerent.
//! - Les chunks generes pour l'affichage sont un simple cache : ils n'entrent pas dans
//!   l'empreinte d'etat (le comportement ne depend pas de la camera).

use crate::content::ResourceId;
use crate::geom::Pos;
use crate::rng::{fractal, hash2, hash_u16, isqrt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CHUNK: i32 = 32;
pub const CHUNK_AREA: usize = (CHUNK * CHUNK) as usize;
/// Version du generateur, figee dans chaque partie : un monde existant ne change jamais.
/// Version du generateur des nouvelles parties.
/// - v1 : deux ressources (minerais A et B) ;
/// - v2 (etape 3) : troisieme ressource provisoire [O] « ressource_c » (combustible), avec un
///   gisement garanti pres du depart. Le reste est identique a v1.
/// Une partie garde la version de son monde : un monde v1 est toujours genere en v1.
pub const GENERATOR_VERSION: u32 = 2;
/// Rayon degage autour du depart (pas d'eau ni d'obstacle).
pub const START_CLEAR_RADIUS: i64 = 26;
pub const NO_RESOURCE: u8 = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Ground {
    Grass = 0,
    Dirt = 1,
    Sand = 2,
    Water = 3,
    /// Eau remblayee par le joueur.
    Fill = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Feature {
    None = 0,
    TreeLeafy = 1,
    TreeConifer = 2,
    Rock = 3,
}

impl Ground {
    pub fn from_u8(v: u8) -> Ground {
        match v {
            1 => Ground::Dirt,
            2 => Ground::Sand,
            3 => Ground::Water,
            4 => Ground::Fill,
            _ => Ground::Grass,
        }
    }
}
impl Feature {
    pub fn from_u8(v: u8) -> Feature {
        match v {
            1 => Feature::TreeLeafy,
            2 => Feature::TreeConifer,
            3 => Feature::Rock,
            _ => Feature::None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub ground: Vec<u8>,
    pub feature: Vec<u8>,
    pub res_kind: Vec<u8>,
    pub res_amount: Vec<u32>,
    /// Vrai des qu'un joueur ou une machine a change le chunk : il sera sauvegarde.
    pub modified: bool,
}

impl Chunk {
    #[inline]
    pub fn idx(local_x: i32, local_y: i32) -> usize {
        (local_y * CHUNK + local_x) as usize
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorldParams {
    pub seed: u64,
    pub generator_version: u32,
    /// Mode ressources infinies (choisi a la creation, v0.4 4.4).
    pub infinite_resources: bool,
}

#[derive(Debug, Clone)]
pub struct World {
    pub params: WorldParams,
    chunks: BTreeMap<(i32, i32), Chunk>,
}

pub fn chunk_of(p: Pos) -> ((i32, i32), i32, i32) {
    let cx = p.x.div_euclid(CHUNK);
    let cy = p.y.div_euclid(CHUNK);
    ((cx, cy), p.x.rem_euclid(CHUNK), p.y.rem_euclid(CHUNK))
}

/// Gisement possible pour une cellule de la grille des gisements.
struct Patch {
    cx: i64,
    cy: i64,
    radius: i64,
    kind: ResourceId,
    richness: i64,
}

const PATCH_CELL: i64 = 96;

impl World {
    pub fn new(params: WorldParams) -> World {
        World {
            params,
            chunks: BTreeMap::new(),
        }
    }

    pub fn chunk(&mut self, key: (i32, i32)) -> &Chunk {
        let params = self.params;
        self.chunks
            .entry(key)
            .or_insert_with(|| generate_chunk(&params, key))
    }

    pub fn chunk_mut(&mut self, key: (i32, i32)) -> &mut Chunk {
        let params = self.params;
        self.chunks
            .entry(key)
            .or_insert_with(|| generate_chunk(&params, key))
    }

    pub fn ground(&mut self, p: Pos) -> Ground {
        let (k, lx, ly) = chunk_of(p);
        Ground::from_u8(self.chunk(k).ground[Chunk::idx(lx, ly)])
    }
    pub fn feature(&mut self, p: Pos) -> Feature {
        let (k, lx, ly) = chunk_of(p);
        Feature::from_u8(self.chunk(k).feature[Chunk::idx(lx, ly)])
    }
    pub fn resource(&mut self, p: Pos) -> Option<(ResourceId, u32)> {
        let (k, lx, ly) = chunk_of(p);
        let c = self.chunk(k);
        let i = Chunk::idx(lx, ly);
        if c.res_kind[i] == NO_RESOURCE || c.res_amount[i] == 0 {
            None
        } else {
            Some((c.res_kind[i], c.res_amount[i]))
        }
    }

    /// Retire une unite de ressource. Renvoie le type extrait.
    pub fn mine_one(&mut self, p: Pos) -> Option<ResourceId> {
        let infinite = self.params.infinite_resources;
        let (k, lx, ly) = chunk_of(p);
        let c = self.chunk_mut(k);
        let i = Chunk::idx(lx, ly);
        if c.res_kind[i] == NO_RESOURCE || c.res_amount[i] == 0 {
            return None;
        }
        if !infinite {
            c.res_amount[i] -= 1;
            c.modified = true;
        }
        Some(c.res_kind[i])
    }

    /// Defrichement ou suppression d'un rocher.
    pub fn clear_feature(&mut self, p: Pos) -> bool {
        let (k, lx, ly) = chunk_of(p);
        let c = self.chunk_mut(k);
        let i = Chunk::idx(lx, ly);
        if c.feature[i] == Feature::None as u8 {
            return false;
        }
        c.feature[i] = Feature::None as u8;
        c.modified = true;
        true
    }

    /// Remblayage d'une case d'eau.
    pub fn fill_water(&mut self, p: Pos) -> bool {
        let (k, lx, ly) = chunk_of(p);
        let c = self.chunk_mut(k);
        let i = Chunk::idx(lx, ly);
        if c.ground[i] != Ground::Water as u8 {
            return false;
        }
        c.ground[i] = Ground::Fill as u8;
        c.modified = true;
        true
    }

    /// Case constructible : pas d'eau, pas d'obstacle.
    pub fn buildable(&mut self, p: Pos) -> bool {
        p.in_world() && self.ground(p) != Ground::Water && self.feature(p) == Feature::None
    }

    pub fn modified_chunks(&self) -> impl Iterator<Item = (&(i32, i32), &Chunk)> {
        self.chunks.iter().filter(|(_, c)| c.modified)
    }

    pub fn insert_chunk(&mut self, key: (i32, i32), c: Chunk) {
        self.chunks.insert(key, c);
    }

    /// Libere les chunks non modifies (regenerables), pour la protection memoire (decision 08).
    pub fn drop_unmodified(&mut self) -> usize {
        let before = self.chunks.len();
        self.chunks.retain(|_, c| c.modified);
        before - self.chunks.len()
    }

    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }
}

fn dist2(x: i64, y: i64) -> i64 {
    x * x + y * y
}

/// Gisement de la cellule (gx, gy) de la grille des gisements, s'il existe.
fn patch_for_cell(params: &WorldParams, gx: i64, gy: i64, n_res: u8) -> Option<Patch> {
    let seed = params.seed;
    // Gisements garantis pres du depart (v0.4 : zone initiale modeste mais suffisante).
    if gx == 0 && gy == 0 {
        return None;
    }
    let h = hash2(seed, 7, gx, gy);
    if h % 100 >= 55 {
        return None;
    }
    let ox = (hash_u16(seed, 8, gx, gy) * (PATCH_CELL - 40) >> 16) + 20;
    let oy = (hash_u16(seed, 9, gx, gy) * (PATCH_CELL - 40) >> 16) + 20;
    let cx = gx * PATCH_CELL + ox;
    let cy = gy * PATCH_CELL + oy;
    // Taille et richesse croissent avec la distance puis atteignent un plateau (decision 11).
    let d = isqrt(dist2(cx, cy));
    let growth = d.min(4000); // plateau a 4000 cases (valeur provisoire [O])
    let radius = 4 + growth / 400 + (hash_u16(seed, 10, gx, gy) % 4);
    let richness = 600 + growth * 3;
    let kind = (hash2(seed, 11, gx, gy) % n_res as u64) as ResourceId;
    Some(Patch {
        cx,
        cy,
        radius,
        kind,
        richness,
    })
}

/// Nombre de ressources du generateur.
fn resource_count(params: &WorldParams) -> u8 {
    if params.generator_version >= 2 {
        3
    } else {
        2
    }
}

fn start_patches(params: &WorldParams) -> Vec<Patch> {
    let mut v = vec![
        Patch {
            cx: -10,
            cy: -2,
            radius: 4,
            kind: 0,
            richness: 900,
        },
        Patch {
            cx: -10,
            cy: 6,
            radius: 4,
            kind: 1,
            richness: 900,
        },
    ];
    if params.generator_version >= 2 {
        // combustible, au sud de la zone de depart, loin de la scene de demonstration
        v.push(Patch {
            cx: -10,
            cy: 16,
            radius: 3,
            kind: 2,
            richness: 900,
        });
    }
    v
}

pub fn generate_chunk(params: &WorldParams, key: (i32, i32)) -> Chunk {
    let seed = params.seed;
    let mut ground = vec![Ground::Grass as u8; CHUNK_AREA];
    let mut feature = vec![Feature::None as u8; CHUNK_AREA];
    let mut res_kind = vec![NO_RESOURCE; CHUNK_AREA];
    let mut res_amount = vec![0u32; CHUNK_AREA];
    let x0 = key.0 as i64 * CHUNK as i64;
    let y0 = key.1 as i64 * CHUNK as i64;
    let n_res = resource_count(params);

    // Gisements candidats des cellules voisines.
    let mut patches: Vec<Patch> = Vec::new();
    let gx0 = (x0 - 64).div_euclid(PATCH_CELL);
    let gx1 = (x0 + CHUNK as i64 + 64).div_euclid(PATCH_CELL);
    let gy0 = (y0 - 64).div_euclid(PATCH_CELL);
    let gy1 = (y0 + CHUNK as i64 + 64).div_euclid(PATCH_CELL);
    for gy in gy0..=gy1 {
        for gx in gx0..=gx1 {
            if let Some(p) = patch_for_cell(params, gx, gy, n_res) {
                patches.push(p);
            }
        }
    }
    patches.extend(start_patches(params));

    for ly in 0..CHUNK {
        for lx in 0..CHUNK {
            let x = x0 + lx as i64;
            let y = y0 + ly as i64;
            let i = Chunk::idx(lx, ly);
            let d2 = dist2(x, y);
            let in_start = d2 <= START_CLEAR_RADIUS * START_CLEAR_RADIUS;

            let wet = fractal(seed, 1, x, y, 128, 4);
            let soil = fractal(seed, 2, x, y, 64, 3);
            let forest = fractal(seed, 3, x, y, 64, 3);

            let mut g = if soil > 41000 {
                Ground::Dirt
            } else {
                Ground::Grass
            };
            if !in_start {
                if wet < 15500 {
                    g = Ground::Water;
                } else if wet < 18500 {
                    g = Ground::Sand;
                }
            }
            ground[i] = g as u8;

            if g != Ground::Water && !in_start {
                let r = hash_u16(seed, 4, x, y);
                if forest > 40000 && r < 26000 {
                    feature[i] = if r & 1 == 0 {
                        Feature::TreeLeafy as u8
                    } else {
                        Feature::TreeConifer as u8
                    };
                } else if forest > 34000 && r < 5000 {
                    feature[i] = Feature::TreeLeafy as u8;
                } else if r > 65200 {
                    feature[i] = Feature::Rock as u8;
                }
            }

            if g != Ground::Water {
                for p in &patches {
                    let dd = dist2(x - p.cx, y - p.cy);
                    // bord irregulier
                    let edge = p.radius * p.radius + (hash_u16(seed, 12, x, y) % 9) - 4;
                    if dd <= edge {
                        let falloff =
                            (p.radius * p.radius - dd).max(0) * 100 / (p.radius * p.radius).max(1);
                        res_kind[i] = p.kind;
                        res_amount[i] = (p.richness * (40 + falloff) / 100).max(50) as u32;
                        if feature[i] == Feature::TreeLeafy as u8
                            || feature[i] == Feature::TreeConifer as u8
                        {
                            feature[i] = Feature::None as u8;
                        }
                        break;
                    }
                }
            }
        }
    }
    Chunk {
        ground,
        feature,
        res_kind,
        res_amount,
        modified: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(seed: u64) -> WorldParams {
        params_v(seed, GENERATOR_VERSION)
    }

    fn params_v(seed: u64, generator_version: u32) -> WorldParams {
        WorldParams {
            seed,
            generator_version,
            infinite_resources: false,
        }
    }

    fn fingerprint(version: u32) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        // v2 : echantillon elargi a des chunks qui contiennent des gisements (celui de la v1
        // n'en contenait aucun et ne voyait pas le changement de ressources)
        let keys: &[(i32, i32)] = if version == 1 {
            &[(0, 0), (1, -1), (-4, 3), (100, 200)]
        } else {
            &[
                (0, 0),
                (1, -1),
                (-4, 3),
                (100, 200),
                (-1, 0),
                (3, 2),
                (-5, -6),
                (7, -3),
                (12, 9),
            ]
        };
        for &key in keys {
            let c = generate_chunk(&params_v(12345, version), key);
            for v in c
                .ground
                .iter()
                .chain(c.feature.iter())
                .chain(c.res_kind.iter())
            {
                h ^= *v as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            for v in &c.res_amount {
                h ^= *v as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        }
        h
    }

    #[test]
    fn generation_is_deterministic() {
        for key in [(0, 0), (-3, 7), (31250, -31250), (-31250, 31249)] {
            let a = generate_chunk(&params(99), key);
            let b = generate_chunk(&params(99), key);
            assert_eq!(a.ground, b.ground);
            assert_eq!(a.feature, b.feature);
            assert_eq!(a.res_amount, b.res_amount);
        }
        let a = generate_chunk(&params(1), (5, 5));
        let b = generate_chunk(&params(2), (5, 5));
        assert!(a.ground != b.ground || a.feature != b.feature);
    }

    /// Empreinte de reference : si elle change, le generateur a change et
    /// GENERATOR_VERSION doit etre incremente (un monde existant ne doit jamais changer).
    /// Empreintes de reference : un monde existant ne doit jamais changer. La v1 doit rester
    /// celle des premieres parties ; une modification du generateur cree une nouvelle version.
    #[test]
    fn generator_reference_fingerprint() {
        assert_eq!(
            fingerprint(1),
            GENERATOR_V1_FINGERPRINT,
            "les mondes v1 ont change"
        );
        assert_eq!(
            fingerprint(2),
            GENERATOR_V2_FINGERPRINT,
            "le generateur a change : incrementer GENERATOR_VERSION"
        );
        assert_eq!(GENERATOR_VERSION, 2);
    }

    #[test]
    fn version2_adds_fuel_near_start() {
        let mut w = World::new(params_v(7, 2));
        let mut found = false;
        for y in 12..=20 {
            for x in -14..=-6 {
                if let Some((k, _)) = w.resource(Pos::new(x, y)) {
                    found |= k == 2;
                }
            }
        }
        assert!(found, "gisement de combustible garanti");
        let mut w1 = World::new(params_v(7, 1));
        for y in 12..=20 {
            for x in -14..=-6 {
                assert_ne!(w1.resource(Pos::new(x, y)).map(|r| r.0), Some(2));
            }
        }
    }

    #[test]
    fn start_area_is_clear_with_two_resources() {
        let mut w = World::new(params(7));
        let mut kinds = [false; 3];
        for y in -26..=26 {
            for x in -26..=26 {
                if x * x + y * y > 26 * 26 {
                    continue;
                }
                let p = Pos::new(x, y);
                assert_ne!(w.ground(p), Ground::Water);
                assert_eq!(w.feature(p), Feature::None);
                if let Some((k, _)) = w.resource(p) {
                    kinds[k as usize] = true;
                }
            }
        }
        assert!(kinds[0] && kinds[1]);
    }

    #[test]
    fn far_chunks_work() {
        let mut w = World::new(params(3));
        let p = Pos::new(999_990, -999_990);
        let _ = w.ground(p);
        assert!(p.in_world());
        assert!(!Pos::new(1_000_001, 0).in_world());
    }
}

#[cfg(test)]
const GENERATOR_V1_FINGERPRINT: u64 = 4726573389097261936;
#[cfg(test)]
const GENERATOR_V2_FINGERPRINT: u64 = 1607450470287719893;
