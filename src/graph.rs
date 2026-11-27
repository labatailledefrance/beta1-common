//! Graphe routier national (B-03, A-07), produit par `beta1-datachain` et lu
//! par le serveur.
//!
//! La chaîne recoud les morceaux de route des tiles (coupés aux bords) en
//! tronçons, fait un nœud de chaque extrémité, associe les ponts, puis calcule
//! l'**ordre CCH** (dissection imbriquée), le plus long à obtenir : le serveur
//! n'a plus qu'à construire la hiérarchie et à la personnaliser par classe
//! d'itinéraire. Analogie : la chaîne livre une table de routage déjà agrégée,
//! le routeur n'applique que ses métriques.
//!
//! Format (repris de P-26, `P26G`) : signature, onze sections, puis l'empreinte
//! BLAKE3 de tout ce qui précède. Chaque section est un tableau d'entiers
//! petit-boutiens, précédé de sa taille brute et de sa taille compressée (u64),
//! compressé en zstd. Ordre des sections : nœuds, `u`, `v`, longueurs,
//! identifiants, rangs, attributs, ouvrage de chaque arête, ouvrages, début de
//! la géométrie de chaque arête, points (différences dans l'arête), ordre.
//!
//! Le décodage ne panique jamais et vérifie tout ce dont le serveur dépend
//! (indices dans les bornes, ordre qui est une permutation) : un fichier faux
//! rend [`Invalid`], jamais un graphe incohérent.

use std::io::Read;

use crate::codec::{Invalid, Result};

/// Signature du fichier.
pub const MAGIC: &[u8; 4] = b"P26G";
/// Quantification des coordonnées : 1/16 m, comme les tiles.
pub const QUANT: i64 = 16;
const HASH: usize = 32;
/// Taille brute maximale d'une section (8 Gio : la France entière en demande
/// environ 2 pour ses points) : borne contre les bombes zstd.
pub const MAX_SECTION: usize = 8 << 30;
/// Pas de lien vers un ouvrage.
const NONE: u32 = u32::MAX;

/// Attributs d'un tronçon de la BD TOPO, tels que les tiles les portent
/// ([`Road`](crate::tile::sections::Road)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RoadAttr {
    /// Nature ([`codes::ROAD_NATURES`](crate::tile::codes::ROAD_NATURES)).
    pub nature: u8,
    /// Importance (1 à 6).
    pub importance: u8,
    /// Position par rapport au sol (négatif : souterrain).
    pub level: i8,
    /// Nombre de voies.
    pub lanes: u8,
    /// Sens ([`codes::DIRECTIONS`](crate::tile::codes::DIRECTIONS)).
    pub direction: u8,
    /// Accès des véhicules légers.
    pub access: u8,
    /// En agglomération.
    pub urban: bool,
    /// Vitesse moyenne des véhicules légers (km/h, 0 : inconnue).
    pub speed: u8,
    /// Largeur de chaussée (dm).
    pub width_dm: u16,
    /// Restriction de poids total (dt, 0 : aucune).
    pub weight_dt: u16,
    /// Restriction de hauteur (dm, 0 : aucune).
    pub height_dm: u16,
    /// Restriction de largeur (dm, 0 : aucune).
    pub width_limit_dm: u16,
}

/// Ouvrage porté par une arête : pont de la BD TOPO, ou ouvrage implicite
/// (tronçon au-dessus du sol sans pont associé).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Structure {
    /// Identifiant BD TOPO du pont.
    pub bridge: Option<u64>,
    /// Charge limite (t), heuristique de la chaîne (B-03).
    pub load_t: u16,
    /// Largeur (dm).
    pub width_dm: u16,
}

/// Graphe routier : nœuds, arêtes (une par tronçon recousu, sens de
/// numérisation `u` → `v`), géométrie et ordre CCH.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoadGraph {
    /// Nœuds : coordonnées globales au 1/16 m (x porte la zone de projection,
    /// comme [`TileId::ix`](crate::geo::TileId)), triés par (y, x).
    pub nodes: Vec<[i64; 2]>,
    /// Extrémité de départ de chaque arête.
    pub u: Vec<u32>,
    /// Extrémité d'arrivée.
    pub v: Vec<u32>,
    /// Longueur (mm).
    pub length_mm: Vec<u32>,
    /// Identifiant du tronçon de la BD TOPO.
    pub ids: Vec<u64>,
    /// Rang du premier morceau du tronçon dans l'arête.
    pub first_rank: Vec<u16>,
    /// Attributs.
    pub attrs: Vec<RoadAttr>,
    /// Ouvrage de chaque arête (indice dans `structures`).
    pub structure_of: Vec<Option<u32>>,
    /// Ouvrages.
    pub structures: Vec<Structure>,
    /// Géométrie : points de l'arête `e` en `geometry_start[e]..geometry_start[e + 1]`.
    pub geometry_start: Vec<u32>,
    /// Points (1/16 m, globaux, sans altitude).
    pub points: Vec<[i32; 2]>,
    /// Ordre CCH : rang de contraction de chaque nœud (permutation de `0..n`).
    pub order: Vec<u32>,
}

