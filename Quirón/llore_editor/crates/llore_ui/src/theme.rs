//! # Tema Visual de Llore
//!
//! Colores y estilos para la interfaz.

/// Tema visual operativo de la UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiTheme {
    QuironDark,
    GraphiteDark,
    CopperLight,
}

impl UiTheme {
    pub fn config_value(self) -> &'static str {
        match self {
            UiTheme::QuironDark => "quiron_dark",
            UiTheme::GraphiteDark => "graphite_dark",
            UiTheme::CopperLight => "copper_light",
        }
    }

    pub fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "quiron_dark" => Some(UiTheme::QuironDark),
            "graphite_dark" => Some(UiTheme::GraphiteDark),
            "copper_light" => Some(UiTheme::CopperLight),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            UiTheme::QuironDark => "Quiron Dark",
            UiTheme::GraphiteDark => "Graphite Dark",
            UiTheme::CopperLight => "Copper Light",
        }
    }

    pub fn next(self) -> Self {
        match self {
            UiTheme::QuironDark => UiTheme::GraphiteDark,
            UiTheme::GraphiteDark => UiTheme::CopperLight,
            UiTheme::CopperLight => UiTheme::QuironDark,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            UiTheme::QuironDark => UiTheme::CopperLight,
            UiTheme::GraphiteDark => UiTheme::QuironDark,
            UiTheme::CopperLight => UiTheme::GraphiteDark,
        }
    }
}

/// Paleta completa usada por render.
#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub is_light: bool,
    pub background: u32,
    pub surface: u32,
    pub text: u32,
    pub text_muted: u32,
    pub accent: u32,
    pub accent_alt: u32,
    pub error: u32,
    pub warning: u32,
    pub success: u32,
    pub border: u32,
    pub selection: u32,
    pub line_highlight: u32,
}

const QUIRON_DARK_PALETTE: ThemePalette = ThemePalette {
    is_light: false,
    background: 0x0D1117, // GitHub Dark / Antigravity base
    surface: 0x161B22,    // Slightly lighter for surfaces
    text: 0xF2F5F8,
    text_muted: 0xB5BEC9,
    accent: 0x58A6FF,     // Vibrant blue
    accent_alt: 0x882EE0, // Deep purple
    error: 0xF85149,
    warning: 0xD29922,
    success: 0x2EA043,
    border: 0x364152,
    selection: 0x2C4775,
    line_highlight: 0x202A36,
};

// Inspirado en paletas editoriales oscuras tipo Codex/Antigravity:
// negros neutros, contraste alto y acento azul frío.
const GRAPHITE_DARK_PALETTE: ThemePalette = ThemePalette {
    is_light: false,
    background: 0x09090B,  // True dark / Zinc
    surface: 0x18181B,     // Zinc 900
    text: 0xFAFAFA,        // Zinc 50
    text_muted: 0xA1A1AA,  // Zinc 400
    accent: 0x3B82F6,      // Blue 500
    accent_alt: 0x0EA5E9,  // Sky 500
    error: 0xEF4444,       // Red 500
    warning: 0xF59E0B,     // Amber 500
    success: 0x10B981,     // Emerald 500
    border: 0x18181B,      // Same as surface to eliminate the box effect
    selection: 0x2563EB40, // Semi-transparent blue
    line_highlight: 0x27272A40,
};

// Tema claro cálido para contraste día/noche.
const COPPER_LIGHT_PALETTE: ThemePalette = ThemePalette {
    is_light: true,
    background: 0xFAFAFA, // Zinc 50
    surface: 0xF4F4F5,    // Zinc 100
    text: 0x18181B,       // Zinc 900
    text_muted: 0x71717A, // Zinc 500
    accent: 0x2563EB,     // Blue 600
    accent_alt: 0x0284C7, // Sky 600
    error: 0xDC2626,      // Red 600
    warning: 0xD97706,    // Amber 600
    success: 0x059669,    // Emerald 600
    border: 0xF4F4F5,     // Merges with surface for light theme
    selection: 0xE0E7FF,  // Indigo 100 (Clean pale blue)
    line_highlight: 0xF4F4F5,
};

pub fn palette(theme: UiTheme) -> &'static ThemePalette {
    match theme {
        UiTheme::QuironDark => &QUIRON_DARK_PALETTE,
        UiTheme::GraphiteDark => &GRAPHITE_DARK_PALETTE,
        UiTheme::CopperLight => &COPPER_LIGHT_PALETTE,
    }
}

/// Paleta de colores para el tema oscuro.
pub mod colors {
    /// Fondo principal del editor
    pub const BACKGROUND: u32 = 0x0D1117;
    /// Fondo secundario (sidebar, tabs)
    pub const SURFACE: u32 = 0x161B22;
    /// Texto principal
    pub const TEXT: u32 = 0xC9D1D9;
    /// Texto secundario
    pub const TEXT_MUTED: u32 = 0x8B949E;
    /// Acento primario
    pub const ACCENT: u32 = 0x58A6FF;
    /// Acento secundario
    pub const ACCENT_ALT: u32 = 0x882EE0;
    /// Error
    pub const ERROR: u32 = 0xF85149;
    /// Advertencia
    pub const WARNING: u32 = 0xD29922;
    /// Éxito
    pub const SUCCESS: u32 = 0x2EA043;
    /// Borde
    pub const BORDER: u32 = 0x1E232B;
    /// Selección
    pub const SELECTION: u32 = 0x264F78;
    /// Línea actual
    pub const LINE_HIGHLIGHT: u32 = 0x1F2428;
}

/// Tamaños de fuente
pub mod font_sizes {
    /// Tamaño de código
    pub const CODE: f32 = 15.5;
    /// Tamaño de UI
    pub const UI: f32 = 14.5;
    /// Tamaño pequeño
    pub const SMALL: f32 = 12.5;
}

/// Espaciados
pub mod spacing {
    /// Padding pequeño
    pub const XS: f32 = 2.0;
    /// Padding normal
    pub const SM: f32 = 4.0;
    /// Padding medio
    pub const MD: f32 = 8.0;
    /// Padding grande
    pub const LG: f32 = 16.0;
    /// Padding muy grande
    pub const XL: f32 = 24.0;
}
