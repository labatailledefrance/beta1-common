//! Lecture et écriture binaires, petit-boutiennes, écrites à la main (repris de
//! P-24 à P-26).
//!
//! Le décodeur lit des octets venus d'Internet : il ne panique jamais. Toute
//! entrée invalide (tronquée, longueur annoncée plus grande que le reste,
//! flottant non fini, entier variable trop long) rend [`Invalide`].

use crate::geo::{Pos, TileId};

/// Écrivain : accumule les octets d'un message.
#[derive(Default, Debug)]
pub struct Ecr {
    /// Octets écrits.
    pub b: Vec<u8>,
}

impl Ecr {
    /// Octet.
    pub fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    /// Entier de 16 bits.
    pub fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier signé de 16 bits.
    pub fn i16(&mut self, v: i16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier de 32 bits.
    pub fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier de 64 bits.
    pub fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Flottant de 32 bits.
    pub fn f32(&mut self, v: f32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier variable (LEB128).
    pub fn v(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.b.push(v as u8 | 0x80);
            v >>= 7;
        }
        self.b.push(v as u8);
    }
    /// Octets bruts, de longueur fixe connue du lecteur.
    pub fn octets(&mut self, o: &[u8]) {
        self.b.extend_from_slice(o);
    }
    /// Texte UTF-8, précédé de sa longueur.
    pub fn txt(&mut self, s: &str) {
        self.v(s.len() as u64);
        self.b.extend_from_slice(s.as_bytes());
    }
    /// Tile (i32 × 2).
    pub fn tile(&mut self, t: TileId) {
        self.b.extend_from_slice(&t.ix.to_le_bytes());
        self.b.extend_from_slice(&t.iy.to_le_bytes());
    }
    /// Position : tile + locale au 1/64 m (i16 × 2).
    pub fn pos(&mut self, p: Pos) {
        let p = p.norm();
        self.tile(p.t);
        for v in p.l {
            self.i16((v * 64.0).round().clamp(-32_000.0, 31_999.0) as i16);
        }
    }
}

/// Entrée invalide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalide;

impl std::fmt::Display for Invalide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("message invalide")
    }
}

impl std::error::Error for Invalide {}

/// Résultat d'une lecture.
pub type R<T> = Result<T, Invalide>;

/// Lecteur borné.
#[derive(Debug)]
pub struct Lec<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Lec<'a> {
    /// Lecteur sur `b`.
    pub fn new(b: &'a [u8]) -> Self {
        Lec { b, p: 0 }
    }
    /// Octets restants.
    pub fn reste(&self) -> usize {
        self.b.len() - self.p
    }
    /// Erreur s'il reste des octets : un message se lit en entier.
    pub fn fin(&self) -> R<()> {
        if self.reste() == 0 { Ok(()) } else { Err(Invalide) }
    }
    fn prendre<const N: usize>(&mut self) -> R<[u8; N]> {
        let s = self.b.get(self.p..self.p.checked_add(N).ok_or(Invalide)?).ok_or(Invalide)?;
        self.p += N;
        s.try_into().map_err(|_| Invalide)
    }
    /// Octet.
    pub fn u8(&mut self) -> R<u8> {
        Ok(self.prendre::<1>()?[0])
    }
    /// Entier de 16 bits.
    pub fn u16(&mut self) -> R<u16> {
        Ok(u16::from_le_bytes(self.prendre()?))
    }
    /// Entier signé de 16 bits.
    pub fn i16(&mut self) -> R<i16> {
        Ok(i16::from_le_bytes(self.prendre()?))
    }
    /// Entier de 32 bits.
    pub fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.prendre()?))
    }
    /// Entier signé de 32 bits.
    pub fn i32(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.prendre()?))
    }
    /// Entier de 64 bits.
    pub fn u64(&mut self) -> R<u64> {
        Ok(u64::from_le_bytes(self.prendre()?))
    }
    /// Flottant fini de 32 bits.
    pub fn f32(&mut self) -> R<f32> {
        let v = f32::from_le_bytes(self.prendre()?);
        if v.is_finite() { Ok(v) } else { Err(Invalide) }
    }
    /// Entier variable (LEB128), 10 octets au plus.
    pub fn v(&mut self) -> R<u64> {
        let (mut v, mut s) = (0u64, 0u32);
        loop {
            let o = self.u8()?;
            if s > 63 || (s == 63 && o > 1) {
                return Err(Invalide);
            }
            v |= u64::from(o & 0x7F) << s;
            if o & 0x80 == 0 {
                return Ok(v);
            }
            s += 7;
        }
    }
    /// Nombre d'éléments annoncé : jamais plus que d'octets restants, pour
    /// qu'un message court ne fasse pas allouer une grande table.
    pub fn nb(&mut self) -> R<usize> {
        let n = usize::try_from(self.v()?).map_err(|_| Invalide)?;
        if n > self.reste() { Err(Invalide) } else { Ok(n) }
    }
    /// Octets bruts de longueur fixe.
    pub fn octets<const N: usize>(&mut self) -> R<[u8; N]> {
        self.prendre()
    }
    /// Texte UTF-8 valide.
    pub fn txt(&mut self) -> R<String> {
        let n = self.nb()?;
        let s = self.b.get(self.p..self.p + n).ok_or(Invalide)?;
        self.p += n;
        String::from_utf8(s.to_vec()).map_err(|_| Invalide)
    }
    /// Tile.
    pub fn tile(&mut self) -> R<TileId> {
        Ok(TileId { ix: self.i32()?, iy: self.i32()? })
    }
    /// Position.
    pub fn pos(&mut self) -> R<Pos> {
        let t = self.tile()?;
        let l = [f32::from(self.i16()?) / 64.0, f32::from(self.i16()?) / 64.0];
        Ok(Pos { t, l }.norm())
    }
}
