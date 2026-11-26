//! Relief d'une tile : grille de nœuds en centimètres (B-01, P-07, P-26).
//!
//! - **Base** de 2 m (501 × 501 nœuds) : altitudes quantifiées (pas `q` cm),
//!   prédiction MED (comme JPEG-LS), résidus zigzag sur deux plans d'octets
//!   (bas, haut) et échappements sur 32 bits ; un masque marque les nœuds sans
//!   donnée (mer, étranger).
//! - **Complément** de 1 m : les nœuds impairs, prédits par la moyenne des
//!   voisins de la base, résidus zigzag sur 16 bits ; 0xFFFF : sans donnée.
//!
//! Analogie : la base est l'image en basse définition, le complément le
//! « delta » qu'on ne télécharge que de près.
//!
//! L'encodeur sert à la chaîne (données de confiance) ; le décodeur lit des
//! octets quelconques et ne panique jamais (arithmétique enveloppante).

use crate::codec::{Invalid, Result};

/// Nœud sans donnée.
pub const NONE: i32 = i32::MIN;
/// Nœuds par côté au plus (1 m : 1 001).
pub const MAX_SIDE: usize = 1001;

/// Grille de nœuds d'une tile au pas `step` (m) : `side²` valeurs en cm, ou
/// [`NONE`] ; ligne 0 au sud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nodes {
    /// Pas, en mètres.
    pub step: usize,
    /// Nœuds par côté (1000 / pas + 1).
    pub side: usize,
    /// Altitudes en cm, ligne par ligne depuis le sud.
    pub cm: Vec<i32>,
}

impl Nodes {
    /// Grille vide au pas `step`.
    pub fn new(step: usize) -> Nodes {
        let side = 1000 / step + 1;
        Nodes { step, side, cm: vec![NONE; side * side] }
    }
    /// Altitude du nœud (i, j), ou [`NONE`].
    pub fn get(&self, i: usize, j: usize) -> i32 {
        self.cm.get(j * self.side + i).copied().unwrap_or(NONE)
    }
}

fn med(a: i32, b: i32, c: i32) -> i32 {
    if c >= a.max(b) {
        a.min(b)
    } else if c <= a.min(b) {
        a.max(b)
    } else {
        a.wrapping_add(b).wrapping_sub(c)
    }
}

/// Division arrondie au plus proche (milieux loin de zéro).
fn div_round(v: i32, q: i32) -> i32 {
    if q == 1 {
        return v;
    }
    let (v, q) = (i64::from(v), i64::from(q));
    (if v >= 0 { (v + q / 2) / q } else { -((-v + q / 2) / q) }) as i32
}

fn zigzag(r: i32) -> u32 {
    ((r << 1) ^ (r >> 31)) as u32
}

fn unzigzag(z: u32) -> i32 {
    ((z >> 1) as i32) ^ -((z & 1) as i32)
}

/// Prédiction MED du nœud `k` à partir des nœuds déjà reconstruits.
fn predict(rec: &[i32], m: usize, i: usize, j: usize) -> i32 {
    let k = j * m + i;
    let a = if i > 0 {
        rec[k - 1]
    } else if j > 0 {
        rec[k - m]
    } else {
        0
    };
    let b = if j > 0 { rec[k - m] } else { a };
    let c = if i > 0 && j > 0 { rec[k - m - 1] } else { b };
    med(a, b, c)
}

/// Encode une grille, quantifiée au pas `q` cm.
pub fn encode(g: &Nodes, q: i32) -> Vec<u8> {
    let m = g.side;
    let qv: Vec<i32> = g.cm.iter().map(|&v| if v == NONE { NONE } else { div_round(v, q) }).collect();
    let mut none = vec![0u8; (m * m).div_ceil(8)];
    let mut rec = vec![0i32; m * m];
    let mut low = Vec::with_capacity(m * m);
    let mut high = Vec::with_capacity(m * m);
    let mut esc: Vec<u8> = Vec::new();
    for j in 0..m {
        for i in 0..m {
            let k = j * m + i;
            let pred = predict(&rec, m, i, j);
            let v = if qv[k] == NONE {
                none[k >> 3] |= 1 << (k & 7);
                pred
            } else {
                qv[k]
            };
            rec[k] = v;
            let z = zigzag(v.wrapping_sub(pred));
            if z < 0xFFFF {
                low.push(z as u8);
                high.push((z >> 8) as u8);
            } else {
                low.push(0xFF);
                high.push(0xFF);
                esc.extend_from_slice(&z.to_le_bytes());
            }
        }
    }
    let mut out = Vec::with_capacity(16 + none.len() + 2 * m * m + esc.len());
    out.extend_from_slice(&(m as u16).to_le_bytes());
    out.extend_from_slice(&(q as u16).to_le_bytes());
    out.extend_from_slice(&(esc.len() as u32).to_le_bytes());
    let all_none = none.iter().all(|&b| b == 0xFF) || g.cm.iter().all(|&v| v == NONE);
    out.push(all_none as u8);
    if !all_none {
        out.extend_from_slice(&none);
        out.extend_from_slice(&low);
        out.extend_from_slice(&high);
        out.extend_from_slice(&esc);
    }
    out
}

