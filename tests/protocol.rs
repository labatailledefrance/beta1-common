//! Tests du protocole et du codec.

use beta1_common::codec::{Reader, Writer};
use beta1_common::geo::{Pos, TileId};
use beta1_common::protocol::{
    BUDGET_ZOMBIES, ClientMessage, FINGERPRINT, RAYON_MAX, RejectReason, ServerMessage, ZombieVu, handshake,
};

/// Brest, position exacte au 1/64 m.
const BREST: Pos = Pos { tile: TileId { ix: 146, iy: 6839 }, local: [12.5, -250.25] };

fn zombies(n: usize) -> ServerMessage {
    let zombies = (0..n).map(|k| ZombieVu { d: [k as i16 * 7 - 3000, -(k as i16)], cap: k as u8 }).collect();
    ServerMessage::Zombies { tick: 99, ancre: BREST.tile, total: n as u32 + 5, zombies }
}

#[test]
fn aller_retour_des_messages() {
    for c in [ClientMessage::Hello { fingerprint: FINGERPRINT }, ClientMessage::Vue { centre: BREST, rayon: 800.0 }] {
        assert_eq!(ClientMessage::decode(&c.encode()), Ok(c));
    }
    for s in [
        ServerMessage::Welcome { tick: 42, camera: BREST },
        ServerMessage::Rejected { reason: RejectReason::Version },
        ServerMessage::State { tick: u64::MAX },
        zombies(0),
        zombies(BUDGET_ZOMBIES),
    ] {
        assert_eq!(ServerMessage::decode(&s.encode()), Ok(s));
    }
}

#[test]
fn rayon_de_vue_borne() {
    // L'aide borne ; le décodeur refuse ce qui sort des bornes.
    assert_eq!(ClientMessage::vue(BREST, 1e9), ClientMessage::Vue { centre: BREST, rayon: RAYON_MAX });
    assert_eq!(ClientMessage::vue(BREST, -3.0), ClientMessage::Vue { centre: BREST, rayon: 0.0 });
    assert_eq!(ClientMessage::vue(BREST, f32::NAN), ClientMessage::Vue { centre: BREST, rayon: RAYON_MAX });
    for rayon in [1e9, -3.0, -0.0] {
        assert!(ClientMessage::decode(&ClientMessage::Vue { centre: BREST, rayon }.encode()).is_err(), "{rayon}");
    }
}

/// Trouvé par le fuzzing : une vue de rayon négatif ne doit pas se décoder (elle se réencodait autrement).
#[test]
fn regression_fuzzing_rayon_negatif() {
    // Base64 : AgAAAAAAAAAAAAAAAAC12P0=
    let b = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xB5, 0xD8, 0xFD];
    assert!(ClientMessage::decode(&b).is_err());
}

#[test]
fn position_hors_de_la_tile_refusee() {
    // Locale de 31 999 / 64 = 499,98 m : acceptée ; 32 000 / 64 = 500 m : refusée.
    let mut w = Writer::default();
    w.tile(BREST.tile);
    w.i16(31_999);
    w.i16(-32_000);
    assert!(Reader::new(&w.buf).pos().is_ok());
    let mut w = Writer::default();
    w.tile(BREST.tile);
    w.i16(32_000);
    w.i16(0);
    assert!(Reader::new(&w.buf).pos().is_err());
}

#[test]
fn zombies_au_dela_du_budget_refuses() {
    assert!(ServerMessage::decode(&zombies(BUDGET_ZOMBIES + 1).encode()).is_err());
    // 5 octets par zombie, en-tête compris : le budget tient en 6 ko.
    assert!(zombies(BUDGET_ZOMBIES).encode().len() <= 6_030);
}

#[test]
fn zombie_vu_au_dixieme_de_metre() {
    let p = BREST.offset([-731.23, 412.06]);
    let z = ZombieVu::new(BREST.tile, p, -std::f32::consts::FRAC_PI_2).unwrap();
    assert!(z.pos(BREST.tile).distance(p) < 0.08, "{:?}", z.pos(BREST.tile));
    assert_eq!(z.cap, 192);
    assert!((z.cap() - 3.0 * std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    // Au-delà de 3,2 km de l'ancre : pas représentable.
    assert_eq!(ZombieVu::new(BREST.tile, BREST.offset([4000.0, 0.0]), 0.0), None);
}

#[test]
fn handshake_selon_le_fingerprint() {
    let bon = ClientMessage::Hello { fingerprint: FINGERPRINT }.encode();
    assert_eq!(handshake(&bon, 7, BREST), ServerMessage::Welcome { tick: 7, camera: BREST });
    let mut autre = FINGERPRINT;
    autre[0] ^= 1;
    let mauvais = ClientMessage::Hello { fingerprint: autre }.encode();
    assert_eq!(handshake(&mauvais, 7, BREST), ServerMessage::Rejected { reason: RejectReason::Version });
    assert_eq!(handshake(&[0xFF], 7, BREST), ServerMessage::Rejected { reason: RejectReason::Invalid });
    // Une vue avant le Hello : refusée.
    let vue = ClientMessage::Vue { centre: BREST, rayon: 1.0 }.encode();
    assert_eq!(handshake(&vue, 7, BREST), ServerMessage::Rejected { reason: RejectReason::Invalid });
}

#[test]
fn toute_troncature_est_refusee_sans_paniquer() {
    for b in [ServerMessage::Welcome { tick: 1 << 40, camera: BREST }.encode(), zombies(3).encode()] {
        for n in 0..b.len() {
            assert!(ServerMessage::decode(&b[..n]).is_err());
        }
        let mut long = b.clone();
        long.push(0);
        assert!(ServerMessage::decode(&long).is_err());
    }
    let b = ClientMessage::Vue { centre: BREST, rayon: 3.0 }.encode();
    for n in 0..b.len() {
        assert!(ClientMessage::decode(&b[..n]).is_err());
    }
}

#[test]
fn troncature_du_welcome() {
    let b = ServerMessage::Welcome { tick: 1 << 40, camera: BREST }.encode();
    for n in 0..b.len() {
        assert!(ServerMessage::decode(&b[..n]).is_err());
    }
    let mut long = b.clone();
    long.push(0);
    assert!(ServerMessage::decode(&long).is_err());
}

#[test]
fn varints_et_longueurs_hostiles() {
    // 11 octets de continuation : refusé.
    assert!(Reader::new(&[0xFF; 11]).varint().is_err());
    // Longueur annoncée plus grande que le message : refusé sans allouer.
    let mut w = Writer::default();
    w.varint(u64::MAX);
    assert!(Reader::new(&w.buf).string().is_err());
    // Flottant non fini : refusé.
    assert!(Reader::new(&f32::NAN.to_le_bytes()).f32().is_err());
}

#[test]
fn positions_au_soixante_quatrieme_de_metre() {
    let p = Pos::new(TileId::from_zone(0, 652, 6862).map_or(0, |t| t.ix), 6862, 499.99, -500.0);
    let mut w = Writer::default();
    w.pos(p);
    let q = Reader::new(&w.buf).pos().unwrap();
    assert!(p.distance(q) < 0.02, "{p:?} {q:?}");
    let loin = Pos::new(0, 0, 1499.0, -1500.2);
    assert_eq!(loin.tile, TileId { ix: 1, iy: -2 });
    assert!((-500.0..500.0).contains(&loin.local[0]) && (-500.0..500.0).contains(&loin.local[1]));
}
