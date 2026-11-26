//! Aléatoire déterministe : hachage sans état (graine, identifiant, tick),
//! indépendant de l'ordre et des threads (A-12).

/// Mélange de splitmix64.
#[inline]
pub fn mel(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Hachage de trois entiers.
#[inline]
pub fn hache(a: u64, b: u64, c: u64) -> u64 {
    mel(mel(mel(a.wrapping_add(0x9E37_79B9_7F4A_7C15)) ^ b.wrapping_mul(0xD6E8_FEB8_6659_FD93))
        ^ c.wrapping_mul(0xA076_1D64_78BD_642F))
}

/// Uniforme dans [0, 1[ depuis un hachage.
#[inline]
pub fn uni(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}
