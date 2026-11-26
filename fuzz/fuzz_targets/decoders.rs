//! Les decoders ne paniquent jamais, et ce qu'ils acceptent se réencode à
//! l'identique (pas deux encodings pour un même message).
#![no_main]

use beta1_common::protocol::{ClientMessage, ServerMessage};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: &[u8]| {
    if let Ok(m) = ClientMessage::decode(b) {
        assert_eq!(m.encode(), b);
    }
    if let Ok(m) = ServerMessage::decode(b) {
        assert_eq!(m.encode(), b);
    }
});
