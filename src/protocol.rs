//! Protocole entre le serveur (qui fait autorité) et les clients : messages
//! binaires sur WebSocket, écrits à la main (P-24).
//!
//! Handshake : le client envoie [`ClientMessage::Hello`] avec son
//! [`FINGERPRINT`] ; le serveur répond [`ServerMessage::Welcome`] ou
//! [`ServerMessage::Rejected`] et ferme. Client et serveur ont toujours la même
//! version (H-01) : un fingerprint différent veut dire « rechargez le client ».
//!
//! Ensuite, le client dit ce qu'il regarde ([`ClientMessage::Vue`]) ; le
//! serveur lui envoie, à 10 Hz, l'état ([`ServerMessage::State`]) et les
//! zombies proches de sa vue ([`ServerMessage::Zombies`]), les plus proches
//! d'abord, dans un budget fixe (60 ko/s par client, H-05).

use crate::codec::{Invalid, Reader, Result, Writer};
use crate::geo::{Pos, TileId};

include!(concat!(env!("OUT_DIR"), "/fingerprint.rs"));

/// Fréquence de la simulation (ticks par seconde).
pub const TICKS_PER_SECOND: u32 = 20;
/// Taille maximale d'un message reçu par le serveur, en octets.
pub const MAX_CLIENT_MESSAGE: usize = 4096;
/// Rayon maximal de la vue des zombies individuels (m) ; au-delà, carte de densité (F-01).
pub const RAYON_MAX: f32 = 1000.0;
/// Zombies au plus par message [`ServerMessage::Zombies`] : 6 ko à 5 octets chacun.
pub const BUDGET_ZOMBIES: usize = 1200;

/// Zombie vu : position relative au centre de la tile d'ancrage, au 1/10 m,
/// et cap au 1/256 de tour. 5 octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZombieVu {
    /// Écart au centre de la tile d'ancrage (dm).
    pub d: [i16; 2],
    /// Cap, en 1/256 de tour.
    pub cap: u8,
}

impl ZombieVu {
    /// Zombie en `pos`, de cap `cap` (radians), vu depuis `ancre` ; `None` au-delà de 3,2 km.
    pub fn new(ancre: TileId, pos: Pos, cap: f32) -> Option<ZombieVu> {
        let v = Pos { tile: ancre, local: [0.0, 0.0] }.vector_to(pos);
        let q = |x: f32| {
            let d = (x * 10.0).round();
            (d.abs() <= i16::MAX as f32).then_some(d as i16)
        };
        let tour = cap.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        Some(ZombieVu { d: [q(v[0])?, q(v[1])?], cap: (tour * 256.0).round() as u32 as u8 })
    }
    /// Position, vue depuis `ancre`.
    pub fn pos(&self, ancre: TileId) -> Pos {
        Pos { tile: ancre, local: [self.d[0] as f32 / 10.0, self.d[1] as f32 / 10.0] }.normalized()
    }
    /// Cap (radians).
    pub fn cap(&self) -> f32 {
        self.cap as f32 / 256.0 * std::f32::consts::TAU
    }
}

/// Raison d'un rejet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectReason {
    /// Fingerprint du protocole différent : le client doit se recharger.
    Version = 1,
    /// Serveur plein.
    Full = 2,
    /// Message invalide.
    Invalid = 3,
}

impl RejectReason {
    fn from_u8(v: u8) -> Result<RejectReason> {
        match v {
            1 => Ok(RejectReason::Version),
            2 => Ok(RejectReason::Full),
            3 => Ok(RejectReason::Invalid),
            _ => Err(Invalid),
        }
    }
}

/// Message du client.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    /// Premier message : fingerprint du protocole du client.
    Hello {
        /// [`FINGERPRINT`] du client.
        fingerprint: [u8; 32],
    },
    /// Ce que le client regarde : zombies individuels à moins de `rayon` du centre.
    Vue {
        /// Centre de la vue.
        centre: Pos,
        /// Rayon (m), dans [0, [`RAYON_MAX`]] ; [`ClientMessage::vue`] le borne.
        rayon: f32,
    },
}

/// Message du serveur.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    /// Handshake accepté.
    Welcome {
        /// Tick courant de la simulation.
        tick: u64,
        /// Où placer la caméra au départ.
        camera: Pos,
    },
    /// Handshake rejeté ; le serveur ferme la connexion.
    Rejected {
        /// Pourquoi.
        reason: RejectReason,
    },
    /// État de la simulation (squelette : le tick seul).
    State {
        /// Tick de cet état.
        tick: u64,
    },
    /// Zombies proches de la vue du client, les plus proches d'abord.
    Zombies {
        /// Tick de ces positions.
        tick: u64,
        /// Tile d'ancrage des positions.
        ancre: TileId,
        /// Zombies dans le rayon de la vue, y compris ceux hors budget.
        total: u32,
        /// Au plus [`BUDGET_ZOMBIES`].
        zombies: Vec<ZombieVu>,
    },
}