impl RoadGraph {
    /// Nombre de nœuds.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    /// Nombre d'arêtes.
    pub fn edge_count(&self) -> usize {
        self.u.len()
    }
    /// Points de l'arête `e`.
    pub fn edge_points(&self, e: usize) -> &[[i32; 2]] {
        match (self.geometry_start.get(e), self.geometry_start.get(e + 1)) {
            (Some(&a), Some(&b)) => self.points.get(a as usize..b as usize).unwrap_or(&[]),
            _ => &[],
        }
    }

    /// Tailles cohérentes, indices dans les bornes, ordre qui est une permutation.
    fn valid(&self) -> bool {
        let (n, m) = (self.node_count(), self.edge_count());
        let tailles = [self.v.len(), self.length_mm.len(), self.ids.len(), self.first_rank.len(), self.attrs.len()];
        tailles.iter().all(|&l| l == m)
            && self.structure_of.len() == m
            && self.geometry_start.len() == m + 1
            && self.order.len() == n
            && n <= NONE as usize
            && self.u.iter().chain(&self.v).all(|&x| (x as usize) < n)
            && self.structure_of.iter().flatten().all(|&s| (s as usize) < self.structures.len())
            && self.geometry_start.first() == Some(&0)
            && self.geometry_start.last().is_some_and(|&l| l as usize == self.points.len())
            && self.geometry_start.windows(2).all(|w| w[0] <= w[1])
            && permutation(&self.order)
    }

    /// Fichier du graphe. `compress` compresse une section en zstd (la chaîne
    /// utilise la bibliothèque zstd ; le format n'en dépend pas). Refuse un
    /// graphe incohérent ou des coordonnées hors de 32 bits.
    pub fn encode(&self, compress: impl Fn(&[u8]) -> Vec<u8>) -> Result<Vec<u8>> {
        if !self.valid() {
            return Err(Invalid);
        }
        let coord = |v: i64| i32::try_from(v).map(|v| v as u32).map_err(|_| Invalid);
        let mut b = MAGIC.to_vec();
        let mut sec = |raw: Vec<u8>| {
            let z = compress(&raw);
            b.extend_from_slice(&(raw.len() as u64).to_le_bytes());
            b.extend_from_slice(&(z.len() as u64).to_le_bytes());
            b.extend(z);
        };
        let nodes = self.nodes.iter().flatten().map(|&v| coord(v)).collect::<Result<Vec<u32>>>()?;
        sec(u32s(nodes.into_iter()));
        sec(u32s(self.u.iter().copied()));
        sec(u32s(self.v.iter().copied()));
        sec(u32s(self.length_mm.iter().copied()));
        sec(self.ids.iter().flat_map(|x| x.to_le_bytes()).collect());
        sec(self.first_rank.iter().flat_map(|x| x.to_le_bytes()).collect());
        sec(self.attrs.iter().flat_map(encode_attr).collect());
        sec(u32s(self.structure_of.iter().map(|o| o.unwrap_or(NONE))));
        sec(self
            .structures
            .iter()
            .flat_map(|o| {
                let mut v = o.bridge.unwrap_or(u64::MAX).to_le_bytes().to_vec();
                v.extend_from_slice(&o.load_t.to_le_bytes());
                v.extend_from_slice(&o.width_dm.to_le_bytes());
                v
            })
            .collect());
        sec(u32s(self.geometry_start.iter().copied()));
        // Différences dans l'arête : petites, elles se compriment.
        let mut d = Vec::with_capacity(self.points.len() * 2);
        for w in self.geometry_start.windows(2) {
            let mut p = [0i32; 2];
            for q in self.points.get(w[0] as usize..w[1] as usize).unwrap_or(&[]) {
                d.push(q[0].wrapping_sub(p[0]) as u32);
                d.push(q[1].wrapping_sub(p[1]) as u32);
                p = *q;
            }
        }
        sec(u32s(d.into_iter()));
        sec(u32s(self.order.iter().copied()));
        let h = blake3::hash(&b);
        b.extend_from_slice(h.as_bytes());
        Ok(b)
    }