/// Décode une grille encodée au pas `step` m.
pub fn decode(b: &[u8], step: usize) -> Result<Nodes> {
    let mut r = crate::codec::Reader::new(b);
    let m = r.u16()? as usize;
    let q = i32::from(r.u16()?);
    let ne = r.u32()? as usize;
    let all_none = r.u8()?;
    if m == 0 || m > MAX_SIDE || q == 0 || step == 0 {
        return Err(Invalid);
    }
    let mut g = Nodes { step, side: m, cm: vec![NONE; m * m] };
    if all_none == 1 {
        r.finish()?;
        return Ok(g);
    }
    let none = r.slice((m * m).div_ceil(8))?;
    let low = r.slice(m * m)?;
    let high = r.slice(m * m)?;
    let esc = r.slice(ne)?;
    r.finish()?;
    let mut esc = crate::codec::Reader::new(esc);
    let mut rec = vec![0i32; m * m];
    for j in 0..m {
        for i in 0..m {
            let k = j * m + i;
            let pred = predict(&rec, m, i, j);
            let mut z = u32::from(low[k]) | u32::from(high[k]) << 8;
            if z == 0xFFFF {
                z = esc.u32()?;
            }
            let v = pred.wrapping_add(unzigzag(z));
            rec[k] = v;
            if none[k >> 3] >> (k & 7) & 1 == 0 {
                g.cm[k] = v.wrapping_mul(q);
            }
        }
    }
    esc.finish()?;
    Ok(g)
}

/// Nœuds impairs (1 m) prédits depuis la base de 2 m quantifiée : moyenne des
/// deux ou quatre voisins.
fn predict_odd(base: &Nodes, i: usize, j: usize, q: i32) -> Option<i32> {
    let g = |i: usize, j: usize| {
        let v = base.get(i, j);
        (v != NONE).then_some(i64::from(v / q))
    };
    let (a, c) = (i / 2, j / 2);
    let (sum, n) = match (i % 2, j % 2) {
        (1, 0) => (g(a, c)? + g(a + 1, c)?, 2),
        (0, 1) => (g(a, c)? + g(a, c + 1)?, 2),
        _ => (g(a, c)? + g(a + 1, c)? + g(a, c + 1)? + g(a + 1, c + 1)?, 4),
    };
    Some(div_round(sum.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32, n))
}

/// Base de 2 m et complément de 1 m d'une grille de 1 m (1 001 × 1 001).
pub fn encode_two(g: &Nodes, q: i32) -> (Vec<u8>, Vec<u8>) {
    let mut b = Nodes::new(2);
    for j in 0..b.side {
        for i in 0..b.side {
            b.cm[j * b.side + i] = g.get(2 * i, 2 * j);
        }
    }
    let base = encode(&b, q);
    // La base relue, telle que le décodeur la verra.
    let b = decode(&base, 2).unwrap_or(b);
    let (mut low, mut high) = (Vec::new(), Vec::new());
    for j in 0..g.side {
        for i in 0..g.side {
            if i % 2 == 0 && j % 2 == 0 {
                continue;
            }
            let v = g.get(i, j);
            let z = if v == NONE {
                0xFFFF
            } else {
                zigzag(div_round(v, q) - predict_odd(&b, i, j, q).unwrap_or(0)).min(0xFFFE)
            };
            low.push(z as u8);
            high.push((z >> 8) as u8);
        }
    }
    let mut fine = (q as u16).to_le_bytes().to_vec();
    fine.append(&mut low);
    fine.append(&mut high);
    (base, fine)
}

/// Grille de 1 m depuis la base et le complément.
pub fn decode_two(base: &[u8], fine: &[u8]) -> Result<Nodes> {
    let b = decode(base, 2)?;
    if b.side != 501 {
        return Err(Invalid);
    }
    let mut r = crate::codec::Reader::new(fine);
    let q = i32::from(r.u16()?);
    let n = MAX_SIDE * MAX_SIDE - b.side * b.side;
    let low = r.slice(n)?;
    let high = r.slice(n)?;
    r.finish()?;
    if q == 0 {
        return Err(Invalid);
    }
    let mut g = Nodes::new(1);
    let mut k = 0;
    for j in 0..g.side {
        for i in 0..g.side {
            g.cm[j * g.side + i] = if i % 2 == 0 && j % 2 == 0 {
                b.get(i / 2, j / 2)
            } else {
                let z = u32::from(low[k]) | u32::from(high[k]) << 8;
                k += 1;
                if z == 0xFFFF {
                    NONE
                } else {
                    predict_odd(&b, i, j, q).unwrap_or(0).wrapping_add(unzigzag(z)).wrapping_mul(q)
                }
            };
        }
    }
    Ok(g)
}
