//! Format des tiles : allers-retours, entrées tronquées ou falsifiées refusées.

use beta1_common::codec::Writer;
use beta1_common::tile::relief::{self, NONE, Nodes};
use beta1_common::tile::sections::{self, BITMAP_LEN, CELLS, Geometry};
use beta1_common::tile::{self, Header, Section, SectionId};

/// Compression de test : un bloc zstd « brut » (non compressé), écrit à la
/// main pour ne pas dépendre de la bibliothèque zstd.
fn zstd_brut(d: &[u8]) -> Vec<u8> {
    let mut o = vec![0x28, 0xB5, 0x2F, 0xFD];
    // Descripteur : taille du contenu sur 4 octets, fenêtre unique.
    o.push(0b1010_0000);
    o.extend_from_slice(&(d.len() as u32).to_le_bytes());
    let mut morceaux = d.chunks(128 * 1024).peekable();
    if morceaux.peek().is_none() {
        o.extend_from_slice(&[1, 0, 0]);
    }
    while let Some(c) = morceaux.next() {
        let dernier = morceaux.peek().is_none() as u32;
        let h = dernier | (c.len() as u32) << 3;
        o.extend_from_slice(&h.to_le_bytes()[..3]);
        o.extend_from_slice(c);
    }
    o
}

fn section(id: SectionId, brut: &[u8]) -> Section {
    Section { id, raw_len: brut.len(), data: zstd_brut(brut) }
}

fn relief_test() -> Nodes {
    let mut g = Nodes::new(1);
    for j in 0..g.side {
        for i in 0..g.side {
            // Pente, colline et une zone de mer.
            let d = ((i as f64 - 400.0).powi(2) + (j as f64 - 600.0).powi(2)).sqrt();
            g.cm[j * g.side + i] =
                if i + j < 150 { NONE } else { (5000.0 + 3.0 * i as f64 + 2000.0 * (-d / 120.0).exp()) as i32 };
        }
    }
    g
}

#[test]
fn relief_aller_retour() {
    let g = relief_test();
    let (base, fin) = relief::encode_two(&g, 10);
    let b = relief::decode(&base, 2).unwrap();
    let f = relief::decode_two(&base, &fin).unwrap();
    for j in 0..g.side {
        for i in 0..g.side {
            let (v, w) = (g.get(i, j), f.get(i, j));
            assert_eq!(v == NONE, w == NONE, "({i}, {j})");
            if v != NONE {
                assert!((v - w).abs() <= 5, "({i}, {j}) : {v} contre {w}");
            }
            if i % 2 == 0 && j % 2 == 0 {
                assert_eq!(w, b.get(i / 2, j / 2));
            }
        }
    }
    // Tronqué, ou avec un octet en trop : refusé.
    assert!(relief::decode(&base[..base.len() - 1], 2).is_err());
    let mut plus = base.clone();
    plus.push(0);
    assert!(relief::decode(&plus, 2).is_err());
    assert!(relief::decode_two(&base, &fin[..fin.len() - 1]).is_err());
}

#[test]
fn relief_vide() {
    let g = Nodes::new(2);
    let b = relief::encode(&g, 10);
    assert_eq!(relief::decode(&b, 2).unwrap(), g);
}

#[test]
fn tile_aller_retour_et_falsification() {
    let mut w = Writer::default();
    w.varint(1); // un bâtiment
    w.varint(300_445_226);
    w.bytes(&[0, 0, 255, 1 | 2 | 64]); // nature, usages, léger + hauteur + porte
    w.varint(95); // hauteur 9,5 m
    w.varint(0); // porte : anneau 0
    w.varint(2); // segment 2
    w.varint(1); // un polygone
    w.varint(1); // un anneau
    w.varint(3);
    for (x, y) in [(-16, -16), (32, 0), (-16, 48)] {
        w.svarint(x);
        w.svarint(y);
    }
    let bati = w.buf;
    let occ = vec![3u8; CELLS];
    let h =
        Header { zone: 0, ix: 146, iy: 6838, flags: 1 | 4 | 2 << 4, built_share: 1234, inhabitants: 42, buildings: 1 };
    let t = tile::encode(&h, &[section(SectionId::Buildings, &bati), section(SectionId::Occupation, &occ)]);

    // En-tête seul : ses premiers octets suffisent.
    assert_eq!(tile::decode_header(&t[..tile::HEADER]).unwrap(), h);
    assert!(tile::decode_header(&t[..tile::HEADER - 1]).is_err());
    let l = tile::decode(&t, true).unwrap();
    assert_eq!(l.header, h);
    assert_eq!(l.header.tile().map(|t| (t.ix, t.iy)), Some((146, 6838)));
    assert_eq!(l.header.relief_source(), tile::ReliefSource::RgeAlti1m);
    let b = sections::buildings(&l.section(SectionId::Buildings).unwrap().unwrap()).unwrap();
    assert_eq!(b.len(), 1);
    assert_eq!(
        (b[0].id, b[0].light, b[0].height_dm, b[0].door, b[0].usage2),
        (300_445_226, true, Some(95), Some((0, 2)), 255)
    );
    assert_eq!(b[0].polygons, vec![vec![vec![[-1.0, -1.0], [1.0, -1.0], [0.0, 2.0]]]]);
    assert_eq!(sections::occupation(&l.section(SectionId::Occupation).unwrap().unwrap()).unwrap().len(), CELLS);
    assert!(l.section(SectionId::Relief).unwrap().is_none());

    // Un octet changé : empreinte fausse.
    let mut f = t.clone();
    f[30] ^= 1;
    assert!(tile::decode(&f, true).is_err());
    // Tronquée : refusée même sans vérifier l'empreinte.
    assert!(tile::decode(&t[..t.len() - 40], false).is_err());
    // Taille annoncée fausse : la section ne se décompresse pas.
    let mut s = section(SectionId::Buildings, &bati);
    s.raw_len += 1;
    let t2 = tile::encode(&h, &[s]);
    assert!(tile::decode(&t2, true).unwrap().section(SectionId::Buildings).is_err());
}

