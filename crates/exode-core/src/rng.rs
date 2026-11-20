//! Hasard et bruit deterministes, en entiers uniquement.
//!
//! La generation du monde fait partie du noyau et doit donner le meme resultat sur tous
//! les appareils (v0.4, decision 21) : aucun flottant, aucune bibliotheque de bruit externe.

/// Melange entier de bonne qualite (finaliseur de type splitmix64).
#[inline]
pub fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Hachage de (graine, canal, x, y) vers 64 bits.
#[inline]
pub fn hash2(seed: u64, channel: u64, x: i64, y: i64) -> u64 {
    let mut h = mix64(seed ^ channel.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    h = mix64(h ^ (x as u64).wrapping_mul(0xd6e8_feb8_6659_fd93));
    mix64(h ^ (y as u64).wrapping_mul(0xa076_1d64_78bd_642f))
}

/// Valeur dans [0, 65535].
#[inline]
pub fn hash_u16(seed: u64, channel: u64, x: i64, y: i64) -> i64 {
    (hash2(seed, channel, x, y) >> 48) as i64
}

/// Bruit de valeur lisse, resultat dans [0, 65535], interpolation entiere.
/// `cell` = taille d'une cellule de bruit en cases (puissance de 2 conseillee).
pub fn value_noise(seed: u64, channel: u64, x: i64, y: i64, cell: i64) -> i64 {
    let cx = x.div_euclid(cell);
    let cy = y.div_euclid(cell);
    let fx = x.rem_euclid(cell) * 1024 / cell; // 0..1023
    let fy = y.rem_euclid(cell) * 1024 / cell;
    // lissage (3t^2 - 2t^3) en virgule fixe 10 bits
    let sx = smooth(fx);
    let sy = smooth(fy);
    let v00 = hash_u16(seed, channel, cx, cy);
    let v10 = hash_u16(seed, channel, cx + 1, cy);
    let v01 = hash_u16(seed, channel, cx, cy + 1);
    let v11 = hash_u16(seed, channel, cx + 1, cy + 1);
    let a = v00 + ((v10 - v00) * sx >> 10);
    let b = v01 + ((v11 - v01) * sx >> 10);
    a + ((b - a) * sy >> 10)
}

#[inline]
fn smooth(t: i64) -> i64 {
    // t dans [0, 1024) -> [0, 1024)
    (t * t * (3 * 1024 - 2 * t)) >> 20
}

/// Bruit fractal a plusieurs octaves, resultat dans [0, 65535].
pub fn fractal(seed: u64, channel: u64, x: i64, y: i64, base_cell: i64, octaves: u32) -> i64 {
    let mut total = 0i64;
    let mut weight_sum = 0i64;
    let mut cell = base_cell;
    let mut weight = 1024i64;
    for o in 0..octaves {
        total += value_noise(seed, channel + o as u64 * 101, x, y, cell.max(1)) * weight;
        weight_sum += weight;
        cell /= 2;
        weight /= 2;
    }
    total / weight_sum
}

/// Racine carree entiere.
pub fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = (n as f64).sqrt() as i64; // estimation, corrigee ci-dessous en entiers
    while x * x > n {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= n {
        x += 1;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noise_is_deterministic_and_bounded() {
        for i in -50..50 {
            let v = fractal(42, 1, i * 7, i * -3, 64, 4);
            assert!((0..=65535).contains(&v));
            assert_eq!(v, fractal(42, 1, i * 7, i * -3, 64, 4));
        }
        assert_ne!(hash2(1, 0, 0, 0), hash2(2, 0, 0, 0));
    }
    #[test]
    fn isqrt_exact() {
        for n in 0..10_000 {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n);
        }
    }
}