    /// Lit un fichier de graphe : empreinte, sections, cohérence.
    pub fn decode(b: &[u8]) -> Result<RoadGraph> {
        let corps = b.len().checked_sub(HASH).ok_or(Invalid)?;
        if b.get(..4) != Some(MAGIC.as_slice()) || blake3::hash(&b[..corps]).as_bytes() != &b[corps..] {
            return Err(Invalid);
        }
        let mut r = crate::codec::Reader::new(&b[4..corps]);
        let mut sec = |taille: usize| -> Result<Vec<u8>> {
            let raw = usize::try_from(r.u64()?).map_err(|_| Invalid)?;
            let z = usize::try_from(r.u64()?).map_err(|_| Invalid)?;
            let v = decompress(r.slice(z)?, raw)?;
            if v.len() % taille == 0 { Ok(v) } else { Err(Invalid) }
        };
        let nodes =
            from_u32s(&sec(8)?).as_chunks::<2>().0.iter().map(|c| [c[0] as i32 as i64, c[1] as i32 as i64]).collect();
        let u = from_u32s(&sec(4)?);
        let v = from_u32s(&sec(4)?);
        let length_mm = from_u32s(&sec(4)?);
        let ids = sec(8)?.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect();
        let first_rank = sec(2)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let attrs = sec(16)?.as_chunks::<16>().0.iter().map(decode_attr).collect();
        let structure_of = from_u32s(&sec(4)?).into_iter().map(|o| (o != NONE).then_some(o)).collect();
        let structures = sec(12)?
            .as_chunks::<12>()
            .0
            .iter()
            .map(|c| {
                let id = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
                Structure {
                    bridge: (id != u64::MAX).then_some(id),
                    load_t: u16::from_le_bytes([c[8], c[9]]),
                    width_dm: u16::from_le_bytes([c[10], c[11]]),
                }
            })
            .collect();
        let geometry_start = from_u32s(&sec(4)?);
        let d = from_u32s(&sec(8)?);
        let order = from_u32s(&sec(4)?);
        r.finish()?;
        let mut points: Vec<[i32; 2]> = d.as_chunks::<2>().0.iter().map(|c| [c[0] as i32, c[1] as i32]).collect();
        let g = RoadGraph {
            nodes,
            u,
            v,
            length_mm,
            ids,
            first_rank,
            attrs,
            structure_of,
            structures,
            geometry_start,
            points: Vec::new(),
            order,
        };
        // Bornes de la géométrie vérifiées avant de recomposer les points.
        if g.geometry_start.last().is_none_or(|&l| l as usize != points.len())
            || !g.geometry_start.windows(2).all(|w| w[0] <= w[1])
        {
            return Err(Invalid);
        }
        for w in g.geometry_start.windows(2) {
            for k in w[0] as usize + 1..w[1] as usize {
                points[k] = [points[k][0].wrapping_add(points[k - 1][0]), points[k][1].wrapping_add(points[k - 1][1])];
            }
        }
        let g = RoadGraph { points, ..g };
        if g.valid() { Ok(g) } else { Err(Invalid) }
    }
}

fn encode_attr(a: &RoadAttr) -> [u8; 16] {
    let mut o = [0u8; 16];
    o[..8].copy_from_slice(&[
        a.nature,
        a.importance,
        a.level as u8,
        a.lanes,
        a.direction,
        a.access,
        a.urban as u8,
        a.speed,
    ]);
    for (k, x) in [a.width_dm, a.weight_dt, a.height_dm, a.width_limit_dm].into_iter().enumerate() {
        o[8 + 2 * k..10 + 2 * k].copy_from_slice(&x.to_le_bytes());
    }
    o
}

fn decode_attr(c: &[u8; 16]) -> RoadAttr {
    let w = |k: usize| u16::from_le_bytes([c[k], c[k + 1]]);
    RoadAttr {
        nature: c[0],
        importance: c[1],
        level: c[2] as i8,
        lanes: c[3],
        direction: c[4],
        access: c[5],
        urban: c[6] != 0,
        speed: c[7],
        width_dm: w(8),
        weight_dt: w(10),
        height_dm: w(12),
        width_limit_dm: w(14),
    }
}

fn u32s(v: impl Iterator<Item = u32>) -> Vec<u8> {
    v.flat_map(|x| x.to_le_bytes()).collect()
}

fn from_u32s(b: &[u8]) -> Vec<u32> {
    b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect()
}

fn permutation(o: &[u32]) -> bool {
    let mut vu = vec![false; o.len()];
    o.iter().all(|&x| vu.get_mut(x as usize).is_some_and(|v| !std::mem::replace(v, true)))
}

/// Décompresse une section de taille annoncée, sans jamais produire plus. La
/// réservation est bornée : une taille annoncée fausse ne fait pas allouer.
fn decompress(data: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    if raw_len > MAX_SECTION {
        return Err(Invalid);
    }
    let mut src = data;
    let dec = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|_| Invalid)?;
    let mut out = Vec::with_capacity(raw_len.min(64 << 20));
    dec.take(raw_len as u64 + 1).read_to_end(&mut out).map_err(|_| Invalid)?;
    if out.len() == raw_len { Ok(out) } else { Err(Invalid) }
}
