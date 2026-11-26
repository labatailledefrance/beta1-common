//! Positions (C-01) : tile de 1 km + position locale f32 centrée (±500 m).
//! Aucune position absolue en f32, ni en simulation, ni dans le protocole.

/// Une tile est (zone de projection, ix, iy). La zone est portée par `ix`, en
/// bandes de [`ZONE_STRIDE`] tiles : deux zones sont toujours à des milliers de
/// kilomètres l'une de l'autre, sans cas particulier dans les calculs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileId {
    /// Colonne, zone comprise.
    pub ix: i32,
    /// Ligne (kilomètres vers le nord dans la projection de la zone).
    pub iy: i32,
}

/// Largeur d'une bande de zone, en tiles.
pub const ZONE_STRIDE: i32 = 2048;
/// Zones de projection : Lambert-93, puis UTM 20N, 22N, 40S, 38S, 21N.
pub const ZONES: [&str; 6] = ["LAMB93", "UTM20", "UTM22", "UTM40S", "UTM38S", "UTM21"];
/// Côté d'une tile, en mètres.
pub const TILE_SIZE: f32 = 1000.0;
const HALF: f32 = TILE_SIZE / 2.0;

impl TileId {
    /// Tile `(ix, iy)` de la zone `zone`, `ix` compté dans la zone.
    pub fn from_zone(zone: u8, ix: i32, iy: i32) -> Option<TileId> {
        ((zone as usize) < ZONES.len() && (0..ZONE_STRIDE).contains(&ix))
            .then(|| TileId { ix: zone as i32 * ZONE_STRIDE + ix, iy })
    }
    /// Zone de projection.
    pub fn zone(self) -> u8 {
        self.ix.div_euclid(ZONE_STRIDE).clamp(0, 255) as u8
    }
    /// Indice ix dans sa zone.
    pub fn zone_ix(self) -> i32 {
        self.ix.rem_euclid(ZONE_STRIDE)
    }
}

/// Position : tile + position locale centrée, dans [-500, 500[ sur chaque axe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    /// Tile.
    pub tile: TileId,
    /// Position locale (m), centrée sur la tile.
    pub local: [f32; 2],
}

impl Pos {
    /// Position normalisée.
    pub fn new(ix: i32, iy: i32, x: f32, y: f32) -> Pos {
        Pos { tile: TileId { ix, iy }, local: [x, y] }.normalized()
    }
    /// Ramène la position locale dans [-500, 500[.
    #[inline]
    pub fn normalized(mut self) -> Pos {
        for k in 0..2 {
            if !(-HALF..HALF).contains(&self.local[k]) {
                let n = ((self.local[k] + HALF) / TILE_SIZE).floor();
                self.local[k] -= n * TILE_SIZE;
                let mut n = n as i32;
                // Arrondi : juste sous -500.
                if self.local[k] >= HALF {
                    self.local[k] -= TILE_SIZE;
                    n += 1;
                }
                if k == 0 {
                    self.tile.ix = self.tile.ix.saturating_add(n);
                } else {
                    self.tile.iy = self.tile.iy.saturating_add(n);
                }
            }
        }
        self
    }
    /// Position décalée de `d` mètres.
    #[inline]
    pub fn offset(self, d: [f32; 2]) -> Pos {
        Pos { tile: self.tile, local: [self.local[0] + d[0], self.local[1] + d[1]] }.normalized()
    }
    /// Vecteur de `self` vers `o` (m).
    #[inline]
    pub fn vector_to(self, o: Pos) -> [f32; 2] {
        [
            (o.tile.ix - self.tile.ix) as f32 * TILE_SIZE + (o.local[0] - self.local[0]),
            (o.tile.iy - self.tile.iy) as f32 * TILE_SIZE + (o.local[1] - self.local[1]),
        ]
    }
    /// Distance (m).
    #[inline]
    pub fn distance(self, o: Pos) -> f32 {
        let v = self.vector_to(o);
        (v[0] * v[0] + v[1] * v[1]).sqrt()
    }
}
