//! Contenu de jeu charge depuis des donnees (objets, recettes, ressources, batiments)
//! et verification de la conservation de la matiere (cahier v0.4, decision 20).

use serde::Deserialize;
use std::collections::BTreeMap;

pub type ItemId = u16;
pub type RecipeId = u16;
pub type ProtoId = u16;
pub type ResourceId = u8;
pub type FluidId = u8;

/// Contenu fourni avec le noyau. Provisoire : les listes definitives restent ouvertes [O].
pub const DEFAULT_CONTENT: &str = include_str!("../data/content.json");

#[derive(Debug, Clone)]
pub struct Item {
    pub key: String,
    pub name: String,
    /// Masse en grammes.
    pub mass: u64,
    pub waste: bool,
    pub color: String,
    /// Energie liberee en brulant (W x tick ; 0 : pas un combustible).
    pub fuel: u64,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub key: String,
    pub category: String,
    /// Duree en ticks a vitesse 1.
    pub time: u32,
    pub inputs: Vec<(ItemId, u32)>,
    pub outputs: Vec<(ItemId, u32)>,
}

/// Fluide (etape 3). Les quantites sont en unites entieres.
#[derive(Debug, Clone)]
pub struct Fluid {
    pub key: String,
    pub name: String,
    /// Masse d'une unite, en grammes (bilan matiere).
    pub mass: u64,
    /// Temperature a la production, en degres Celsius.
    pub temperature: i32,
    pub color: String,
    /// Energie utilisable portee par une unite (W x tick), pour la vapeur.
    pub energy: u64,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub key: String,
    pub item: ItemId,
    pub color: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtoKind {
    Belt,
    Inserter,
    Drill,
    Machine,
    Chest,
    WasteBin,
    Pole,
    Generator,
    /// Conduite (relie ses quatre voisines).
    Pipe,
    /// Conduite souterraine : un raccord en surface (cote `dir`), un tunnel vers l'arriere.
    PipeUnderground,
    /// Pompe : impose le sens, debit plafonne, consomme de l'electricite.
    Pump,
    /// Vanne : ouverte, laisse passer dans son sens ; fermee, bloque.
    Valve,
    /// Reservoir : volume propre, quatre raccords.
    Tank,
    /// Pompe a eau : au bord d'un lac, produit de l'eau.
    WaterPump,
    /// Chaudiere : brule du combustible, produit de la chaleur.
    Boiler,
    /// Caloduc : transporte la chaleur (relie ses quatre voisins).
    HeatPipe,
    /// Echangeur : chaleur + eau -> vapeur.
    Exchanger,
    /// Turbine : vapeur -> electricite ; vapeur evacuee comptee comme perte.
    Turbine,
}

#[derive(Debug, Clone)]
pub struct Proto {
    pub key: String,
    pub name: String,
    pub kind: ProtoKind,
    pub w: i32,
    pub h: i32,
    /// Watts consommes en marche (ou produits pour un generateur).
    pub power: u64,
    /// Sens selon le type : unites par tick (convoyeur), ticks par demi-mouvement (bras),
    /// ticks par extraction (foreuse), vitesse en pour mille (machine).
    pub speed: u32,
    pub category: Option<String>,
    pub auto_recipe: bool,
    pub capacity: u32,
    pub supply_radius: i32,
    pub wire_reach: i32,
    /// Capacite des liaisons electriques de ce noeud (W) : une liaison vaut la plus faible
    /// des capacites de ses deux extremites.
    pub link_capacity: u64,
    /// Volume d'une conduite ou d'un reservoir (unites de fluide).
    pub fluid_capacity: u64,
    /// Debit maximal (unites par tick) d'une pompe, d'une vanne ou d'un raccord de reservoir.
    pub flow_rate: u64,
    /// Portee d'une conduite souterraine (cases).
    pub reach: i32,
    /// Capacite thermique (W x tick par degre).
    pub heat_capacity: u64,
    /// Puissance thermique d'une chaudiere (W).
    pub heat_power: u64,
    /// Temperature maximale (chaudiere) ou minimale de fonctionnement (echangeur), degres.
    pub temperature: i32,
    /// Puissance maximale echangee par un raccord de chaleur (W).
    pub heat_limit: u64,
}

#[derive(Debug, Clone)]
pub struct Content {
    pub items: Vec<Item>,
    pub recipes: Vec<Recipe>,
    pub resources: Vec<Resource>,
    pub fluids: Vec<Fluid>,
    pub protos: Vec<Proto>,
    item_index: BTreeMap<String, ItemId>,
    recipe_index: BTreeMap<String, RecipeId>,
    proto_index: BTreeMap<String, ProtoId>,
}

#[derive(Deserialize)]
struct RawContent {
    items: Vec<RawItem>,
    recipes: Vec<RawRecipe>,
    resources: Vec<RawResource>,
    #[serde(default)]
    fluids: Vec<RawFluid>,
    buildings: Vec<RawProto>,
}

#[derive(Deserialize)]
struct RawFluid {
    id: String,
    name: String,
    mass: u64,
    temperature: i32,
    #[serde(default)]
    color: String,
    #[serde(default)]
    energy: u64,
}

#[derive(Deserialize)]
struct RawItem {
    id: String,
    name: String,
    mass: u64,
    #[serde(default)]
    waste: bool,
    #[serde(default)]
    color: String,
    #[serde(default)]
    fuel: u64,
}

#[derive(Deserialize)]
struct RawRecipe {
    id: String,
    category: String,
    time: u32,
    inputs: Vec<(String, u32)>,
    outputs: Vec<(String, u32)>,
}

#[derive(Deserialize)]
struct RawResource {
    id: String,
    item: String,
    #[serde(default)]
    color: String,
}

#[derive(Deserialize)]
struct RawProto {
    id: String,
    name: String,
    kind: String,
    size: (i32, i32),
    #[serde(default)]
    power: u64,
    #[serde(default)]
    speed: u32,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    auto_recipe: bool,
    #[serde(default)]
    capacity: u32,
    #[serde(default)]
    supply_radius: i32,
    #[serde(default)]
    wire_reach: i32,
    #[serde(default)]
    link_capacity: u64,
    #[serde(default)]
    fluid_capacity: u64,
    #[serde(default)]
    flow_rate: u64,
    #[serde(default)]
    reach: i32,
    #[serde(default)]
    heat_capacity: u64,
    #[serde(default)]
    heat_power: u64,
    #[serde(default)]
    temperature: i32,
    #[serde(default)]
    heat_limit: u64,
}

impl Content {
    pub fn load_default() -> Content {
        Content::from_json(DEFAULT_CONTENT).expect("contenu par defaut invalide")
    }

