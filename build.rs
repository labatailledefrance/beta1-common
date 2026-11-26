//! Empreinte du protocole : BLAKE3 des sources qui définissent les messages.
//! Toute modification du protocole change l'empreinte ; un client et un serveur
//! d'empreintes différentes refusent de se parler (H-01 : même version partout).

// Un script de build qui échoue arrête la compilation : paniquer est le bon comportement.
#![allow(clippy::panic)]

use std::fs;
use std::path::Path;

const SOURCES: [&str; 3] = ["src/codec.rs", "src/geo.rs", "src/protocole.rs"];

fn main() {
    let mut h = blake3::Hasher::new();
    for s in SOURCES {
        println!("cargo::rerun-if-changed={s}");
        let octets = fs::read(s).unwrap_or_else(|e| panic!("{s} : {e}"));
        h.update(s.as_bytes());
        h.update(&(octets.len() as u64).to_le_bytes());
        h.update(&octets);
    }
    let e = h.finalize();
    let sortie = Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|e| panic!("OUT_DIR : {e}"))).join("empreinte.rs");
    fs::write(
        sortie,
        format!(
            "/// Empreinte BLAKE3 du protocole (calculée par build.rs).\npub const EMPREINTE: [u8; 32] = {:?};\n",
            e.as_bytes()
        ),
    )
    .unwrap_or_else(|e| panic!("empreinte.rs : {e}"));
}
