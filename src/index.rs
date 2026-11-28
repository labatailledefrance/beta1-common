//! Index national des tiles (`index.bin`), produit par `beta1-datachain` une
//! fois par publication, lu par le serveur et par le client.
//!
//! Il reprend, pour chaque tile de la publication, ce que porte son en-tête
//! ([`Header`](crate::tile::Header)) : drapeaux, part de bâti, habitants. Le
//! serveur y trouve la population de départ des 790 759 tiles sans lire un
//! seul paquet (les en-têtes, lus un par un, demandent 5 à 6 min à froid sur
//! srv) ; le client, la France entière vue de loin, en un fichier immuable gardé
//! en cache (A-01, P-04, H-06). Comme un fichier de zone DNS face aux
//! enregistrements : une table compacte, signée par son empreinte, qu'on
//! distribue entière plutôt que d'interroger chaque entrée.
//!
//! Format :
//!
//! ```text
//! "B1IX", version (u8), taille brute (u64), taille compressée (u64),
//! corps compressé (zstd), empreinte BLAKE3 de tout ce qui précède
//! ```
//!
//! Le corps, en colonnes (elles se compressent mieux que des lignes) :
//!
//! ```text
//! nombre de colonnes (varint)
//! par colonne de même ix, dans l'ordre croissant :
//!   ix : la première en svarint, les suivantes en varint (ix - précédente - 1)
//!   nombre de plages de iy consécutifs, moins 1 (varint)
//!   par plage : début (la première en svarint, les suivantes en varint
//!   (début - fin de la précédente - 2)), longueur moins 1 (varint)
//! drapeaux : un octet par tile, dans l'ordre des tiles
//! part de bâti : un varint par tile
//! habitants : un varint par tile
//! ```
//!
//! Les tiles sont triées par [`TileId`] (ix, puis iy) et les plages sont
//! maximales : un index n'a qu'une écriture ([`NationalIndex::decode_body`]
//! refuse toute autre, varints non minimaux compris).

use crate::codec::{Invalid, Reader, Result, Writer};
use crate::geo::TileId;
use crate::tile::decompress;

/// Signature du fichier.
pub const MAGIC: &[u8; 4] = b"B1IX";
/// Version du format.
pub const VERSION: u8 = 1;
/// Tiles au plus (la France en a 790 759) : borne contre un fichier hostile.
pub const MAX_TILES: usize = 1 << 22;
const HASH: usize = 32;
const ENTETE: usize = 4 + 1 + 8 + 8;

/// Ce que l'index retient d'une tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileInfo {
    /// Tile, zone comprise.
    pub tile: TileId,
    /// Drapeaux de l'en-tête (1 terre, 2 mer, 4 relief, 8 étranger ; bits 4-5 : source du relief).
    pub flags: u8,
    /// Part de bâti, sur 65 535.
    pub built_share: u16,
    /// Habitants initiaux.
    pub inhabitants: u32,
}

impl TileInfo {
    /// Reprend l'en-tête d'une tile ; `None` si sa zone est inconnue.
    pub fn from_header(h: &crate::tile::Header) -> Option<TileInfo> {
        Some(TileInfo { tile: h.tile()?, flags: h.flags, built_share: h.built_share, inhabitants: h.inhabitants })
    }
}

/// Index national : tiles triées par [`TileId`], sans doublon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NationalIndex {
    tiles: Vec<TileInfo>,
}

impl NationalIndex {
    /// Trie les tiles ; une tile en double, ou plus de [`MAX_TILES`] tiles, est refusée.
    pub fn new(mut tiles: Vec<TileInfo>) -> Result<NationalIndex> {
        tiles.sort_by_key(|t| t.tile);
        if tiles.len() > MAX_TILES || tiles.windows(2).any(|w| w[0].tile == w[1].tile) {
            return Err(Invalid);
        }
        Ok(NationalIndex { tiles })
    }

    /// Tiles, dans l'ordre.
    pub fn tiles(&self) -> &[TileInfo] {
        &self.tiles
    }

    /// Nombre de tiles.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Aucune tile.
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Rang de la tile dans l'index (`None` : hors des données, de la mer).
    pub fn rank(&self, t: TileId) -> Option<usize> {
        self.tiles.binary_search_by_key(&t, |x| x.tile).ok()
    }

    /// Tile de rang `k`.
    pub fn get(&self, k: usize) -> Option<&TileInfo> {
        self.tiles.get(k)
    }

