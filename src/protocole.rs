//! Protocole entre le serveur (qui fait autorité) et les clients : messages
//! binaires sur WebSocket, écrits à la main (P-24).
//!
//! Poignée de main : le client envoie [`DuClient::Bonjour`] avec son
//! [`EMPREINTE`] ; le serveur répond [`DuServeur::Bienvenue`] ou
//! [`DuServeur::Refus`] et ferme. Client et serveur ont toujours la même version
//! (H-01) : une empreinte différente veut dire « rechargez le client ».

use crate::codec::{Ecr, Invalide, Lec, R};

include!(concat!(env!("OUT_DIR"), "/empreinte.rs"));

/// Fréquence de la simulation (ticks par seconde).
pub const TICKS_PAR_SECONDE: u32 = 20;
/// Taille maximale d'un message reçu par le serveur, en octets.
pub const TAILLE_MAX_DU_CLIENT: usize = 4096;

/// Raison d'un refus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Raison {
    /// Empreinte du protocole différente : le client doit se recharger.
    Version = 1,
    /// Serveur plein.
    Plein = 2,
    /// Message invalide.
    Invalide = 3,
}

impl Raison {
    fn de(v: u8) -> R<Raison> {
        match v {
            1 => Ok(Raison::Version),
            2 => Ok(Raison::Plein),
            3 => Ok(Raison::Invalide),
            _ => Err(Invalide),
        }
    }
}

/// Message du client.
#[derive(Debug, Clone, PartialEq)]
pub enum DuClient {
    /// Premier message : empreinte du protocole du client.
    Bonjour {
        /// [`EMPREINTE`] du client.
        empreinte: [u8; 32],
    },
}

/// Message du serveur.
#[derive(Debug, Clone, PartialEq)]
pub enum DuServeur {
    /// Poignée de main acceptée.
    Bienvenue {
        /// Tick courant de la simulation.
        tick: u64,
    },
    /// Poignée de main refusée ; le serveur ferme la connexion.
    Refus {
        /// Pourquoi.
        raison: Raison,
    },
    /// État de la simulation (squelette : le tick seul).
    Etat {
        /// Tick de cet état.
        tick: u64,
    },
}

const C_BONJOUR: u8 = 1;
const S_BIENVENUE: u8 = 1;
const S_REFUS: u8 = 2;
const S_ETAT: u8 = 3;

impl DuClient {
    /// Encode le message.
    pub fn encoder(&self) -> Vec<u8> {
        let mut e = Ecr::default();
        match self {
            DuClient::Bonjour { empreinte } => {
                e.u8(C_BONJOUR);
                e.octets(empreinte);
            }
        }
        e.b
    }
    /// Décode un message entier ; refuse les octets en trop.
    pub fn decoder(b: &[u8]) -> R<DuClient> {
        if b.len() > TAILLE_MAX_DU_CLIENT {
            return Err(Invalide);
        }
        let mut l = Lec::new(b);
        let m = match l.u8()? {
            C_BONJOUR => DuClient::Bonjour { empreinte: l.octets()? },
            _ => return Err(Invalide),
        };
        l.fin()?;
        Ok(m)
    }
}

impl DuServeur {
    /// Encode le message.
    pub fn encoder(&self) -> Vec<u8> {
        let mut e = Ecr::default();
        match self {
            DuServeur::Bienvenue { tick } => {
                e.u8(S_BIENVENUE);
                e.u64(*tick);
            }
            DuServeur::Refus { raison } => {
                e.u8(S_REFUS);
                e.u8(*raison as u8);
            }
            DuServeur::Etat { tick } => {
                e.u8(S_ETAT);
                e.u64(*tick);
            }
        }
        e.b
    }
    /// Décode un message entier ; refuse les octets en trop.
    pub fn decoder(b: &[u8]) -> R<DuServeur> {
        let mut l = Lec::new(b);
        let m = match l.u8()? {
            S_BIENVENUE => DuServeur::Bienvenue { tick: l.u64()? },
            S_REFUS => DuServeur::Refus { raison: Raison::de(l.u8()?)? },
            S_ETAT => DuServeur::Etat { tick: l.u64()? },
            _ => return Err(Invalide),
        };
        l.fin()?;
        Ok(m)
    }
}

/// Réponse du serveur à un premier message.
pub fn poignee_de_main(premier: &[u8], tick: u64) -> DuServeur {
    match DuClient::decoder(premier) {
        Ok(DuClient::Bonjour { empreinte }) if empreinte == EMPREINTE => DuServeur::Bienvenue { tick },
        Ok(DuClient::Bonjour { .. }) => DuServeur::Refus { raison: Raison::Version },
        Err(_) => DuServeur::Refus { raison: Raison::Invalide },
    }
}
