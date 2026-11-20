use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Printer {
    #[default]
    P1s,
    A1,
    A1Mini,
    X1c,
}

impl Printer {
    pub const ALL: [Self; 4] = [Self::P1s, Self::A1, Self::A1Mini, Self::X1c];

    pub fn name(self) -> &'static str {
        match self {
            Self::P1s => "Bambu Lab P1S",
            Self::A1 => "Bambu Lab A1",
            Self::A1Mini => "Bambu Lab A1 mini",
            Self::X1c => "Bambu Lab X1 Carbon",
        }
    }

    pub fn bed_size(self) -> [f32; 2] {
        match self {
            Self::A1Mini => [180.; 2],
            _ => [256.; 2],
        }
    }

    pub fn height(self) -> f32 {
        match self {
            Self::P1s | Self::X1c => 250.,
            Self::A1 => 256.,
            Self::A1Mini => 180.,
        }
    }

    pub fn excluded_corner(self) -> [f32; 2] {
        match self {
            Self::P1s | Self::X1c => [18., 28.],
            Self::A1 | Self::A1Mini => [0., 0.],
        }
    }

    pub fn nozzle_offset(self) -> [f32; 2] {
        match self {
            Self::P1s | Self::X1c => [0., 2.],
            Self::A1 | Self::A1Mini => [0., 0.],
        }
    }

    pub(super) fn template(self) -> &'static str {
        match self {
            Self::P1s => include_str!("../../config/export/p1s-0.4.json"),
            Self::A1 => include_str!("../../config/export/a1-0.4.json"),
            Self::A1Mini => include_str!("../../config/export/a1-mini-0.4.json"),
            Self::X1c => include_str!("../../config/export/x1c-0.4.json"),
        }
    }

    pub(super) fn filament_overrides(self) -> &'static str {
        match self {
            Self::P1s => include_str!("../../config/export/filament-overrides.json"),
            Self::A1 => include_str!("../../config/export/a1-filaments.json"),
            Self::A1Mini => include_str!("../../config/export/a1-mini-filaments.json"),
            Self::X1c => include_str!("../../config/export/x1c-filaments.json"),
        }
    }
}
