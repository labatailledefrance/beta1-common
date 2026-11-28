//! Format des tiles (H-06), produit par `beta1-datachain` et lu par le serveur
//! et le client.
//!
//! Une tile est un en-tête, une table de sections, les sections (chacune
//! compressée seule en zstd : la vue de loin ne décode pas le relief de 1 m),
//! puis l'empreinte BLAKE3 de tout ce qui précède. Les tiles sont rangées dans
//! des **paquets** de 16 × 16 (un fichier par bloc de 16 km), index en tête :
//! lire une tile, c'est une lecture à un décalage connu, comme une couche
//! d'image de conteneur.
//!
//! Tout est petit-boutien ; les entiers variables sont en LEB128 (zigzag pour
//! les signés), comme le [`codec`](crate::codec).
//!
//! Le décodage ne panique jamais : une tile vient d'Internet (client) ou d'un
//! disque (serveur), et toute entrée invalide rend [`Invalid`]. Il est en Rust
//! pur (zstd par `ruzstd`) pour compiler en WebAssembly.

pub mod codes;
pub mod relief;
pub mod sections;

use std::io::Read;

use crate::codec::{Invalid, Result};
use crate::geo::TileId;

/// Signature d'une tile.
pub const MAGIC: &[u8; 4] = b"P07T";
/// Version du format (2 : zone, complément de relief, toit, matériaux, porte, routes des ponts).
pub const VERSION: u8 = 2;
/// Signature d'un paquet.
pub const PACK_MAGIC: &[u8; 4] = b"P07P";
/// Côté d'un paquet, en tiles.
pub const PACK_SIDE: i32 = 16;
/// Quantification des coordonnées locales : 1/16 m, relatives au centre de la
/// tile (C-01).
pub const QUANT: f64 = 16.0;
/// Taille décompressée maximale d'une section : borne contre les bombes zstd.
pub const MAX_SECTION: usize = 32 << 20;
const HASH: usize = 32;
/// Taille de la partie fixe de l'en-tête (nombre de sections compris) : ce que lit [`decode_header`].
pub const HEADER: usize = 26;

/// Sections d'une tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum SectionId {
    /// Relief, base de 2 m ([`relief`]).
    Relief = 1,
    /// Bâtiments ([`sections::buildings`]).
    Buildings = 2,
    /// Cases de 2 m franchissables à pied ([`sections::Bitmap`]).
    Passable = 3,
    /// Zone accessible : exclusions et passages percés ([`sections::zone`]).
    Zone = 4,
    /// Routes et voies ferrées ([`sections::network`]).
    Network = 5,
    /// Ponts ([`sections::bridges`]).
    Bridges = 6,
    /// Occupation du sol à 2 m ([`sections::occupation`]).
    Occupation = 7,
    /// Communes de la tile ([`sections::communes`]).
    Communes = 8,
    /// Lieux d'intérêt ([`sections::places`]).
    Places = 9,
    /// Complément de 1 m du relief ([`relief::decode_two`]).
    Relief1m = 10,
}

impl SectionId {
    /// Toutes les sections, dans l'ordre de la tile.
    pub const ALL: [SectionId; 10] = [
        SectionId::Relief,
        SectionId::Buildings,
        SectionId::Passable,
        SectionId::Zone,
        SectionId::Network,
        SectionId::Bridges,
        SectionId::Occupation,
        SectionId::Communes,
        SectionId::Places,
        SectionId::Relief1m,
    ];
    /// Section de numéro `v`.
    pub fn from_u8(v: u8) -> Option<SectionId> {
        SectionId::ALL.into_iter().find(|s| *s as u8 == v)
    }
    /// Nom court (rapports de la chaîne).
    pub fn name(self) -> &'static str {
        match self {
            SectionId::Relief => "relief",
            SectionId::Buildings => "bati",
            SectionId::Passable => "franchissable",
            SectionId::Zone => "zone",
            SectionId::Network => "reseau",
            SectionId::Bridges => "ponts",
            SectionId::Occupation => "occupation",
            SectionId::Communes => "communes",
            SectionId::Places => "lieux",
            SectionId::Relief1m => "relief1m",
        }
    }
}

