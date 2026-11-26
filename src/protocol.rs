//! Protocole entre le serveur (qui fait autorité) et les clients : messages
//! binaires sur WebSocket, écrits à la main (P-24).
//!
//! Handshake : le client envoie [`ClientMessage::Hello`] avec son
//! [`FINGERPRINT`] ; le serveur répond [`ServerMessage::Welcome`] ou
//! [`ServerMessage::Rejected`] et ferme. Client et serveur ont toujours la même
//! version (H-01) : un fingerprint différent veut dire « rechargez le client ».

use crate::codec::{Invalid, Reader, Result, Writer};

include!(concat!(env!("OUT_DIR"), "/fingerprint.rs"));

/// Fréquence de la simulation (ticks par seconde).
pub const TICKS_PER_SECOND: u32 = 20;
/// Taille maximale d'un message reçu par le serveur, en octets.
pub const MAX_CLIENT_MESSAGE: usize = 4096;

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
}

/// Message du serveur.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    /// Handshake accepté.
    Welcome {
        /// Tick courant de la simulation.
        tick: u64,
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
}

const C_HELLO: u8 = 1;
const S_WELCOME: u8 = 1;
const S_REJECTED: u8 = 2;
const S_STATE: u8 = 3;

impl ClientMessage {
    /// Encode le message.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            ClientMessage::Hello { fingerprint } => {
                w.u8(C_HELLO);
                w.bytes(fingerprint);
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
            ServerMessage::Welcome { tick } => {
                w.u8(S_WELCOME);
                w.u64(*tick);
            }
            ServerMessage::Rejected { reason } => {
                w.u8(S_REJECTED);
                w.u8(*reason as u8);
            }
            ServerMessage::State { tick } => {
                w.u8(S_STATE);
                w.u64(*tick);
            }
        }
        w.buf
    }
    /// Décode un message entier ; refuse les octets en trop.
    pub fn decode(b: &[u8]) -> Result<ServerMessage> {
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            S_WELCOME => ServerMessage::Welcome { tick: r.u64()? },
            S_REJECTED => ServerMessage::Rejected { reason: RejectReason::from_u8(r.u8()?)? },
            S_STATE => ServerMessage::State { tick: r.u64()? },
            _ => return Err(Invalid),
        };
        r.finish()?;
        Ok(m)
    }
}

/// Réponse du serveur au premier message d'un client.
pub fn handshake(first: &[u8], tick: u64) -> ServerMessage {
    match ClientMessage::decode(first) {
        Ok(ClientMessage::Hello { fingerprint }) if fingerprint == FINGERPRINT => ServerMessage::Welcome { tick },
        Ok(ClientMessage::Hello { .. }) => ServerMessage::Rejected { reason: RejectReason::Version },
        Err(_) => ServerMessage::Rejected { reason: RejectReason::Invalid },
    }
}
