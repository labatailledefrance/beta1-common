//! Décodage des sections d'une tile (données déjà décompressées).
//!
//! Chaque décodeur lit **tout** ce que la chaîne écrit et exige d'arriver
//! exactement au bout : un champ ajouté d'un côté seulement se voit tout de
//! suite. Coordonnées : mètres, locales au centre de la tile (C-01), lues au
//! 1/16 m ([`QUANT`](super::QUANT)), exactes en f32.

use crate::codec::{Invalid, Reader, Result};

use super::QUANT;

/// Côté des grilles de 2 m d'une tile.
pub const GRID: usize = 500;
/// Cases d'une grille de 2 m.
pub const CELLS: usize = GRID * GRID;
/// Taille d'une [`Bitmap`] de 500 × 500 en octets.
pub const BITMAP_LEN: usize = CELLS.div_ceil(64) * 8;

/// Grille de 500 × 500 bits (cases de 2 m), mots de 64 bits petit-boutiens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    /// Mots de 64 bits ; le bit `j * 500 + i` est la case (i, j).
    pub words: Vec<u64>,
}

impl Bitmap {
    /// Lit une grille de [`BITMAP_LEN`] octets.
    pub fn decode(b: &[u8]) -> Result<Bitmap> {
        if b.len() != BITMAP_LEN {
            return Err(Invalid);
        }
        Ok(Bitmap { words: b.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect() })
    }
    /// Case (i, j), ligne 0 au sud.
    pub fn get(&self, i: usize, j: usize) -> bool {
        let k = j * GRID + i;
        i < GRID && self.words.get(k >> 6).is_some_and(|w| w >> (k & 63) & 1 == 1)
    }
    /// Nombre de cases à 1.
    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

fn coord(v: i64) -> f32 {
    (v as f64 / QUANT) as f32
}

/// Anneau de polygone : points en m, non fermé (le dernier ne répète pas le premier).
pub type Ring = Vec<[f32; 2]>;
/// Polygone : anneau extérieur puis trous.
pub type Polygon = Vec<Ring>;

fn ring(r: &mut Reader) -> Result<Ring> {
    let n = r.count()?;
    let (mut x, mut y) = (0i64, 0i64);
    (0..n)
        .map(|_| {
            x = x.wrapping_add(r.svarint()?);
            y = y.wrapping_add(r.svarint()?);
            Ok([coord(x), coord(y)])
        })
        .collect()
}

fn polygon(r: &mut Reader) -> Result<Polygon> {
    let n = r.count()?;
    (0..n).map(|_| ring(r)).collect()
}

/// Ligne : points en m ; z en m (dm dans le format), 0 si la ligne est au sol.
fn line(r: &mut Reader, z: bool) -> Result<Vec<[f32; 3]>> {
    let n = r.count()?;
    let (mut x, mut y, mut zz) = (0i64, 0i64, 0i64);
    (0..n)
        .map(|_| {
            x = x.wrapping_add(r.svarint()?);
            y = y.wrapping_add(r.svarint()?);
            if z {
                zz = zz.wrapping_add(r.svarint()?);
            }
            Ok([coord(x), coord(y), zz as f32 / 10.0])
        })
        .collect()
}

// ------------------------------------------------------------------ Bâtiments

/// Un bâtiment de la BD TOPO.
#[derive(Clone, Debug, PartialEq)]
pub struct Building {
    /// Identifiant (chiffres du `cleabs`).
    pub id: u64,
    /// Nature ([`codes::BUILDING_NATURES`](super::codes::BUILDING_NATURES)).
    pub nature: u8,
    /// Usage principal ([`codes::USAGES`](super::codes::USAGES)).
    pub usage1: u8,
    /// Usage secondaire.
    pub usage2: u8,
    /// Construction légère.
    pub light: bool,
    /// Hauteur (dm).
    pub height_dm: Option<u32>,
    /// Nombre d'étages.
    pub floors: Option<u32>,
    /// Nombre de logements.
    pub dwellings: Option<u32>,
    /// Hauteur du toit (dm).
    pub roof_dm: Option<u32>,
    /// Matériaux (murs, toiture), codes des fichiers fonciers (à vérifier).
    pub materials: Option<(u8, u8)>,
    /// Porte : (anneau, segment) de l'emprise.
    pub door: Option<(u32, u32)>,
    /// Emprise.
    pub polygons: Vec<Polygon>,
}

fn small(v: u64) -> Result<u32> {
    u32::try_from(v).map_err(|_| Invalid)
}

/// Section [`SectionId::Buildings`](super::SectionId::Buildings).
pub fn buildings(b: &[u8]) -> Result<Vec<Building>> {
    let mut r = Reader::new(b);
    let n = r.count()?;
    let mut out = Vec::with_capacity(n);
    let mut id = 0u64;
    for _ in 0..n {
        id = id.wrapping_add(r.varint()?);
        let nature = r.u8()?;
        let usage1 = r.u8()?;
        let usage2 = r.u8()?;
        let f = r.u8()?;
        if f & 0x80 != 0 {
            return Err(Invalid);
        }
        let mut opt = |bit: u8| -> Result<Option<u32>> {
            if f & bit != 0 { r.varint().and_then(small).map(Some) } else { Ok(None) }
        };
        let height_dm = opt(2)?;
        let floors = opt(4)?;
        let dwellings = opt(8)?;
        let roof_dm = opt(16)?;
        let materials = if f & 32 != 0 { Some((r.u8()?, r.u8()?)) } else { None };
        let door = if f & 64 != 0 { Some((small(r.varint()?)?, small(r.varint()?)?)) } else { None };
        let np = r.count()?;
        let polygons = (0..np).map(|_| polygon(&mut r)).collect::<Result<_>>()?;
        out.push(Building {
            id,
            nature,
            usage1,
            usage2,
            light: f & 1 != 0,
            height_dm,
            floors,
            dwellings,
            roof_dm,
            materials,
            door,
            polygons,
        });
    }
    r.finish()?;
    Ok(out)
}

// -------------------------------------------------------------------- Réseau

/// Morceau d'un tronçon de route dans la tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Road {
    /// Identifiant du tronçon.
    pub id: u64,
    /// Rang du morceau dans le tronçon (coupé aux bords des tiles).
    pub rank: u32,
    /// Nature ([`codes::ROAD_NATURES`](super::codes::ROAD_NATURES)).
    pub nature: u8,
    /// Importance (1 à 6).
    pub importance: u8,
    /// Position par rapport au sol (négatif : souterrain).
    pub level: i8,
    /// Largeur de chaussée (dm).
    pub width_dm: u32,
    /// Nombre de voies.
    pub lanes: u8,
    /// Sens ([`codes::DIRECTIONS`](super::codes::DIRECTIONS)).
    pub direction: u8,
    /// Accès des véhicules légers ([`codes::LIGHT_VEHICLE_ACCESS`](super::codes::LIGHT_VEHICLE_ACCESS)).
    pub access: u8,
    /// Tronçon fictif (raccord sans existence physique).
    pub fictive: bool,
    /// En agglomération.
    pub urban: bool,
    /// Vitesse moyenne des véhicules légers (km/h).
    pub speed: u8,
    /// Restriction de poids total (dt, 0 : aucune).
    pub weight_dt: u32,
    /// Restriction de hauteur (dm, 0 : aucune).
    pub height_dm: u32,
    /// Restriction de largeur (dm, 0 : aucune).
    pub width_limit_dm: u32,
    /// Points (m) ; z en m hors du sol.
    pub points: Vec<[f32; 3]>,
}

