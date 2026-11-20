//! Coordonnees entieres et directions.

use serde::{Deserialize, Serialize};

/// Limite technique du monde : +/- 1 000 000 cases par axe (v0.4, decision 05).
pub const WORLD_LIMIT: i32 = 1_000_000;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32) -> Pos {
        Pos { x, y }
    }
    pub fn step(self, d: Dir) -> Pos {
        let (dx, dy) = d.delta();
        Pos::new(self.x + dx, self.y + dy)
    }
    pub fn offset(self, dx: i32, dy: i32) -> Pos {
        Pos::new(self.x + dx, self.y + dy)
    }
    pub fn in_world(self) -> bool {
        self.x.abs() <= WORLD_LIMIT && self.y.abs() <= WORLD_LIMIT
    }
}

/// Direction cardinale. Nord = y negatif (axe y vers le bas, comme l'ecran).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub enum Dir {
    #[default]
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (0, -1),
            Dir::E => (1, 0),
            Dir::S => (0, 1),
            Dir::W => (-1, 0),
        }
    }
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn from_index(i: i64) -> Dir {
        Dir::ALL[(i.rem_euclid(4)) as usize]
    }
    pub fn opposite(self) -> Dir {
        Dir::from_index(self as i64 + 2)
    }
    pub fn cw(self) -> Dir {
        Dir::from_index(self as i64 + 1)
    }
    pub fn ccw(self) -> Dir {
        Dir::from_index(self as i64 + 3)
    }
    /// Cote gauche quand on regarde dans cette direction.
    pub fn left(self) -> Dir {
        self.ccw()
    }
    pub fn right(self) -> Dir {
        self.cw()
    }
    pub fn is_perpendicular(self, o: Dir) -> bool {
        (self as u8 + o as u8) % 2 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirs() {
        assert_eq!(Dir::N.left(), Dir::W);
        assert_eq!(Dir::E.left(), Dir::N);
        assert_eq!(Dir::S.opposite(), Dir::N);
        assert!(Dir::N.is_perpendicular(Dir::E));
        assert_eq!(Pos::new(0, 0).step(Dir::N), Pos::new(0, -1));
    }
}