    pub fn from_json(text: &str) -> Result<Content, String> {
        let raw: RawContent = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut item_index = BTreeMap::new();
        let mut items = Vec::new();
        for (i, it) in raw.items.into_iter().enumerate() {
            if item_index.insert(it.id.clone(), i as ItemId).is_some() {
                return Err(format!("objet en double : {}", it.id));
            }
            items.push(Item {
                key: it.id,
                name: it.name,
                mass: it.mass,
                waste: it.waste,
                color: it.color,
                fuel: it.fuel,
            });
        }
        let lookup = |k: &str| -> Result<ItemId, String> {
            item_index
                .get(k)
                .copied()
                .ok_or_else(|| format!("objet inconnu : {k}"))
        };
        let mut recipe_index = BTreeMap::new();
        let mut recipes = Vec::new();
        for (i, r) in raw.recipes.into_iter().enumerate() {
            let inputs = r
                .inputs
                .iter()
                .map(|(k, n)| Ok((lookup(k)?, *n)))
                .collect::<Result<Vec<_>, String>>()?;
            let outputs = r
                .outputs
                .iter()
                .map(|(k, n)| Ok((lookup(k)?, *n)))
                .collect::<Result<Vec<_>, String>>()?;
            if r.time == 0 {
                return Err(format!("recette {} : duree nulle", r.id));
            }
            if recipe_index.insert(r.id.clone(), i as RecipeId).is_some() {
                return Err(format!("recette en double : {}", r.id));
            }
            recipes.push(Recipe {
                key: r.id,
                category: r.category,
                time: r.time,
                inputs,
                outputs,
            });
        }
        let mut resources = Vec::new();
        for r in raw.resources {
            resources.push(Resource {
                item: lookup(&r.item)?,
                key: r.id,
                color: r.color,
            });
        }
        let mut fluids = Vec::new();
        for f in raw.fluids {
            if fluids.iter().any(|x: &Fluid| x.key == f.id) {
                return Err(format!("fluide en double : {}", f.id));
            }
            fluids.push(Fluid {
                key: f.id,
                name: f.name,
                mass: f.mass,
                temperature: f.temperature,
                color: f.color,
                energy: f.energy,
            });
        }
        let mut proto_index = BTreeMap::new();
        let mut protos = Vec::new();
        for (i, p) in raw.buildings.into_iter().enumerate() {
            let kind = match p.kind.as_str() {
                "belt" => ProtoKind::Belt,
                "inserter" => ProtoKind::Inserter,
                "drill" => ProtoKind::Drill,
                "machine" => ProtoKind::Machine,
                "chest" => ProtoKind::Chest,
                "waste_bin" => ProtoKind::WasteBin,
                "pole" => ProtoKind::Pole,
                "generator" => ProtoKind::Generator,
                "pipe" => ProtoKind::Pipe,
                "pipe_underground" => ProtoKind::PipeUnderground,
                "pump" => ProtoKind::Pump,
                "valve" => ProtoKind::Valve,
                "tank" => ProtoKind::Tank,
                "water_pump" => ProtoKind::WaterPump,
                "boiler" => ProtoKind::Boiler,
                "heat_pipe" => ProtoKind::HeatPipe,
                "exchanger" => ProtoKind::Exchanger,
                "turbine" => ProtoKind::Turbine,
                other => return Err(format!("type de batiment inconnu : {other}")),
            };
            if proto_index.insert(p.id.clone(), i as ProtoId).is_some() {
                return Err(format!("batiment en double : {}", p.id));
            }
            protos.push(Proto {
                key: p.id,
                name: p.name,
                kind,
                w: p.size.0,
                h: p.size.1,
                power: p.power,
                speed: p.speed,
                category: p.category,
                auto_recipe: p.auto_recipe,
                capacity: p.capacity,
                supply_radius: p.supply_radius,
                wire_reach: p.wire_reach,
                link_capacity: p.link_capacity,
                fluid_capacity: p.fluid_capacity,
                flow_rate: p.flow_rate,
                reach: p.reach,
                heat_capacity: p.heat_capacity,
                heat_power: p.heat_power,
                temperature: p.temperature,
                heat_limit: p.heat_limit,
            });
        }
        let content = Content {
            items,
            recipes,
            resources,
            fluids,
            protos,
            item_index,
            recipe_index,
            proto_index,
        };
        let errors = content.validate();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(content)
    }