    /// Corps non compressé, dans sa seule écriture.
    pub fn encode_body(&self) -> Vec<u8> {
        let mut w = Writer::default();
        // Colonnes : tranches de même ix.
        let colonnes: Vec<&[TileInfo]> = self.tiles.chunk_by(|a, b| a.tile.ix == b.tile.ix).collect();
        w.varint(colonnes.len() as u64);
        let mut ix_prec: Option<i32> = None;
        for c in colonnes {
            let ix = c[0].tile.ix;
            match ix_prec {
                None => w.svarint(ix.into()),
                Some(p) => w.varint((i64::from(ix) - i64::from(p) - 1) as u64),
            }
            ix_prec = Some(ix);
            let plages: Vec<&[TileInfo]> =
                c.chunk_by(|a, b| i64::from(b.tile.iy) == i64::from(a.tile.iy) + 1).collect();
            w.varint(plages.len() as u64 - 1);
            let mut fin: Option<i32> = None;
            for p in plages {
                let debut = p[0].tile.iy;
                match fin {
                    None => w.svarint(debut.into()),
                    Some(f) => w.varint((i64::from(debut) - i64::from(f) - 2) as u64),
                }
                w.varint(p.len() as u64 - 1);
                fin = Some(p[p.len() - 1].tile.iy);
            }
        }
        for t in &self.tiles {
            w.u8(t.flags);
        }
        for t in &self.tiles {
            w.varint(t.built_share.into());
        }
        for t in &self.tiles {
            w.varint(t.inhabitants.into());
        }
        w.buf
    }

    /// Lit un corps non compressé ; refuse tout ce que [`encode_body`](Self::encode_body) n'écrirait pas.
    pub fn decode_body(b: &[u8]) -> Result<NationalIndex> {
        let mut r = Reader::new(b);
        let ncol = r.count()?;
        let mut ids: Vec<TileId> = Vec::new();
        let mut ix_prec: Option<i64> = None;
        for _ in 0..ncol {
            let ix = match ix_prec {
                None => r.svarint()?,
                Some(p) => p.checked_add(i64::try_from(r.varint()?).map_err(|_| Invalid)?).ok_or(Invalid)? + 1,
            };
            let ix = i32::try_from(ix).map_err(|_| Invalid)?;
            ix_prec = Some(ix.into());
            let nplages = usize::try_from(r.varint()?).map_err(|_| Invalid)?.checked_add(1).ok_or(Invalid)?;
            if nplages > r.remaining() {
                return Err(Invalid);
            }
            let mut fin: Option<i64> = None;
            for _ in 0..nplages {
                let debut = match fin {
                    None => r.svarint()?,
                    Some(f) => f.checked_add(i64::try_from(r.varint()?).map_err(|_| Invalid)?).ok_or(Invalid)? + 2,
                };
                let n = usize::try_from(r.varint()?).map_err(|_| Invalid)?.checked_add(1).ok_or(Invalid)?;
                // Chaque tile demande au moins un octet de drapeaux, un de bâti, un d'habitants.
                if ids.len().saturating_add(n) > MAX_TILES || n > r.remaining() / 3 {
                    return Err(Invalid);
                }
                let derniere = i32::try_from(debut.checked_add(n as i64 - 1).ok_or(Invalid)?).map_err(|_| Invalid)?;
                let debut = i32::try_from(debut).map_err(|_| Invalid)?;
                ids.extend((debut..=derniere).map(|iy| TileId { ix, iy }));
                fin = Some(derniere.into());
            }
        }
        let flags = r.slice(ids.len())?;
        let mut tiles: Vec<TileInfo> = ids
            .iter()
            .zip(flags)
            .map(|(&tile, &flags)| TileInfo { tile, flags, built_share: 0, inhabitants: 0 })
            .collect();
        for t in &mut tiles {
            t.built_share = u16::try_from(r.varint()?).map_err(|_| Invalid)?;
        }
        for t in &mut tiles {
            t.inhabitants = u32::try_from(r.varint()?).map_err(|_| Invalid)?;
        }
        r.finish()?;
        Ok(NationalIndex { tiles })
    }