#[test]
fn bombe_zstd_refusee() {
    // Annonce 1 octet, en contient 1 Mo.
    let gros = vec![0u8; 1 << 20];
    let s = Section { id: SectionId::Occupation, raw_len: 1, data: zstd_brut(&gros) };
    let t = tile::encode(&Header::default(), &[s]);
    assert!(tile::decode(&t, true).unwrap().section(SectionId::Occupation).is_err());
    // Taille annoncée au-delà de la borne : refusée dès l'en-tête.
    let s = Section { id: SectionId::Occupation, raw_len: tile::MAX_SECTION + 1, data: vec![] };
    assert!(tile::decode(&tile::encode(&Header::default(), &[s]), true).is_err());
}

#[test]
fn paquet() {
    let a = tile::encode(&Header { ix: 144, iy: 6836, ..Header::default() }, &[]);
    let b = tile::encode(&Header { ix: 145, iy: 6836, ..Header::default() }, &[]);
    let p = tile::encode_pack(&[(144, 6836, &a), (145, 6836, &b)]);
    let idx = tile::decode_pack_index(&p).unwrap();
    assert_eq!(idx.len(), 2);
    let e = idx[1];
    let t = &p[e.offset as usize..e.offset as usize + e.len as usize];
    assert_eq!(tile::decode(t, true).unwrap().header.ix, 145);
    assert_eq!(tile::pack_of(144, 6836), (9, 427));
    assert!(tile::decode_pack_index(&p[..20]).is_err());
}

#[test]
fn sections_diverses() {
    // Réseau : une route au sol, une voie ferrée en viaduc.
    let mut w = Writer::default();
    w.varint(1);
    w.svarint(77);
    w.varint(0);
    w.bytes(&[2, 3, 0]);
    w.varint(60);
    w.bytes(&[2, 0, 0x40, 80]);
    w.varint(0);
    w.varint(0);
    w.varint(0);
    w.varint(2);
    for v in [0, 0, 160, 0] {
        w.svarint(v);
    }
    w.varint(1);
    w.svarint(9);
    w.varint(1);
    w.bytes(&[0, 1, 1, 2]);
    w.varint(1);
    for v in [16, 16, 55] {
        w.svarint(v);
    }
    let n = sections::network(&w.buf).unwrap();
    assert_eq!((n.roads[0].id, n.roads[0].fictive, n.roads[0].points[1]), (77, true, [10.0, 0.0, 0.0]));
    assert_eq!((n.railways[0].level, n.railways[0].points[0]), (1, [1.0, 1.0, 5.5]));

    // Communes : un fragment, cases hors commune.
    let mut w = Writer::default();
    w.varint(1);
    w.varint(29019);
    w.varint(240_000);
    w.varint(1200);
    w.bytes(&vec![0u8; BITMAP_LEN]);
    let c = sections::communes(&w.buf).unwrap();
    assert_eq!(c.fragments[0].code, 29019);
    assert_eq!(c.outside.map(|b| b.count()), Some(0));
    assert!(sections::communes(&w.buf[..w.buf.len() - 1]).is_err());

    // Lieux.
    let mut w = Writer::default();
    w.varint(1);
    w.u8(0);
    w.varint(12);
    w.string("Mairie de Brest");
    w.string("");
    w.u8(1);
    w.svarint(-32);
    w.svarint(8);
    let p = sections::places(&w.buf).unwrap();
    assert_eq!((p[0].name.as_str(), &p[0].geometry), ("Mairie de Brest", &Geometry::Point([-2.0, 0.5])));

    // Zone : passages hors de la grille refusés.
    let mut w = Writer::default();
    w.u8(0);
    w.varint(1);
    w.varint(CELLS as u64);
    assert!(sections::zone(&w.buf).is_err());

    assert_eq!(tile::codes::commune_insee(tile::codes::commune_code("2B033")), "2B033");
    assert_eq!(tile::codes::commune_insee(tile::codes::commune_code("01004")), "01004");
}