/// Morceau d'un tronçon de voie ferrée.
#[derive(Clone, Debug, PartialEq)]
pub struct Railway {
    /// Identifiant du tronçon.
    pub id: u64,
    /// Rang du morceau.
    pub rank: u32,
    /// Nature (code de la BD TOPO).
    pub nature: u8,
    /// Position par rapport au sol.
    pub level: i8,
    /// Électrifiée.
    pub electrified: bool,
    /// Nombre de voies.
    pub tracks: u8,
    /// Points (m).
    pub points: Vec<[f32; 3]>,
}

/// Routes et voies ferrées d'une tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    /// Routes, triées par (identifiant, rang).
    pub roads: Vec<Road>,
    /// Voies ferrées, triées par (identifiant, rang).
    pub railways: Vec<Railway>,
}

/// Section [`SectionId::Network`](super::SectionId::Network).
pub fn network(b: &[u8]) -> Result<Network> {
    let mut r = Reader::new(b);
    let mut net = Network::default();
    let n = r.count()?;
    let mut id = 0i64;
    for _ in 0..n {
        id = id.wrapping_add(r.svarint()?);
        let rank = small(r.varint()?)?;
        let nature = r.u8()?;
        let importance = r.u8()?;
        let level = r.u8()? as i8;
        let width_dm = small(r.varint()?)?;
        let lanes = r.u8()?;
        let direction = r.u8()?;
        let a = r.u8()?;
        let speed = r.u8()?;
        let weight_dt = small(r.varint()?)?;
        let height_dm = small(r.varint()?)?;
        let width_limit_dm = small(r.varint()?)?;
        let points = line(&mut r, level != 0)?;
        net.roads.push(Road {
            id: id as u64,
            rank,
            nature,
            importance,
            level,
            width_dm,
            lanes,
            direction,
            access: a & 0x3F,
            fictive: a & 0x40 != 0,
            urban: a & 0x80 != 0,
            speed,
            weight_dt,
            height_dm,
            width_limit_dm,
            points,
        });
    }
    let n = r.count()?;
    let mut id = 0i64;
    for _ in 0..n {
        id = id.wrapping_add(r.svarint()?);
        let rank = small(r.varint()?)?;
        let nature = r.u8()?;
        let level = r.u8()? as i8;
        let electrified = r.u8()? != 0;
        let tracks = r.u8()?;
        let points = line(&mut r, level != 0)?;
        net.railways.push(Railway { id: id as u64, rank, nature, level, electrified, tracks, points });
    }
    r.finish()?;
    Ok(net)
}

