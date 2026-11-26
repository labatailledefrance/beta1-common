//! Tests du protocole et du codec.

use beta1_common::codec::{Reader, Writer};
use beta1_common::geo::{Pos, TileId};
use beta1_common::protocol::{ClientMessage, FINGERPRINT, RejectReason, ServerMessage, handshake};

#[test]
fn aller_retour_des_messages() {
    let c = ClientMessage::Hello { fingerprint: FINGERPRINT };
    assert_eq!(ClientMessage::decode(&c.encode()), Ok(c));
    for s in [
        ServerMessage::Welcome { tick: 42 },
        ServerMessage::Rejected { reason: RejectReason::Version },
        ServerMessage::State { tick: u64::MAX },
    ] {
        assert_eq!(ServerMessage::decode(&s.encode()), Ok(s));
    }
}

#[test]
fn handshake_selon_le_fingerprint() {
    let bon = ClientMessage::Hello { fingerprint: FINGERPRINT }.encode();
    assert_eq!(handshake(&bon, 7), ServerMessage::Welcome { tick: 7 });
    let mut autre = FINGERPRINT;
    autre[0] ^= 1;
    let mauvais = ClientMessage::Hello { fingerprint: autre }.encode();
    assert_eq!(handshake(&mauvais, 7), ServerMessage::Rejected { reason: RejectReason::Version });
    assert_eq!(handshake(&[0xFF], 7), ServerMessage::Rejected { reason: RejectReason::Invalid });
}

#[test]
fn toute_troncature_est_refusee_sans_paniquer() {
    let b = ServerMessage::Welcome { tick: 1 << 40 }.encode();
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
