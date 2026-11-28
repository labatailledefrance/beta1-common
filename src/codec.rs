//! `Writer` et `Reader` binaires, little-endian, écrits à la main (repris de
//! P-24 à P-26).
//!
//! Le `Reader` lit des octets venus d'Internet : il ne panique jamais. Toute
//! entrée invalide (tronquée, longueur annoncée plus grande que le reste,
//! flottant non fini, varint trop long) rend [`Invalid`].

use crate::geo::{Pos, TileId};

/// Accumule les octets d'un message.
#[derive(Default, Debug)]
pub struct Writer {
    /// Octets écrits.
    pub buf: Vec<u8>,
}

impl Writer {
    /// Octet.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    /// Entier de 16 bits.
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier signé de 16 bits.
    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier de 32 bits.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier signé de 32 bits.
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Entier de 64 bits.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Flottant de 32 bits.
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Varint (LEB128).
    pub fn varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.buf.push(v as u8 | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }
    /// Entier signé en varint zigzag (0, -1, 1, -2… → 0, 1, 2, 3…).
    pub fn svarint(&mut self, v: i64) {
        self.varint(((v << 1) ^ (v >> 63)) as u64);
    }
    /// Octets bruts, de longueur fixe connue du `Reader`.
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    /// Chaîne UTF-8, précédée de sa longueur.
    pub fn string(&mut self, s: &str) {
        self.varint(s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
    }
    /// Tile (i32 × 2).
    pub fn tile(&mut self, t: TileId) {
        self.i32(t.ix);
        self.i32(t.iy);
    }
    /// Position : tile + locale au 1/64 m (i16 × 2).
    pub fn pos(&mut self, p: Pos) {
        let p = p.normalized();
        self.tile(p.tile);
        for v in p.local {
            self.i16((v * 64.0).round().clamp(-32_000.0, 31_999.0) as i16);
        }
    }
}

/// Entrée invalide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalid;

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("message invalide")
    }
}

impl std::error::Error for Invalid {}

/// Résultat d'une lecture.
pub type Result<T> = std::result::Result<T, Invalid>;

/// Lecture bornée d'un message.
#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// `Reader` sur `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    /// Octets restants.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    /// Erreur s'il reste des octets : un message se lit en entier.
    pub fn finish(&self) -> Result<()> {
        if self.remaining() == 0 { Ok(()) } else { Err(Invalid) }
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.buf.get(self.pos..self.pos.checked_add(N).ok_or(Invalid)?).ok_or(Invalid)?;
        self.pos += N;
        s.try_into().map_err(|_| Invalid)
    }
    /// Octet.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }
    /// Entier de 16 bits.
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    /// Entier signé de 16 bits.
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take()?))
    }
    /// Entier de 32 bits.
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    /// Entier signé de 32 bits.
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take()?))
    }
    /// Entier de 64 bits.
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    /// Flottant fini de 32 bits.
    pub fn f32(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take()?);
        if v.is_finite() { Ok(v) } else { Err(Invalid) }
    }
    /// Varint (LEB128), 10 octets au plus.
    pub fn varint(&mut self) -> Result<u64> {
        let (mut v, mut shift) = (0u64, 0u32);
        loop {
            let b = self.u8()?;
            if shift > 63 || (shift == 63 && b > 1) {
                return Err(Invalid);
            }
            v |= u64::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                // Forme minimale seulement : un dernier octet nul ne s'écrit jamais
                // (sinon deux écritures pour une même valeur).
                if b == 0 && shift > 0 {
                    return Err(Invalid);
                }
                return Ok(v);
            }
            shift += 7;
        }
    }
    /// Entier signé en varint zigzag.
    pub fn svarint(&mut self) -> Result<i64> {
        let z = self.varint()?;
        Ok(((z >> 1) as i64) ^ -((z & 1) as i64))
    }
    /// Nombre d'éléments annoncé : jamais plus que d'octets restants, pour
    /// qu'un message court ne fasse pas allouer une grande table.
    pub fn count(&mut self) -> Result<usize> {
        let n = usize::try_from(self.varint()?).map_err(|_| Invalid)?;
        if n > self.remaining() { Err(Invalid) } else { Ok(n) }
    }
    /// Octets bruts de longueur fixe.
    pub fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.take()
    }
    /// Les `n` octets suivants, sans copie.
    pub fn slice(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.buf.get(self.pos..self.pos.checked_add(n).ok_or(Invalid)?).ok_or(Invalid)?;
        self.pos += n;
        Ok(s)
    }
    /// Tous les octets restants.
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.buf[self.pos..];
        self.pos = self.buf.len();
        s
    }
    /// Chaîne UTF-8 valide.
    pub fn string(&mut self) -> Result<String> {
        let n = self.count()?;
        let s = self.buf.get(self.pos..self.pos + n).ok_or(Invalid)?;
        self.pos += n;
        String::from_utf8(s.to_vec()).map_err(|_| Invalid)
    }
    /// Tile.
    pub fn tile(&mut self) -> Result<TileId> {
        Ok(TileId { ix: self.i32()?, iy: self.i32()? })
    }
    /// Position, telle que l'écrit [`Writer::pos`] : une position locale hors de
    /// [-500, 500[ est refusée (elle se réencoderait autrement).
    pub fn pos(&mut self) -> Result<Pos> {
        let tile = self.tile()?;
        let mut local = [0.0; 2];
        for l in &mut local {
            let v = self.i16()?;
            if !(-32_000..=31_999).contains(&v) {
                return Err(Invalid);
            }
            *l = f32::from(v) / 64.0;
        }
        Ok(Pos { tile, local })
    }
}