const C_HELLO: u8 = 1;
const C_VUE: u8 = 2;
const S_WELCOME: u8 = 1;
const S_REJECTED: u8 = 2;
const S_STATE: u8 = 3;
const S_ZOMBIES: u8 = 4;

impl ClientMessage {
    /// Vue de rayon ramené dans [0, [`RAYON_MAX`]] (un rayon hors bornes est refusé au décodage).
    pub fn vue(centre: Pos, rayon: f32) -> ClientMessage {
        let rayon = if rayon.is_finite() { rayon.clamp(0.0, RAYON_MAX) } else { RAYON_MAX };
        ClientMessage::Vue { centre: centre.normalized(), rayon }
    }

    /// Encode le message.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            ClientMessage::Hello { fingerprint } => {
                w.u8(C_HELLO);
                w.bytes(fingerprint);
            }
            ClientMessage::Vue { centre, rayon } => {
                w.u8(C_VUE);
                w.pos(*centre);
                w.f32(*rayon);
            }
        }
        w.buf
    }
    /// Décode un message entier ; refuse les octets en trop.
    pub fn decode(b: &[u8]) -> Result<ClientMessage> {
        if b.len() > MAX_CLIENT_MESSAGE {
            return Err(Invalid);
        }
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            C_HELLO => ClientMessage::Hello { fingerprint: r.bytes()? },
            C_VUE => {
                let centre = r.pos()?;
                let rayon = r.f32()?;
                // Strict : une seule écriture possible pour chaque message.
                if !(0.0..=RAYON_MAX).contains(&rayon) || rayon.to_bits() == (-0.0f32).to_bits() {
                    return Err(Invalid);
                }
                ClientMessage::Vue { centre, rayon }
            }
            _ => return Err(Invalid),
        };
        r.finish()?;
        Ok(m)
    }
}

impl ServerMessage {
    /// Encode le message.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            ServerMessage::Welcome { tick, camera } => {
                w.u8(S_WELCOME);
                w.u64(*tick);
                w.pos(*camera);
            }
            ServerMessage::Rejected { reason } => {
                w.u8(S_REJECTED);
                w.u8(*reason as u8);
            }
            ServerMessage::State { tick } => {
                w.u8(S_STATE);
                w.u64(*tick);
            }
            ServerMessage::Zombies { tick, ancre, total, zombies } => {
                w.u8(S_ZOMBIES);
                w.u64(*tick);
                w.tile(*ancre);
                w.u32(*total);
                w.varint(zombies.len() as u64);
                for z in zombies {
                    w.i16(z.d[0]);
                    w.i16(z.d[1]);
                    w.u8(z.cap);
                }
            }
        }
        w.buf
    }
    /// Décode un message entier ; refuse les octets en trop.
    pub fn decode(b: &[u8]) -> Result<ServerMessage> {
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            S_WELCOME => ServerMessage::Welcome { tick: r.u64()?, camera: r.pos()? },
            S_REJECTED => ServerMessage::Rejected { reason: RejectReason::from_u8(r.u8()?)? },
            S_STATE => ServerMessage::State { tick: r.u64()? },
            S_ZOMBIES => {
                let (tick, ancre, total) = (r.u64()?, r.tile()?, r.u32()?);
                let n = r.count()?;
                if n > BUDGET_ZOMBIES {
                    return Err(Invalid);
                }
                let zombies =
                    (0..n).map(|_| Ok(ZombieVu { d: [r.i16()?, r.i16()?], cap: r.u8()? })).collect::<Result<_>>()?;
                ServerMessage::Zombies { tick, ancre, total, zombies }
            }
            _ => return Err(Invalid),
        };
        r.finish()?;
        Ok(m)
    }
}

/// Réponse du serveur au premier message d'un client ; `camera` : où le placer au départ.
pub fn handshake(first: &[u8], tick: u64, camera: Pos) -> ServerMessage {
    match ClientMessage::decode(first) {
        Ok(ClientMessage::Hello { fingerprint }) if fingerprint == FINGERPRINT => {
            ServerMessage::Welcome { tick, camera }
        }
        Ok(ClientMessage::Hello { .. }) => ServerMessage::Rejected { reason: RejectReason::Version },
        Ok(_) | Err(_) => ServerMessage::Rejected { reason: RejectReason::Invalid },
    }
}
