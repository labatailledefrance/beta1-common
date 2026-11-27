# beta1-common

Code partagé de la beta 1 de **La Bataille de France** : le serveur ([beta1-server](https://github.com/labatailledefrance/beta1-server)), le client ([beta1-client](https://github.com/labatailledefrance/beta1-client)) et la chaîne de données ([beta1-datachain](https://github.com/labatailledefrance/beta1-datachain)) en dépendent, épinglés sur le même tag.

| Module | Contenu |
|---|---|
| `geo` | Positions : tile de 1 km (zone de projection, ix, iy) + position locale f32 centrée |
| `codec` | `Reader` et `Writer` binaires ; le `Reader` ne panique jamais |
| `protocol` | Messages client ↔ serveur et handshake |
| `rng` | Aléatoire déterministe, indépendant de l'ordre et des threads |
| `tile` | Format des tiles : en-tête, sections compressées (zstd en Rust pur, pour le wasm), empreinte BLAKE3, paquets de 16 × 16 ; relief (base 2 m + complément 1 m) ; décodeur de chaque section ; codes des énumérations de la BD TOPO |
| `graph` | Graphe routier national (`graphe.bin`) : nœuds, tronçons recousus, attributs, ouvrages et charges, géométrie, **ordre CCH** |

## Fingerprint du protocole

`build.rs` calcule le hash BLAKE3 des sources du protocole (`codec.rs`, `geo.rs`, `protocol.rs`). Client et serveur le comparent au handshake : toute modification du protocole impose de les livrer ensemble, ce qui est la règle (une mise à jour du serveur remet le monde à zéro).

## Format des tiles

Produit par [beta1-datachain](https://github.com/labatailledefrance/beta1-datachain), lu par le serveur et le client. Le décodage ne panique jamais et refuse toute section qui ne se lit pas jusqu'au dernier octet ; la taille décompressée est bornée (pas de bombe zstd). Vérifié sur les 18 323 tiles du Finistère ; fuzzé avec le protocole.

## Graphe routier

Produit une fois par publication par la chaîne (étape la plus longue : l'ordre CCH, par dissection imbriquée), lu par le serveur qui construit la hiérarchie et la personnalise par classe d'itinéraire ([A-07](https://github.com/labatailledefrance/documentation/blob/main/OPEN-PROBLEMS.md#a-07--recherche-de-chemin), [B-03](https://github.com/labatailledefrance/documentation/blob/main/OPEN-PROBLEMS.md#b-03--charge-limite-des-ponts)). Format de P-26 (`P26G`), mêmes octets : sections zstd et empreinte BLAKE3. Le décodage vérifie tout ce dont le serveur dépend (indices dans les bornes, ordre qui est une permutation) et ne panique jamais ; fuzzé.

## Développer

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo deny check
cargo +nightly fuzz run decoders   # fuzzing (cargo install cargo-fuzz)
```

Règles de contribution : [CONTRIBUTING.md](https://github.com/labatailledefrance/.github/blob/main/CONTRIBUTING.md). Licence : AGPL-3.0-or-later.