    pub fn item(&self, key: &str) -> Option<ItemId> {
        self.item_index.get(key).copied()
    }
    pub fn recipe(&self, key: &str) -> Option<RecipeId> {
        self.recipe_index.get(key).copied()
    }
    pub fn proto(&self, key: &str) -> Option<ProtoId> {
        self.proto_index.get(key).copied()
    }

    pub fn fluid(&self, key: &str) -> Option<FluidId> {
        self.fluids
            .iter()
            .position(|f| f.key == key)
            .map(|i| i as FluidId)
    }

    /// Masse totale (grammes) d'une liste d'objets.
    pub fn stack_mass(&self, list: &[(ItemId, u32)]) -> u64 {
        list.iter()
            .map(|(i, n)| self.items[*i as usize].mass * *n as u64)
            .sum()
    }

    /// Masse perdue (grammes) par cycle d'une recette : entrees - sorties.
    pub fn recipe_loss(&self, r: RecipeId) -> u64 {
        let rec = &self.recipes[r as usize];
        self.stack_mass(&rec.inputs) - self.stack_mass(&rec.outputs)
    }

    /// Controles de conservation de la matiere (v0.4, decision 20) et coherence des donnees.
    ///
    /// Regle : aucune recette ne produit plus de masse qu'elle n'en consomme. Seule
    /// l'extraction fait entrer de la matiere. Une recette conforme ne peut donc pas former,
    /// seule ou en boucle, un cycle qui cree de la matiere.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if it.mass == 0 {
                errors.push(format!("objet {} : masse nulle", it.key));
            }
            let _ = i;
        }
        for r in &self.recipes {
            if r.inputs.is_empty() {
                errors.push(format!(
                    "recette {} : aucune entree (seule l'extraction cree de la matiere)",
                    r.key
                ));
                continue;
            }
            let min = self.stack_mass(&r.inputs);
            let mout = self.stack_mass(&r.outputs);
            if mout > min {
                errors.push(format!(
                    "recette {} : cree de la matiere ({} g en entree, {} g en sortie)",
                    r.key, min, mout
                ));
            }
        }
        for p in &self.protos {
            if p.w <= 0 || p.h <= 0 {
                errors.push(format!("batiment {} : taille invalide", p.key));
            }
            match p.kind {
                ProtoKind::Machine => {
                    if p.category.is_none() {
                        errors.push(format!("machine {} : categorie manquante", p.key));
                    }
                }
                ProtoKind::Pipe | ProtoKind::PipeUnderground | ProtoKind::Tank => {
                    if p.fluid_capacity == 0 {
                        errors.push(format!("{} : volume nul", p.key));
                    }
                    if p.kind == ProtoKind::PipeUnderground && p.reach <= 1 {
                        errors.push(format!("{} : portee trop courte", p.key));
                    }
                }
                ProtoKind::Pump | ProtoKind::Valve | ProtoKind::WaterPump => {
                    if p.flow_rate == 0 {
                        errors.push(format!("{} : debit nul", p.key));
                    }
                }
                ProtoKind::Boiler | ProtoKind::HeatPipe | ProtoKind::Exchanger => {
                    if p.heat_capacity == 0 {
                        errors.push(format!("{} : capacite thermique nulle", p.key));
                    }
                }
                ProtoKind::Pole => {
                    if p.wire_reach <= 0 || p.link_capacity == 0 {
                        errors.push(format!(
                            "noeud electrique {} : portee ou capacite de liaison nulle",
                            p.key
                        ));
                    }
                }
                ProtoKind::Belt | ProtoKind::Inserter | ProtoKind::Drill => {
                    if p.speed == 0 {
                        errors.push(format!("batiment {} : vitesse nulle", p.key));
                    }
                    // les lignes de transport supposent qu'un objet avance de moins d'un ecart
                    // minimal par tick (belts.rs)
                    if p.kind == ProtoKind::Belt && p.speed > crate::entity::SPACING {
                        errors.push(format!(
                            "convoyeur {} : vitesse {} superieure a l'ecart minimal {}",
                            p.key,
                            p.speed,
                            crate::entity::SPACING
                        ));
                    }
                }
                _ => {}
            }
        }
        errors
    }

    /// Recette de la categorie dont l'unique entree est cet objet (choix automatique des fours).
    pub fn auto_recipe_for(&self, category: &str, item: ItemId) -> Option<RecipeId> {
        self.recipes
            .iter()
            .position(|r| r.category == category && r.inputs.len() == 1 && r.inputs[0].0 == item)
            .map(|i| i as RecipeId)
    }

    pub fn resource_for_item(&self, item: ItemId) -> Option<ResourceId> {
        self.resources
            .iter()
            .position(|r| r.item == item)
            .map(|i| i as ResourceId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_content_is_valid() {
        let c = Content::load_default();
        assert!(c.validate().is_empty());
        assert!(c.item("plaque_a").is_some());
    }

    #[test]
    fn matter_creating_recipe_is_rejected() {
        let bad = DEFAULT_CONTENT.replace(
            "[\"plaque_a\", 4], [\"scories\", 1]",
            "[\"plaque_a\", 5], [\"scories\", 1]",
        );
        let err = Content::from_json(&bad).unwrap_err();
        assert!(err.contains("cree de la matiere"), "{err}");
    }

    #[test]
    fn recycling_loop_cannot_create_matter() {
        // plaque -> engrenage + chutes -> plaques : la boucle doit perdre ou conserver la masse.
        let c = Content::load_default();
        for r in 0..c.recipes.len() {
            let _ = c.recipe_loss(r as RecipeId); // ne doit pas deborder (sortie <= entree)
        }
    }
}