// ---------------------------------------------------------------- Géométries

/// Géométrie d'un pont ou d'un lieu.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    /// Aucune.
    Empty,
    /// Point (m).
    Point([f32; 2]),
    /// Lignes (m), au sol.
    Lines(Vec<Vec<[f32; 3]>>),
    /// Polygones.
    Polygons(Vec<Polygon>),
}

fn geometry(r: &mut Reader) -> Result<Geometry> {
    Ok(match r.u8()? {
        0 => Geometry::Empty,
        1 => Geometry::Point([coord(r.svarint()?), coord(r.svarint()?)]),
        2 => {
            let n = r.count()?;
            Geometry::Lines((0..n).map(|_| line(r, false)).collect::<Result<_>>()?)
        }
        3 => {
            let n = r.count()?;
            Geometry::Polygons((0..n).map(|_| polygon(r)).collect::<Result<_>>()?)
        }
        _ => return Err(Invalid),
    })
}

// --------------------------------------------------------------------- Ponts

/// Un pont : un tablier de la BD TOPO (B-03).
#[derive(Clone, Debug, PartialEq)]
pub struct Bridge {
    /// Identifiant de l'ouvrage.
    pub id: u64,
    /// Longueur (dm).
    pub length_dm: u32,
    /// Largeur (dm).
    pub width_dm: u32,
    /// Importance de la route portée.
    pub importance: u8,
    /// Nature de la route portée.
    pub road_nature: u8,
    /// Charge admise (dt).
    pub weight_dt: u32,
    /// Ce qu'il franchit (code de la chaîne).
    pub crosses: u8,
    /// Nombre de tronçons de route portés.
    pub roads: u32,
    /// Nombre de tronçons de voie ferrée portés.
    pub railways: u32,
    /// Toponyme.
    pub name: String,
    /// Tronçons portés (identifiants croissants).
    pub carried: Vec<u64>,
    /// Emprise.
    pub geometry: Geometry,
}

/// Section [`SectionId::Bridges`](super::SectionId::Bridges).
pub fn bridges(b: &[u8]) -> Result<Vec<Bridge>> {
    let mut r = Reader::new(b);
    let n = r.count()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.varint()?;
        let length_dm = small(r.varint()?)?;
        let width_dm = small(r.varint()?)?;
        let importance = r.u8()?;
        let road_nature = r.u8()?;
        let weight_dt = small(r.varint()?)?;
        let crosses = r.u8()?;
        let roads = small(r.varint()?)?;
        let railways = small(r.varint()?)?;
        let name = r.string()?;
        let nc = r.count()?;
        let mut prev = 0u64;
        let carried = (0..nc)
            .map(|_| {
                prev = prev.wrapping_add(r.varint()?);
                Ok(prev)
            })
            .collect::<Result<_>>()?;
        let geometry = geometry(&mut r)?;
        out.push(Bridge {
            id,
            length_dm,
            width_dm,
            importance,
            road_nature,
            weight_dt,
            crosses,
            roads,
            railways,
            name,
            carried,
            geometry,
        });
    }
    r.finish()?;
    Ok(out)
}

