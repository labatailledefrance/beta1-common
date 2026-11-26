//! Codes des énumérations de la BD TOPO, dans l'ordre du format : un octet
//! est l'indice dans la liste. 254 : valeur vide ; 255 : valeur inconnue.
//! L'ordre est figé ; ajouter une valeur, c'est l'ajouter **à la fin**.

/// Valeur vide dans la BD TOPO.
pub const EMPTY: u8 = 254;
/// Valeur absente de la liste.
pub const UNKNOWN: u8 = 255;

/// Code de `v` dans `list`.
pub fn code(list: &[&str], v: &str) -> u8 {
    list.iter().position(|x| *x == v).map_or(if v.is_empty() { EMPTY } else { UNKNOWN }, |p| p as u8)
}

/// Libellé du code `c`, s'il existe.
pub fn label<'a>(list: &[&'a str], c: u8) -> Option<&'a str> {
    list.get(c as usize).copied()
}

/// Usages des bâtiments (`usage_1`, `usage_2`).
pub const USAGES: &[&str] = &[
    "Résidentiel",
    "Indifférencié",
    "Annexe",
    "Commercial et services",
    "Industriel",
    "Sportif",
    "Religieux",
    "Agricole",
];

/// Natures des bâtiments.
pub const BUILDING_NATURES: &[&str] = &[
    "Indifférenciée",
    "Industriel, agricole ou commercial",
    "Eglise",
    "Chapelle",
    "Monument",
    "Serre",
    "Silo",
    "Château",
    "Tour, donjon",
    "Tribune",
    "Moulin à vent",
    "Fort, blockhaus, casemate",
    "Arc de triomphe",
    "Arène ou théâtre antique",
    "Phare",
];

/// Natures des tronçons de route.
pub const ROAD_NATURES: &[&str] = &[
    "Type autoroutier",
    "Route à 2 chaussées",
    "Route à 1 chaussée",
    "Bretelle",
    "Rond-point",
    "Route empierrée",
    "Chemin",
    "Sentier",
    "Escalier",
    "Bac ou liaison maritime",
    "Piste cyclable",
    "Bac ou liaison fluviale",
];

/// Sens de circulation.
pub const DIRECTIONS: &[&str] = &["Double sens", "Sens direct", "Sens inverse", "Sans objet"];

/// Accès des véhicules légers.
pub const LIGHT_VEHICLE_ACCESS: &[&str] =
    &["Libre", "A péage", "Restreint aux ayants droit", "Physiquement impossible", "Saisonnier"];

/// Classes de végétation (quartet bas de l'occupation, 1 à 10) ; 5 = haie.
pub const VEGETATION: &[&str] = &[
    "",
    "Forêt fermée de feuillus",
    "Forêt fermée de conifères",
    "Forêt fermée mixte",
    "Bois",
    "Haie",
    "Lande ligneuse",
    "Verger",
    "Vigne",
    "Peupleraie",
    "Forêt ouverte",
];

/// Classes de largeur des cours d'eau.
pub const WATERCOURSE_WIDTHS: &[&str] = &["Entre 0 et 15 m", "Entre 15 et 50 m", "Plus de 50 m", "Sans objet"];

/// Types de lieux d'intérêt.
pub const PLACE_KINDS: &[&str] = &[
    "Mairie",
    "Centrale électrique (CNPE)",
    "Usine de retraitement",
    "Port",
    "Gare",
    "Gare maritime",
    "Aérodrome",
    "Piste",
    "Barrage",
    "Enceinte militaire",
    "Centrale éolienne",
    "Écluse",
];

/// Occupation : eau douce (bit).
pub const OCC_WATER: u8 = 16;
/// Occupation : estran (bit).
pub const OCC_FORESHORE: u8 = 32;
/// Occupation : mer (bit).
pub const OCC_SEA: u8 = 64;
/// Occupation : eau salée (bit).
pub const OCC_SALT: u8 = 128;
/// Occupation : hors de France (gris, infranchissable), quartet bas à 15.
pub const OCC_FOREIGN: u8 = 15;

/// Code INSEE numérique d'une commune : `2A004` → 1 000 004, `2B033` →
/// 1 100 033 (Corse), sinon le code lui-même (`29019` → 29 019).
pub fn commune_code(insee: &str) -> u32 {
    if let Some(r) = insee.strip_prefix("2A") {
        1_000_000 + r.parse::<u32>().unwrap_or(0)
    } else if let Some(r) = insee.strip_prefix("2B") {
        1_100_000 + r.parse::<u32>().unwrap_or(0)
    } else {
        insee.parse().unwrap_or(0)
    }
}

/// Code INSEE d'un code numérique ([`commune_code`] à l'envers).
pub fn commune_insee(code: u32) -> String {
    match code {
        1_100_000.. => format!("2B{:03}", code - 1_100_000),
        1_000_000.. => format!("2A{:03}", code - 1_000_000),
        _ => format!("{code:05}"),
    }
}
