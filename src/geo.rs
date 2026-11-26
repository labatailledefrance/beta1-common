//! Positions (C-01) : tile de 1 km + position locale f32 centrée (±500 m).
//! Aucune position absolue en f32, ni en simulation, ni dans le protocole.

/// Une tile est (zone de projection, ix, iy). La zone est portée par `ix`, en
/// bandes de [`ECART`] tiles : deux zones sont toujours à des milliers de
/// kilomètres l'une de l'autre, sans cas particulier dans les calculs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileId {
    /// Colonne, zone comprise.
    pub ix: i32,
    /// Ligne (kilomètres vers le nord dans la projection de la zone).
    pub iy: i32,
}

/// Largeur d'une bande de zone, en tiles.
pub const ECART: i32 = 2048;
/// Zones de projection : Lambert-93, puis UTM 20N, 22N, 40S, 38S, 21N.
pub const ZONES: [&str; 6] = ["LAMB93", "UTM20", "UTM22", "UTM40S", "UTM38S", "UTM21"];
/// Côté d'une tile, en mètres.
pub const COTE: f32 = 1000.0;
const DEMI: f32 = COTE / 2.0;

impl TileId {
    /// Tile `(ix, iy)` de la zone `zone`, `ix` compté dans la zone.
    pub fn de_zone(zone: u8, ix: i32, iy: i32) -> Option<TileId> {
        ((zone as usize) < ZONES.len() && (0..ECART).contains(&ix)).then(|| TileId { ix: zone as i32 * ECART + ix, iy })
    }
    /// Zone de projection.
    pub fn zone(self) -> u8 {
        self.ix.div_euclid(ECART).clamp(0, 255) as u8
    }
    /// Indice ix dans sa zone.
    pub fn ix_zone(self) -> i32 {
        self.ix.rem_euclid(ECART)
    }
}

/// Position : tile + position locale centrée, dans [-500, 500[ sur chaque axe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    /// Tile.
    pub t: TileId,
    /// Position locale (m), centrée sur la tile.
    pub l: [f32; 2],
}

impl Pos {
    /// Position normalisée.
    pub fn new(ix: i32, iy: i32, x: f32, y: f32) -> Pos {
        Pos { t: TileId { ix, iy }, l: [x, y] }.norm()
    }
    /// Ramène la position locale dans [-500, 500[.
    #[inline]
    pub fn norm(mut self) -> Pos {
        for k in 0..2 {
            if !(-DEMI..DEMI).contains(&self.l[k]) {
                let n = ((self.l[k] + DEMI) / COTE).floor();
                self.l[k] -= n * COTE;
                let mut n = n as i32;
                // Arrondi : juste sous -500.
                if self.l[k] >= DEMI {
                    self.l[k] -= COTE;
                    n += 1;
                }
                if k == 0 {
                    self.t.ix = self.t.ix.saturating_add(n);
                } else {
                    self.t.iy = self.t.iy.saturating_add(n);
                }
            }
        }
        self
    }
    /// Déplacement de `d` mètres.
    #[inline]
    pub fn plus(self, d: [f32; 2]) -> Pos {
        Pos { t: self.t, l: [self.l[0] + d[0], self.l[1] + d[1]] }.norm()
    }
    /// Vecteur de `self` vers `o` (m).
    #[inline]
    pub fn vers(self, o: Pos) -> [f32; 2] {
        [
            (o.t.ix - self.t.ix) as f32 * COTE + (o.l[0] - self.l[0]),
            (o.t.iy - self.t.iy) as f32 * COTE + (o.l[1] - self.l[1]),
        ]
    }
    /// Distance (m).
    #[inline]
    pub fn dist(self, o: Pos) -> f32 {
        let v = self.vers(o);
        (v[0] * v[0] + v[1] * v[1]).sqrt()
    }
}