// ---------------------------------------------------------------------- Zone

/// Zone accessible d'une tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// Cases exclues (cours fermées, enclaves), si la tile en a.
    pub excluded: Option<Bitmap>,
    /// Cases percées pour les passages, indices `j * 500 + i` croissants.
    pub passages: Vec<u32>,
}

/// Section [`SectionId::Zone`](super::SectionId::Zone).
pub fn zone(b: &[u8]) -> Result<Zone> {
    let mut r = Reader::new(b);
    let excluded = match r.u8()? {
        0 => None,
        1 => Some(Bitmap::decode(r.slice(BITMAP_LEN)?)?),
        _ => return Err(Invalid),
    };
    let n = r.count()?;
    let mut k = 0u64;
    let passages = (0..n)
        .map(|_| {
            k = k.checked_add(r.varint()?).ok_or(Invalid)?;
            if k >= CELLS as u64 { Err(Invalid) } else { Ok(k as u32) }
        })
        .collect::<Result<_>>()?;
    r.finish()?;
    Ok(Zone { excluded, passages })
}

/// Section [`SectionId::Passable`](super::SectionId::Passable).
pub fn passable(b: &[u8]) -> Result<Bitmap> {
    Bitmap::decode(b)
}

// ---------------------------------------------------------------- Occupation

/// Occupation du sol à 2 m : un octet par case, quartet bas = végétation
/// ([`codes::VEGETATION`](super::codes::VEGETATION)) ou étranger (15), bits
/// hauts = eau, estran, mer, eau salée.
pub fn occupation(b: &[u8]) -> Result<&[u8]> {
    if b.len() == CELLS { Ok(b) } else { Err(Invalid) }
}

// ------------------------------------------------------------------ Communes

/// Fragment de commune dans la tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    /// Code INSEE numérique ([`codes::commune_code`](super::codes::commune_code)).
    pub code: u32,
    /// Cases de 2 m.
    pub cells: u32,
    /// Habitants initiaux.
    pub inhabitants: u32,
}

/// Communes d'une tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Communes {
    /// Fragments, par code croissant.
    pub fragments: Vec<Fragment>,
    /// Carte à 2 m : indice du fragment par case, 255 = mer ; seulement si la
    /// tile a plusieurs fragments.
    pub map: Option<Vec<u8>>,
    /// Cases hors de la commune (mer, étranger) d'une tile à un seul fragment.
    pub outside: Option<Bitmap>,
}

/// Section [`SectionId::Communes`](super::SectionId::Communes).
pub fn communes(b: &[u8]) -> Result<Communes> {
    let mut r = Reader::new(b);
    let n = r.count()?;
    let fragments = (0..n)
        .map(|_| {
            Ok(Fragment { code: small(r.varint()?)?, cells: small(r.varint()?)?, inhabitants: small(r.varint()?)? })
        })
        .collect::<Result<Vec<_>>>()?;
    let rest = r.rest();
    let (map, outside) = match (n, rest.len()) {
        (_, 0) => (None, None),
        (2.., CELLS) if rest.iter().all(|&c| c == 255 || (c as usize) < n) => (Some(rest.to_vec()), None),
        (1, BITMAP_LEN) => (None, Some(Bitmap::decode(rest)?)),
        _ => return Err(Invalid),
    };
    Ok(Communes { fragments, map, outside })
}

// --------------------------------------------------------------------- Lieux

/// Lieu d'intérêt (mairie, port, gare… : [`codes::PLACE_KINDS`](super::codes::PLACE_KINDS)).
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// Type.
    pub kind: u8,
    /// Identifiant de l'objet de la BD TOPO.
    pub id: u64,
    /// Nom.
    pub name: String,
    /// Précision (nature détaillée, code OACI…).
    pub detail: String,
    /// Emprise.
    pub geometry: Geometry,
}

/// Section [`SectionId::Places`](super::SectionId::Places).
pub fn places(b: &[u8]) -> Result<Vec<Place>> {
    let mut r = Reader::new(b);
    let n = r.count()?;
    let out = (0..n)
        .map(|_| {
            Ok(Place {
                kind: r.u8()?,
                id: r.varint()?,
                name: r.string()?,
                detail: r.string()?,
                geometry: geometry(&mut r)?,
            })
        })
        .collect::<Result<_>>()?;
    r.finish()?;
    Ok(out)
}
