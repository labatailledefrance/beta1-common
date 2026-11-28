//! Les decoders ne paniquent jamais, et ce qu'ils acceptent se réencode à
//! l'identique (pas deux encodings pour un même message). Les tiles et leurs
//! sections viennent aussi d'Internet (client) : mêmes exigences, sans panique.
#![no_main]

use beta1_common::graph::RoadGraph;
use beta1_common::index::NationalIndex;
use beta1_common::protocol::{ClientMessage, ServerMessage};
use beta1_common::tile::{self, SectionId, relief, sections};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: &[u8]| {
    if let Ok(m) = ClientMessage::decode(b) {
        assert_eq!(m.encode(), b);
    }
    if let Ok(m) = ServerMessage::decode(b) {
        assert_eq!(m.encode(), b);
    }
    // Tile entière, sans empreinte (sinon le fuzzer ne passe jamais la vérification).
    if let Ok(t) = tile::decode(b, false) {
        for id in SectionId::ALL {
            let _ = t.section(id);
        }
    }
    let _ = tile::decode_pack_index(b);
    let _ = tile::decode_header(b);
    // Index national : une seule écriture.
    if let Ok(x) = NationalIndex::decode_body(b) {
        assert_eq!(x.encode_body(), b);
    }
    // Sections décompressées : chaque décodeur sur les mêmes octets.
    let _ = sections::buildings(b);
    let _ = sections::network(b);
    let _ = sections::bridges(b);
    let _ = sections::zone(b);
    let _ = sections::communes(b);
    let _ = sections::places(b);
    let _ = sections::passable(b);
    if let Ok(base) = relief::decode(b, 2) {
        let _ = relief::encode(&base, 10);
    }
    if let Some((a, f)) = b.split_first().map(|(k, r)| r.split_at((*k as usize * 97).min(r.len()))) {
        let _ = relief::decode_two(a, f);
    }
    // Graphe routier, sans empreinte valide il est refusé : on teste le corps
    // en recalculant l'empreinte.
    let mut g = b.to_vec();
    g.extend_from_slice(blake3::hash(b).as_bytes());
    let _ = RoadGraph::decode(&g);
    let _ = tile::decompress(b, b.len().saturating_mul(4).min(1 << 20));
});
