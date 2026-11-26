//! Tests du protocole et du codec.

use beta1_common::codec::{Ecr, Lec};
use beta1_common::geo::{Pos, TileId};
use beta1_common::protocole::{DuClient, DuServeur, EMPREINTE, Raison, poignee_de_main};

#[test]
fn aller_retour_des_messages() {
    let c = DuClient::Bonjour { empreinte: EMPREINTE };
    assert_eq!(DuClient::decoder(&c.encoder()), Ok(c));
    for s in [
        DuServeur::Bienvenue { tick: 42 },
        DuServeur::Refus { raison: Raison::Version },
        DuServeur::Etat { tick: u64::MAX },
    ] {
        assert_eq!(DuServeur::decoder(&s.encoder()), Ok(s));
    }
}

#[test]
fn poignee_de_main_selon_l_empreinte() {
    let bon = DuClient::Bonjour { empreinte: EMPREINTE }.encoder();
    assert_eq!(poignee_de_main(&bon, 7), DuServeur::Bienvenue { tick: 7 });
    let mut autre = EMPREINTE;
    autre[0] ^= 1;
    let mauvais = DuClient::Bonjour { empreinte: autre }.encoder();
    assert_eq!(poignee_de_main(&mauvais, 7), DuServeur::Refus { raison: Raison::Version });
    assert_eq!(poignee_de_main(&[0xFF], 7), DuServeur::Refus { raison: Raison::Invalide });
}

#[test]
fn toute_troncature_est_refusee_sans_paniquer() {
    let b = DuServeur::Bienvenue { tick: 1 << 40 }.encoder();
    for n in 0..b.len() {
        assert!(DuServeur::decoder(&b[..n]).is_err());
    }
    let mut long = b.clone();
    long.push(0);
    assert!(DuServeur::decoder(&long).is_err());
}

#[test]
fn entiers_variables_et_longueurs_hostiles() {
    // 11 octets de continuation : refusé.
    assert!(Lec::new(&[0xFF; 11]).v().is_err());
    // Longueur annoncée plus grande que le message : refusé sans allouer.
    let mut e = Ecr::default();
    e.v(u64::MAX);
    assert!(Lec::new(&e.b).txt().is_err());
    // Flottant non fini : refusé.
    assert!(Lec::new(&f32::NAN.to_le_bytes()).f32().is_err());
}

#[test]
fn positions_au_soixante_quatrieme_de_metre() {
    let p = Pos::new(TileId::de_zone(0, 652, 6862).map_or(0, |t| t.ix), 6862, 499.99, -500.0);
    let mut e = Ecr::default();
    e.pos(p);
    let q = Lec::new(&e.b).pos().unwrap();
    assert!(p.dist(q) < 0.02, "{p:?} {q:?}");
    let loin = Pos::new(0, 0, 1499.0, -1500.2);
    assert_eq!(loin.t, TileId { ix: 1, iy: -2 });
    assert!((-500.0..500.0).contains(&loin.l[0]) && (-500.0..500.0).contains(&loin.l[1]));
}