/// Source du relief, dans [`Header::flags`] (bits 4 et 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReliefSource {
    /// Pas de relief.
    None,
    /// LiDAR HD (P-26 seulement, plus produit : B-01).
    LidarHd,
    /// RGE ALTI 1 m.
    RgeAlti1m,
    /// RGE ALTI 5 m.
    RgeAlti5m,
}

/// En-tête : ce qu'il faut sans rien décompresser (vue de loin, tiles inertes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// Zone de projection (voir [`crate::geo::ZONES`]).
    pub zone: u8,
    /// Colonne dans la zone (km).
    pub ix: i32,
    /// Ligne (km).
    pub iy: i32,
    /// 1 terre, 2 mer, 4 relief présent, 8 étranger ; bits 4-5 : [`ReliefSource`].
    pub flags: u8,
    /// Part de bâti, sur 65 535.
    pub built_share: u16,
    /// Habitants initiaux.
    pub inhabitants: u32,
    /// Nombre de bâtiments.
    pub buildings: u32,
}

impl Header {
    /// Tile, zone comprise.
    pub fn tile(&self) -> Option<TileId> {
        TileId::from_zone(self.zone, self.ix, self.iy)
    }
    /// La tile a de la terre.
    pub fn land(&self) -> bool {
        self.flags & 1 != 0
    }
    /// La tile a de la mer.
    pub fn sea(&self) -> bool {
        self.flags & 2 != 0
    }
    /// La tile est surtout hors de France.
    pub fn foreign(&self) -> bool {
        self.flags & 8 != 0
    }
    /// Source majoritaire du relief.
    pub fn relief_source(&self) -> ReliefSource {
        match self.flags >> 4 & 3 {
            1 => ReliefSource::LidarHd,
            2 => ReliefSource::RgeAlti1m,
            3 => ReliefSource::RgeAlti5m,
            _ => ReliefSource::None,
        }
    }
}

/// Une section telle qu'écrite : données compressées et taille décompressée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Section.
    pub id: SectionId,
    /// Taille décompressée.
    pub raw_len: usize,
    /// Données compressées (zstd).
    pub data: Vec<u8>,
}

/// Assemble une tile à partir de sections déjà compressées (la chaîne
/// compresse avec la bibliothèque zstd ; le format n'en dépend pas).
pub fn encode(h: &Header, sections: &[Section]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(HEADER + 9 * sections.len() + sections.iter().map(|s| s.data.len()).sum::<usize>() + HASH);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(h.zone);
    out.extend_from_slice(&h.ix.to_le_bytes());
    out.extend_from_slice(&h.iy.to_le_bytes());
    out.push(h.flags);
    out.extend_from_slice(&h.built_share.to_le_bytes());
    out.extend_from_slice(&h.inhabitants.to_le_bytes());
    out.extend_from_slice(&h.buildings.to_le_bytes());
    out.push(sections.len() as u8);
    for s in sections {
        out.push(s.id as u8);
        out.extend_from_slice(&(s.raw_len as u32).to_le_bytes());
        out.extend_from_slice(&(s.data.len() as u32).to_le_bytes());
    }
    for s in sections {
        out.extend_from_slice(&s.data);
    }
    let h = blake3::hash(&out);
    out.extend_from_slice(h.as_bytes());
    out
}

/// Lit le seul en-tête d'une tile (ses [`HEADER`] premiers octets suffisent),
/// sans vérifier l'empreinte : pour parcourir les tiles d'un paquet sans les lire.
pub fn decode_header(b: &[u8]) -> Result<Header> {
    let b = b.get(..HEADER).ok_or(Invalid)?;
    if &b[..4] != MAGIC || b[4] != VERSION {
        return Err(Invalid);
    }
    let mut r = crate::codec::Reader::new(&b[5..]);
    let h = Header {
        zone: r.u8()?,
        ix: r.i32()?,
        iy: r.i32()?,
        flags: r.u8()?,
        built_share: r.u16()?,
        inhabitants: r.u32()?,
        buildings: r.u32()?,
    };
    Ok(h)
}

/// Une tile lue, sections encore compressées.
#[derive(Debug)]
pub struct Tile<'a> {
    /// En-tête.
    pub header: Header,
    /// Sections : (section, taille décompressée, données compressées).
    pub sections: Vec<(SectionId, usize, &'a [u8])>,
}

