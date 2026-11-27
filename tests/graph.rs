//! Graphe routier : aller-retour, fichiers tronqués, falsifiés ou incohérents refusés.

use beta1_common::graph::{RoadAttr, RoadGraph, Structure};

/// Un bloc zstd « brut » (non compressé), comme dans `tests/tile.rs`.
fn zstd_brut(d: &[u8]) -> Vec<u8> {
    let mut o = vec![0x28, 0xB5, 0x2F, 0xFD, 0b1010_0000];
    o.extend_from_slice(&(d.len() as u32).to_le_bytes());
    let mut morceaux = d.chunks(128 * 1024).peekable();
    if morceaux.peek().is_none() {
        o.extend_from_slice(&[1, 0, 0]);
    }
    while let Some(c) = morceaux.next() {
        let h = morceaux.peek().is_none() as u32 | (c.len() as u32) << 3;
        o.extend_from_slice(&h.to_le_bytes()[..3]);
        o.extend_from_slice(c);
    }
    o
}

/// Trois nœuds, deux arêtes, dont une sur un pont ; zone 1 (x au-delà de 2 048 km).
fn graphe() -> RoadGraph {
    let x0 = 2_100_000 * 16;
    RoadGraph {
        nodes: vec![[x0, 16_000], [x0 + 1600, 16_000], [x0 - 800, 32_000]],
        u: vec![0, 1],
        v: vec![1, 2],
        length_mm: vec![100_000, 180_000],
        ids: vec![42, u64::MAX - 1],
        first_rank: vec![0, 3],
        attrs: vec![
            RoadAttr { nature: 2, importance: 4, speed: 50, width_dm: 60, ..Default::default() },
            RoadAttr {
                nature: 0,
                importance: 1,
                level: 1,
                lanes: 2,
                urban: true,
                weight_dt: 190,
                ..Default::default()
            },
        ],
        structure_of: vec![None, Some(0)],
        structures: vec![Structure { bridge: Some(7), load_t: 44, width_dm: 90 }],
        geometry_start: vec![0, 2, 5],
        points: vec![
            [x0 as i32, 16_000],
            [x0 as i32 + 1600, 16_000],
            [x0 as i32 + 1600, 16_000],
            [x0 as i32, 24_000],
            [x0 as i32 - 800, 32_000],
        ],
        order: vec![2, 0, 1],
    }
}

/// Recalcule l'empreinte après une falsification.
fn resceller(b: &mut Vec<u8>) {
    b.truncate(b.len() - 32);
    let h = blake3::hash(b);
    b.extend_from_slice(h.as_bytes());
}

#[test]
fn aller_retour() {
    let g = graphe();
    let b = g.encode(zstd_brut).unwrap();
    let d = RoadGraph::decode(&b).unwrap();
    assert_eq!(d, g);
    assert_eq!(d.edge_points(1).len(), 3);
    assert_eq!(d.edge_points(9), &[] as &[[i32; 2]]);
    // Graphe vide : valide aussi.
    let v = RoadGraph { geometry_start: vec![0], ..Default::default() };
    assert_eq!(RoadGraph::decode(&v.encode(zstd_brut).unwrap()).unwrap(), v);
}

#[test]
fn tronque_ou_falsifie() {
    let b = graphe().encode(zstd_brut).unwrap();
    for n in [0, 3, 4, 40, b.len() / 2, b.len() - 1] {
        assert!(RoadGraph::decode(&b[..n]).is_err(), "tronqué à {n}");
    }
    let mut f = b.clone();
    f[20] ^= 1;
    assert!(RoadGraph::decode(&f).is_err(), "empreinte");
}

#[test]
fn incoherent() {
    let mut g = graphe();
    g.order = vec![0, 0, 1];
    assert!(g.encode(zstd_brut).is_err(), "ordre qui n'est pas une permutation");
    let mut g = graphe();
    g.v[1] = 3;
    assert!(g.encode(zstd_brut).is_err(), "nœud hors du graphe");
    let mut g = graphe();
    g.structure_of[0] = Some(1);
    assert!(g.encode(zstd_brut).is_err(), "ouvrage hors de la table");
    let mut g = graphe();
    g.geometry_start = vec![0, 6, 5];
    assert!(g.encode(zstd_brut).is_err(), "géométrie décroissante");
    let mut g = graphe();
    g.nodes[0][0] = 1 << 40;
    assert!(g.encode(zstd_brut).is_err(), "coordonnée hors de 32 bits");

    // Même contrôle au décodage : un ordre faux, empreinte recalculée.
    let b = graphe().encode(zstd_brut).unwrap();
    let ordre = [2u32, 0, 1].iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let p = b.windows(12).rposition(|w| w == ordre.as_slice()).unwrap();
    let mut f = b.clone();
    f[p..p + 4].copy_from_slice(&1u32.to_le_bytes());
    resceller(&mut f);
    assert!(RoadGraph::decode(&f).is_err(), "ordre falsifié");
    // Taille brute annoncée énorme : refusée sans allouer.
    let mut f = b.clone();
    f[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    resceller(&mut f);
    assert!(RoadGraph::decode(&f).is_err(), "taille annoncée");
}
