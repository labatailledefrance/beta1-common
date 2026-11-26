//! Fingerprint du protocole : hash BLAKE3 des sources qui définissent les
//! messages. Toute modification du protocole change le fingerprint ; un client et
//! un serveur de fingerprints différents refusent de se parler (H-01 : même
//! version partout).

// Un script de build qui échoue arrête la compilation : paniquer est le bon comportement.
#![allow(clippy::panic)]

use std::fs;
use std::path::Path;

const SOURCES: [&str; 3] = ["src/codec.rs", "src/geo.rs", "src/protocol.rs"];

fn main() {
    let mut hasher = blake3::Hasher::new();
    for s in SOURCES {
        println!("cargo::rerun-if-changed={s}");
        let bytes = fs::read(s).unwrap_or_else(|e| panic!("{s} : {e}"));
        hasher.update(s.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let hash = hasher.finalize();
    let out = Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|e| panic!("OUT_DIR : {e}"))).join("fingerprint.rs");
    fs::write(
        out,
        format!(
            "/// Fingerprint BLAKE3 du protocole (calculé par build.rs).\npub const FINGERPRINT: [u8; 32] = {:?};\n",
            hash.as_bytes()
        ),
    )
    .unwrap_or_else(|e| panic!("fingerprint.rs : {e}"));
}
