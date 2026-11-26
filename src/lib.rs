//! La Bataille de France, beta 1 : code partagé par le serveur, le client et la
//! chaîne de données.
//!
//! - [`geo`] : positions (tile de 1 km + position locale f32 centrée, C-01) ;
//! - [`codec`] : `Reader` et `Writer` binaires, bornés : une entrée hostile ne
//!   fait jamais paniquer le décodeur, elle rend une erreur ;
//! - [`protocol`] : messages entre le client et le serveur, handshake ;
//! - [`rng`] : aléatoire déterministe, indépendant de l'ordre et des threads ;
//! - [`tile`] : format des tiles produites par la chaîne de données.

pub mod codec;
pub mod geo;
pub mod protocol;
pub mod rng;
pub mod tile;
