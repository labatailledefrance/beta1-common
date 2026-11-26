//! Les décodeurs ne paniquent jamais, et ce qu'ils acceptent se réencode à
//! l'identique (pas deux encodages pour un même message).
#![no_main]

use beta1_common::protocole::{DuClient, DuServeur};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: &[u8]| {
    if let Ok(m) = DuClient::decoder(b) {
        assert_eq!(m.encoder(), b);
    }
    if let Ok(m) = DuServeur::decoder(b) {
        assert_eq!(m.encoder(), b);
    }
});