/// Lit une tile. `verify` : contrôle l'empreinte (toujours pour des octets
/// venus du réseau).
pub fn decode(b: &[u8], verify: bool) -> Result<Tile<'_>> {
    if b.len() < HEADER + HASH || &b[..4] != MAGIC || b[4] != VERSION {
        return Err(Invalid);
    }
    let corps = b.len() - HASH;
    if verify && blake3::hash(&b[..corps]).as_bytes() != &b[corps..] {
        return Err(Invalid);
    }
    let mut r = crate::codec::Reader::new(&b[5..corps]);
    let zone = r.u8()?;
    let ix = r.i32()?;
    let iy = r.i32()?;
    let flags = r.u8()?;
    let built_share = r.u16()?;
    let inhabitants = r.u32()?;
    let buildings = r.u32()?;
    let n = r.u8()? as usize;
    let mut table = Vec::with_capacity(n);
    for _ in 0..n {
        let id = SectionId::from_u8(r.u8()?).ok_or(Invalid)?;
        let raw = r.u32()? as usize;
        let len = r.u32()? as usize;
        if raw > MAX_SECTION || table.iter().any(|(s, _, _)| *s == id) {
            return Err(Invalid);
        }
        table.push((id, raw, len));
    }
    let mut sections = Vec::with_capacity(n);
    for (id, raw, len) in table {
        sections.push((id, raw, r.slice(len)?));
    }
    r.finish()?;
    Ok(Tile { header: Header { zone, ix, iy, flags, built_share, inhabitants, buildings }, sections })
}

impl Tile<'_> {
    /// Section décompressée, si la tile l'a. La taille doit être exactement
    /// celle annoncée.
    pub fn section(&self, id: SectionId) -> Result<Option<Vec<u8>>> {
        let Some(&(_, raw, data)) = self.sections.iter().find(|s| s.0 == id) else {
            return Ok(None);
        };
        decompress(data, raw).map(Some)
    }
}

/// Décompresse un flux zstd de taille connue, sans jamais produire plus.
pub fn decompress(data: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    if raw_len > MAX_SECTION {
        return Err(Invalid);
    }
    let mut src = data;
    let dec = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|_| Invalid)?;
    let mut out = Vec::with_capacity(raw_len);
    dec.take(raw_len as u64 + 1).read_to_end(&mut out).map_err(|_| Invalid)?;
    if out.len() == raw_len { Ok(out) } else { Err(Invalid) }
}

/// Paquet d'une tile : (bx, by), en blocs de [`PACK_SIDE`] tiles.
pub fn pack_of(ix: i32, iy: i32) -> (i32, i32) {
    (ix.div_euclid(PACK_SIDE), iy.div_euclid(PACK_SIDE))
}

/// Entrée de l'index d'un paquet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackEntry {
    /// Colonne dans la zone (km).
    pub ix: i32,
    /// Ligne (km).
    pub iy: i32,
    /// Décalage de la tile dans le fichier.
    pub offset: u64,
    /// Taille de la tile.
    pub len: u32,
}

/// Écrit un paquet : index (nombre, puis ix, iy, décalage, taille), puis les
/// tiles dans l'ordre donné.
pub fn encode_pack(tiles: &[(i32, i32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&(tiles.len() as u32).to_le_bytes());
    let mut off = (8 + tiles.len() * 20) as u64;
    for (ix, iy, b) in tiles {
        out.extend_from_slice(&ix.to_le_bytes());
        out.extend_from_slice(&iy.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        off += b.len() as u64;
    }
    for (_, _, b) in tiles {
        out.extend_from_slice(b);
    }
    out
}

/// Index d'un paquet, lu dans ses premiers octets (au moins tout l'index).
pub fn decode_pack_index(b: &[u8]) -> Result<Vec<PackEntry>> {
    let mut r = crate::codec::Reader::new(b);
    if &r.bytes::<4>()? != PACK_MAGIC {
        return Err(Invalid);
    }
    let n = r.u32()? as usize;
    if n > (PACK_SIDE * PACK_SIDE) as usize || r.remaining() < n * 20 {
        return Err(Invalid);
    }
    (0..n).map(|_| Ok(PackEntry { ix: r.i32()?, iy: r.i32()?, offset: r.u64()?, len: r.u32()? })).collect()
}

/// Taille de l'index d'un paquet de `n` tiles.
pub fn pack_index_len(n: usize) -> usize {
    8 + n * 20
}
