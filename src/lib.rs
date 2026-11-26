//! La Bataille de France, beta 1 : code partagé par le serveur, le client et la
//! chaîne de données.
//!
//! - [`geo`] : positions (tile de 1 km + position locale f32 centrée, C-01) ;
//! - [`codec`] : lecture et écriture binaires, bornées : une entrée hostile ne
//!   fait jamais paniquer le décodeur, elle rend une erreur ;
//! - [`protocole`] : messages entre le client et le serveur ;
//! - [`alea`] : aléatoire déterministe, indépendant de l'ordre et des threads.

pub mod alea;
pub mod codec;
pub mod geo;
pub mod protocole;