    /// Fichier complet ; `compress` est le compresseur zstd de l'appelant (la
    /// chaîne a la bibliothèque C, le format n'en dépend pas).
    pub fn encode(&self, compress: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
        let corps = self.encode_body();
        let z = compress(&corps);
        let mut out = Vec::with_capacity(ENTETE + z.len() + HASH);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&(corps.len() as u64).to_le_bytes());
        out.extend_from_slice(&(z.len() as u64).to_le_bytes());
        out.extend_from_slice(&z);
        let h = blake3::hash(&out);
        out.extend_from_slice(h.as_bytes());
        out
    }

    /// Lit un fichier complet : signature, version, empreinte, corps.
    pub fn decode(b: &[u8]) -> Result<NationalIndex> {
        let corps = b.len().checked_sub(HASH).ok_or(Invalid)?;
        if corps < ENTETE || &b[..4] != MAGIC || b[4] != VERSION || blake3::hash(&b[..corps]).as_bytes() != &b[corps..]
        {
            return Err(Invalid);
        }
        let mut r = Reader::new(&b[5..corps]);
        let brut = usize::try_from(r.u64()?).map_err(|_| Invalid)?;
        let z = usize::try_from(r.u64()?).map_err(|_| Invalid)?;
        let donnees = r.slice(z)?;
        r.finish()?;
        // Taille brute bornée par celle d'une section de tile (32 Mio ; France : 4 Mo environ).
        NationalIndex::decode_body(&decompress(donnees, brut)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(ix: i32, iy: i32, h: u32) -> TileInfo {
        TileInfo {
            tile: TileId { ix, iy },
            flags: (h % 3) as u8,
            built_share: h.wrapping_mul(7) as u16,
            inhabitants: h,
        }
    }

    fn exemple() -> NationalIndex {
        let mut v = vec![info(-3, -5, 1), info(-3, -4, 0), info(-3, -2, 900_000), info(0, 6837, 12)];
        v.extend((0..40).map(|k| info(145, 6800 + k, k as u32 * 100)));
        v.push(info(145, 6900, 3));
        v.push(info(2 * 2048 + 600, 1800, 4));
        v.push(info(i32::MAX, i32::MIN, u32::MAX));
        NationalIndex::new(v).unwrap_or_default()
    }

    /// Compresseur d'essai : une trame zstd d'un seul bloc brut (non compressé), lisible par ruzstd.
    fn brut(b: &[u8]) -> Vec<u8> {
        assert!(b.len() < 256, "essai : un bloc court");
        // Signature, descripteur (taille de contenu sur 1 octet, segment unique), taille.
        let mut out = vec![0x28, 0xB5, 0x2F, 0xFD, 0x20, b.len() as u8];
        // En-tête de bloc : dernier bloc, type brut, taille.
        out.extend_from_slice(&(1 | (b.len() as u32) << 3).to_le_bytes()[..3]);
        out.extend_from_slice(b);
        out
    }

    #[test]
    fn aller_retour() {
        let x = exemple();
        assert_eq!(x.len(), 47);
        let corps = x.encode_body();
        assert_eq!(NationalIndex::decode_body(&corps), Ok(x.clone()));
        assert_eq!(x.rank(TileId { ix: 145, iy: 6810 }), Some(14));
        assert_eq!(x.rank(TileId { ix: 145, iy: 6850 }), None);
        let petit = NationalIndex::new(vec![info(145, 6838, 9), info(146, 6838, 3)]).unwrap_or_default();
        let f = petit.encode(brut);
        assert_eq!(NationalIndex::decode(&f), Ok(petit));
        let mut faux = f.clone();
        faux[ENTETE] ^= 1;
        assert_eq!(NationalIndex::decode(&faux), Err(Invalid));
    }

    #[test]
    fn une_seule_ecriture() {
        assert_eq!(NationalIndex::new(vec![info(1, 1, 0), info(1, 1, 2)]), Err(Invalid));
        // Deux tiles voisines : une seule plage (colonnes 1, ix 0, 1 plage, début 5, longueur 2).
        let x = NationalIndex::new(vec![info(0, 5, 1), info(0, 6, 2)]).unwrap_or_default();
        let corps = x.encode_body();
        assert_eq!(corps[..5], [1, 0, 0, 10, 1]);
        // Écrire ces deux tiles en deux plages est impossible : l'écart suivant compte à partir de 2.
        // Varint non minimal pour les habitants : refusé.
        let mut long = corps.clone();
        let n = long.len();
        long[n - 1] |= 0x80;
        long.push(0);
        assert_eq!(NationalIndex::decode_body(&long), Err(Invalid));
        // Octet en trop : refusé.
        let mut trop = corps;
        trop.push(0);
        assert_eq!(NationalIndex::decode_body(&trop), Err(Invalid));
        // Débordement de i32 : refusé.
        let mut w = Writer::default();
        w.varint(1);
        w.svarint(i64::from(i32::MAX) + 1);
        assert_eq!(NationalIndex::decode_body(&w.buf), Err(Invalid));
    }
}
