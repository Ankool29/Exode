//! EXODE - noyau de simulation.
//!
//! Independant de tout moteur graphique. Calculs en entiers, ordre de traitement stable,
//! empreinte d'etat pour verifier le determinisme (cahier de conception v0.4, sections 8 et 9).

pub mod content;
pub mod entity;
pub mod geom;
pub mod rng;
pub mod scenario;
pub mod sim;
pub mod world;

pub use content::Content;
pub use geom::{Dir, Pos};
pub use sim::{Command, Simulation, TICKS_PER_SECOND};

#[cfg(test)]
mod tests;
