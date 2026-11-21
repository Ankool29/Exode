//! Etat des entites. Tout est en entiers et serialisable.

use crate::content::{FluidId, ItemId, ProtoId, RecipeId};
use crate::geom::{Dir, Pos};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type EntityId = u32;

/// Longueur d'une voie de convoyeur sur une case, en unites internes.
pub const LANE_LEN: u32 = 256;
/// Ecart minimal entre deux objets d'une meme voie : 4 objets par voie et par case.
/// Densite provisoire (GRAPHICS-01, decision 4 [O]).
pub const SPACING: u32 = 64;
/// Point d'insertion au milieu d'une voie (bras, foreuses, insertion laterale).
pub const MID: u32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeltItem {
    pub item: ItemId,
    /// 0 = entree de la case, LANE_LEN = sortie.
    pub pos: u32,
}

/// Comment un convoyeur deverse dans le suivant (cache recalcule a chaque changement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Feed {
    /// Pas de convoyeur accessible en sortie.
    #[default]
    None,
    /// Meme voie, entree au debut (tout droit ou virage).
    Same(EntityId),
    /// Insertion laterale sur la voie indiquee, au milieu.
    Side(EntityId, u8),
}

/// Convoyeur. Les objets ne sont pas ici : ils sont regroupes par lignes de transport
/// (`belts.rs`) et lus case par case avec `Simulation::belt_lanes`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Belt {
    #[serde(skip)]
    pub feed: Feed,
    /// Cote d'entree si ce convoyeur est un virage.
    #[serde(skip)]
    pub curve_from: Option<Dir>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArmState {
    #[default]
    Idle,
    ToDrop,
    Dropping,
    Returning,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inserter {
    pub held: Option<ItemId>,
    pub state: ArmState,
    /// Avancement du mouvement en milli-ticks (augmente de la satisfaction electrique).
    pub timer: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Machine {
    pub recipe: Option<RecipeId>,
    pub inputs: BTreeMap<ItemId, u32>,
    pub outputs: BTreeMap<ItemId, u32>,
    /// Recette en cours : entrees deja consommees.
    pub running: bool,
    /// Avancement en milli-ticks.
    pub progress: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Drill {
    pub progress: u64,
    pub buffer: Option<ItemId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Storage {
    pub items: BTreeMap<ItemId, u32>,
}

impl Storage {
    pub fn total(&self) -> u32 {
        self.items.values().sum()
    }
    pub fn add(&mut self, item: ItemId, n: u32) {
        *self.items.entry(item).or_insert(0) += n;
    }
    pub fn take(&mut self, item: ItemId) -> bool {
        match self.items.get_mut(&item) {
            Some(c) if *c > 0 => {
                *c -= 1;
                if *c == 0 {
                    self.items.remove(&item);
                }
                true
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Data {
    Belt(Belt),
    Inserter(Inserter),
    Drill(Drill),
    Machine(Machine),
    Storage(Storage),
    Pole,
    Generator,
    // --- reseaux de fluides (etape 3), variantes ajoutees a la fin (sauvegardes) ---
    /// Conduite ou conduite souterraine : le fluide est tenu par le segment (`fluids.rs`).
    Pipe,
    Pump,
    Valve {
        open: bool,
    },
    Tank(FluidBox),
    WaterPump,
}

/// Contenu d'un volume de fluide (segment, reservoir, tampon de machine).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FluidBox {
    pub fluid: Option<FluidId>,
    pub amount: u64,
    /// Degres Celsius.
    pub temp: i32,
}

impl FluidBox {
    /// Ajoute `amount` unites a la temperature `temp` (melange pondere, en entiers).
    pub fn add(&mut self, fluid: FluidId, amount: u64, temp: i32) {
        if amount == 0 {
            return;
        }
        let total = self.amount + amount;
        self.temp = ((self.amount as i64 * self.temp as i64 + amount as i64 * temp as i64)
            / total as i64) as i32;
        self.amount = total;
        self.fluid = Some(fluid);
    }

    /// Retire `amount` unites (au plus ce qu'il y a).
    pub fn remove(&mut self, amount: u64) -> u64 {
        let m = amount.min(self.amount);
        self.amount -= m;
        if self.amount == 0 {
            self.fluid = None;
            self.temp = 0;
        }
        m
    }

    /// Le fluide peut-il entrer ici ?
    pub fn accepts(&self, fluid: FluidId) -> bool {
        self.fluid.map(|f| f == fluid).unwrap_or(true)
    }
}

/// Etat affiche au-dessus des entites (v0.4 : explication des attentes et blocages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum Status {
    #[default]
    Idle = 0,
    Working = 1,
    NoInput = 2,
    OutputFull = 3,
    WasteFull = 4,
    NoPower = 5,
    LowPower = 6,
    NoRecipe = 7,
    NoResource = 8,
    NoTarget = 9,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub proto: ProtoId,
    /// Coin haut-gauche de l'emprise.
    pub pos: Pos,
    pub dir: Dir,
    pub w: i32,
    pub h: i32,
    pub data: Data,
    pub status: Status,
    /// Demande d'energie lors du dernier tick (sert au calcul du tick suivant).
    pub wants_power: bool,
}

impl Entity {
    pub fn tiles(&self) -> impl Iterator<Item = Pos> + '_ {
        (0..self.h).flat_map(move |dy| (0..self.w).map(move |dx| self.pos.offset(dx, dy)))
    }
    pub fn contains(&self, p: Pos) -> bool {
        p.x >= self.pos.x
            && p.y >= self.pos.y
            && p.x < self.pos.x + self.w
            && p.y < self.pos.y + self.h
    }
    /// Case derriere l'entite, symetrique de `front_tile`.
    pub fn back_tile(&self) -> Pos {
        let (x, y, w, h) = (self.pos.x, self.pos.y, self.w, self.h);
        match self.dir {
            Dir::N => Pos::new(x, y + h),
            Dir::E => Pos::new(x - 1, y),
            Dir::S => Pos::new(x + w - 1, y - 1),
            Dir::W => Pos::new(x + w, y + h - 1),
        }
    }
    /// Case de sortie devant l'entite : case avant-gauche quand on regarde vers `dir`.
    pub fn front_tile(&self) -> Pos {
        let (x, y, w, h) = (self.pos.x, self.pos.y, self.w, self.h);
        match self.dir {
            Dir::N => Pos::new(x, y - 1),
            Dir::E => Pos::new(x + w, y),
            Dir::S => Pos::new(x + w - 1, y + h),
            Dir::W => Pos::new(x - 1, y + h - 1),
        }
    }
}
