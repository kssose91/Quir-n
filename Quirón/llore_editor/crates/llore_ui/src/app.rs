//! # App
//!
//! Aplicación principal y event loop usando winit 0.30 API.
//! Conecta con quiron-brain para la mente.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use arboard::Clipboard;
use regex::{Regex, RegexBuilder};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::WindowId;

#[derive(Debug, Clone)]
pub enum AppEvent {
    FileOpened(PathBuf),
    FolderOpened(PathBuf),
}

use llore_brain::client::{
    HealthResponse, LlmToolDef, SessionTelemetryAnomalySummary,
    SessionTelemetryCheckpointSummary, SessionTelemetryResponse,
};
use llore_brain::orchestrator::Citation;
use llore_brain::{DelegationMetrics, Quiron, QuironConfig, SessionTelemetrySnapshot};
use llore_editor::Editor;
use llore_language::LanguageRegistry;
use rfd::{AsyncFileDialog, FileHandle};

use crate::chat_tools;
use crate::layout::{Bounds, LayoutEngine};
use crate::syntax_highlight::syntax_diagnostic;
use crate::text::TextSystem;
use crate::design::Scale;
use crate::project_id;
use crate::recents;
use crate::theme::{ThemePalette, UiTheme};
use crate::workspace_guard::{self, Access};
use crate::{Color, Window};

/// Alto del tab bar en panel editor.
pub const EDITOR_TAB_BAR_HEIGHT: f32 = 24.0;
/// Altura de línea en el render de código.
pub const EDITOR_LINE_HEIGHT: f32 = 18.0;
/// Ancho aproximado por carácter monoespaciado.
pub const EDITOR_CHAR_WIDTH: f32 = 8.0;
/// Tamaño base de fuente para render del código.
pub const EDITOR_CODE_FONT_SIZE: f32 = 13.0;
/// Escala de fuente por defecto del editor.
pub const DEFAULT_EDITOR_FONT_SCALE: f32 = 1.0;
/// Escala mínima permitida para fuente del editor.
pub const MIN_EDITOR_FONT_SCALE: f32 = 0.80;
/// Escala máxima permitida para fuente del editor.
pub const MAX_EDITOR_FONT_SCALE: f32 = 1.45;
/// Paso por ajuste de escala de fuente del editor.
pub const EDITOR_FONT_SCALE_STEP: f32 = 0.05;
/// Padding horizontal base del bloque de código tras el gutter.
pub const DEFAULT_EDITOR_HORIZONTAL_PADDING: f32 = 4.0;
/// Padding horizontal mínimo del bloque de código.
pub const MIN_EDITOR_HORIZONTAL_PADDING: f32 = 0.0;
/// Padding horizontal máximo del bloque de código.
pub const MAX_EDITOR_HORIZONTAL_PADDING: f32 = 32.0;
/// Paso de ajuste de padding horizontal de editor.
pub const EDITOR_HORIZONTAL_PADDING_STEP: f32 = 2.0;
/// Sangría horizontal por nivel en el árbol del explorer.
pub const DEFAULT_EXPLORER_INDENT_STEP: f32 = 12.0;
/// Sangría mínima por nivel del explorer.
pub const MIN_EXPLORER_INDENT_STEP: f32 = 8.0;
/// Sangría máxima por nivel del explorer.
pub const MAX_EXPLORER_INDENT_STEP: f32 = 24.0;
/// Paso de ajuste de sangría del explorer.
pub const EXPLORER_INDENT_STEP_DELTA: f32 = 1.0;
/// Ancho del gutter para números de línea.
pub const EDITOR_GUTTER_WIDTH: f32 = 46.0;
/// Padding superior del body editor bajo el tab bar.
pub const EDITOR_BODY_TOP_PADDING: f32 = 4.0;
/// Padding total usado para cálculo de body editor.
pub const EDITOR_BODY_BOTTOM_PADDING: f32 = 8.0;
/// Número máximo de filas visibles en panel de resultados de búsqueda.
pub const SEARCH_RESULTS_MAX_ROWS: usize = 6;
/// Umbral temporal para considerar doble click.
pub const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(350);
/// Ruta relativa del snapshot de sesión de tabs.
pub const SESSION_SNAPSHOT_RELATIVE_PATH: &str = ".llore/state/session.txt";
/// Ruta relativa del snapshot de layout de paneles.
pub const LAYOUT_SNAPSHOT_RELATIVE_PATH: &str = ".llore/state/layout.txt";
/// Valor por defecto del ancho de sidebar.
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 252.0;

// Límites de columna del rediseño «Modernist» (`diseño/Llore Rediseño.dc.html`,
// `LIMITES = { izq: [172, 420], editor: [340, 780], fondo: [252, 520] }`, con el
// chat reservando 320). No son adorno: una columna por debajo de su mínimo deja
// de servir para trabajar, que es justo lo que permitía el 180 anterior —el
// mismo suelo para el explorador, el editor y el chat.
/// Ancho mínimo de la barra lateral.
pub const MIN_SIDEBAR_WIDTH: f32 = 172.0;
/// Ancho máximo de la barra lateral.
pub const MAX_SIDEBAR_WIDTH: f32 = 420.0;
/// Ancho mínimo del editor.
pub const MIN_EDITOR_WIDTH: f32 = 340.0;
/// Ancho mínimo del panel de chat.
pub const MIN_CHAT_WIDTH: f32 = 320.0;
/// Ancho máximo del editor.
pub const MAX_EDITOR_WIDTH: f32 = 780.0;
/// Ancho de partida del editor.
pub const DEFAULT_EDITOR_WIDTH: f32 = 532.0;
/// Ancho mínimo del panel de segundo plano.
pub const MIN_BACKGROUND_WIDTH: f32 = 252.0;
/// Ancho máximo del panel de segundo plano.
pub const MAX_BACKGROUND_WIDTH: f32 = 520.0;
/// Ancho de partida del panel de segundo plano.
pub const DEFAULT_BACKGROUND_WIDTH: f32 = 360.0;
/// Separación entre columnas.
pub const COLUMN_GAP: f32 = 12.0;
/// Valor por defecto del split editor/chat.
pub const DEFAULT_EDITOR_SPLIT_RATIO: f32 = 0.58;
/// Valor por defecto del split interno entre panel editor primario/secundario.
pub const DEFAULT_EDITOR_PANE_SPLIT_RATIO: f32 = 0.5;
/// Modelos seleccionables desde el chat usando la misma ruta de conexión.
///
/// Verificados contra la cuenta el 2026-07-10. `gpt-5.3` y `codex-spark` existen
/// pero el servidor los rechaza con una cuenta de suscripción; `gpt-5.6-luna` no
/// existe.
pub const AI_MODEL_OPTIONS: &[&str] = &[
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
];
/// Caracteres de un archivo que se adjuntan al chat.
///
/// Un archivo mayor se recorta, y el recorte se le declara al modelo: un
/// contenido truncado en silencio invita a concluir que algo no existe.
const CHAT_CONTEXT_MAX_CHARS: usize = 24_000;
/// Máximo de archivos escaneados para quick open.
pub const QUICK_OPEN_MAX_FILES: usize = 5000;
/// Profundidad máxima de escaneo de quick open.
pub const QUICK_OPEN_MAX_DEPTH: usize = 14;
/// Máximo de archivos analizados para índice de símbolos de workspace.
pub const WORKSPACE_SYMBOL_MAX_FILES: usize = 1200;
/// Tamaño máximo por archivo (bytes) para indexar símbolos de workspace.
pub const WORKSPACE_SYMBOL_MAX_FILE_BYTES: u64 = 512 * 1024;
/// Máximo de símbolos cacheados del workspace.
pub const WORKSPACE_SYMBOL_MAX_ENTRIES: usize = 20_000;
/// Máximo de archivos escaneados para búsqueda textual global.
pub const WORKSPACE_TEXT_SEARCH_MAX_FILES: usize = 1400;
/// Tamaño máximo por archivo (bytes) para búsqueda textual global.
pub const WORKSPACE_TEXT_SEARCH_MAX_FILE_BYTES: u64 = 384 * 1024;
/// Máximo de filas candidatas mantenidas antes de truncar resultados.
pub const WORKSPACE_TEXT_SEARCH_MAX_RESULTS: usize = 1200;
/// Máximo de caracteres para preview de línea en resultados globales.
pub const WORKSPACE_TEXT_PREVIEW_MAX_CHARS: usize = 56;
/// Número máximo de resultados visibles en overlay.
pub const OVERLAY_RESULTS_MAX: usize = 10;
/// Máximo de caracteres que se insertan en chat al adjuntar una selección.
pub const CHAT_SELECTION_MAX_CHARS: usize = 8_000;
/// Intervalo de polling para detectar cambios externos en workspace.
pub const WORKSPACE_POLL_INTERVAL: Duration = Duration::from_millis(1200);
/// Límite de entradas para firma de workspace.
pub const WORKSPACE_SIGNATURE_MAX_ENTRIES: usize = 6000;
/// Profundidad máxima para firma de workspace.
pub const WORKSPACE_SIGNATURE_MAX_DEPTH: usize = 12;
/// Preset runtime: budget de tokens ajustado para respuesta rápida.
pub const RUNTIME_TOKEN_BUDGET_TIGHT: u32 = 60_000;
/// Preset runtime: budget de tokens por defecto.
pub const RUNTIME_TOKEN_BUDGET_BALANCED: u32 = 100_000;
/// Preset runtime: budget de tokens para deliberación profunda.
pub const RUNTIME_TOKEN_BUDGET_DEEP: u32 = 180_000;
/// Preset runtime: cap de paralelismo bajo.
pub const RUNTIME_PARALLEL_CAP_CONSERVATIVE: usize = 2;
/// Preset runtime: cap de paralelismo medio.
pub const RUNTIME_PARALLEL_CAP_BALANCED: usize = 3;
/// Preset runtime: cap de paralelismo alto.
pub const RUNTIME_PARALLEL_CAP_WIDE: usize = 5;
/// Preset runtime: escala worker por defecto.
pub const RUNTIME_WORKER_SCALE_BALANCED: f32 = 1.0;
/// Preset runtime: escala primary por defecto.
pub const RUNTIME_PRIMARY_SCALE_BALANCED: f32 = 1.0;
/// Preset runtime: favorece ruta worker y protege primary.
pub const RUNTIME_WORKER_SCALE_WORKER_BIAS: f32 = 0.85;
/// Preset runtime: favorece ruta worker y protege primary.
pub const RUNTIME_PRIMARY_SCALE_WORKER_BIAS: f32 = 1.15;
/// Preset runtime: facilita fallback temprano a primary.
pub const RUNTIME_WORKER_SCALE_PRIMARY_BIAS: f32 = 1.20;
/// Preset runtime: facilita fallback temprano a primary.
pub const RUNTIME_PRIMARY_SCALE_PRIMARY_BIAS: f32 = 0.85;
/// Límite de lectura de telemetría persistida por sesión.
pub const RUNTIME_TELEMETRY_FETCH_LIMIT: usize = 200;
/// Intervalo de prefetch automático de páginas persistidas en background.
pub const TELEMETRY_PERSISTED_PREFETCH_INTERVAL: Duration = Duration::from_secs(15);
/// Intervalo de polling para resumen Git del sidebar.
pub const GIT_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(6);
/// Intervalo de polling para health de quiron-brain.
pub const QUIRON_HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(20);
/// Variable de entorno para URL de quiron-brain.
pub const QUIRON_BRAIN_URL_ENV: &str = "QUIRON_BRAIN_URL";
/// Variable de entorno para token bearer directo.
pub const QUIRON_API_TOKEN_ENV: &str = "QUIRON_API_TOKEN";
/// Variable de entorno para ruta de archivo token (preferido en modo seguro).
pub const QUIRON_API_TOKEN_FILE_ENV: &str = "QUIRON_API_TOKEN_FILE";
/// Variable opcional para ruta del env file de quiron-brain.
pub const QUIRON_BRAIN_ENV_FILE_ENV: &str = "QUIRON_BRAIN_ENV_FILE";
/// Ruta por defecto del env file de quiron-brain (relativa a HOME).
pub const QUIRON_BRAIN_ENV_FILE_DEFAULT_REL: &str = ".config/quiron/quiron-brain.env";
/// Variable de entorno para activar/desactivar modo seguro de conexión.
pub const QUIRON_SECURE_MODE_ENV: &str = "QUIRON_SECURE_MODE";

/// Mensaje en el historial de chat
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// true = usuario, false = Quirón
    pub is_user: bool,
    /// Contenido del mensaje
    pub content: String,
    /// Metadatos cognitivos visibles en UI
    pub meta: Option<String>,
    /// Evidencias citadas por Quirón (clicables en la UI)
    pub citations: Vec<Citation>,
}

/// Menú superior activo en la barra principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopMenuKind {
    File,
    Edit,
    View,
    Go,
    Project,
    Help,
}

impl TopMenuKind {
    pub fn label(self) -> &'static str {
        match self {
            TopMenuKind::File => "File",
            TopMenuKind::Edit => "Edit",
            TopMenuKind::View => "View",
            TopMenuKind::Go => "Go",
            TopMenuKind::Project => "Project",
            TopMenuKind::Help => "Help",
        }
    }

    pub fn next(self) -> Self {
        match self {
            TopMenuKind::File => TopMenuKind::Edit,
            TopMenuKind::Edit => TopMenuKind::View,
            TopMenuKind::View => TopMenuKind::Go,
            TopMenuKind::Go => TopMenuKind::Project,
            TopMenuKind::Project => TopMenuKind::Help,
            TopMenuKind::Help => TopMenuKind::File,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            TopMenuKind::File => TopMenuKind::Help,
            TopMenuKind::Edit => TopMenuKind::File,
            TopMenuKind::View => TopMenuKind::Edit,
            TopMenuKind::Go => TopMenuKind::View,
            TopMenuKind::Project => TopMenuKind::Go,
            TopMenuKind::Help => TopMenuKind::Project,
        }
    }

    pub fn from_mnemonic(ch: &str) -> Option<Self> {
        match ch {
            "f" => Some(TopMenuKind::File),
            "e" => Some(TopMenuKind::Edit),
            "v" => Some(TopMenuKind::View),
            "g" => Some(TopMenuKind::Go),
            "p" => Some(TopMenuKind::Project),
            "h" => Some(TopMenuKind::Help),
            _ => None,
        }
    }
}

/// Acción asociada a una zona clicable en pantalla.
#[derive(Debug, Clone)]
pub enum ClickTargetAction {
    /// Abrir el selector de carpeta desde la pantalla de bienvenida.
    WelcomeOpenFolder,
    /// Abrir un proyecto reciente desde la pantalla de bienvenida.
    WelcomeOpenRecent(PathBuf),
    Citation(Citation),
    ExplorerFile(PathBuf),
    ExplorerDir(PathBuf),
    BreadcrumbSegment {
        pane: EditorPane,
        path: PathBuf,
        is_file: bool,
    },
    TabSelect {
        pane: EditorPane,
        index: usize,
    },
    SearchMatchSelect(usize),
    SidebarProblemSelect(usize),
    OverlayItemSelect(usize),
    ActivityFocusExplorer,
    ActivityQuickOpen,
    /// Vacía el hilo y empieza una conversación limpia.
    NewChat,
    /// Pliega o despliega el bloque de pensamiento del mensaje dado.
    ToggleThought(usize),
    ActivityCommandPalette,
    ActivityToggleTelemetry,
    ActivityCycleTheme,
    ActivityCycleDensity,
    ActivitySetTheme(UiTheme),
    ActivitySetDensity(UiDensity),
    ActivityApplyAppearancePreset(UiAppearancePreset),
    ActivityIncreaseEditorFontScale,
    ActivityDecreaseEditorFontScale,
    ActivityResetEditorFontScale,
    ActivityResetAppearance,
    ActivityIncreaseEditorHorizontalPadding,
    ActivityDecreaseEditorHorizontalPadding,
    ActivityResetEditorHorizontalPadding,
    ActivityIncreaseExplorerIndent,
    ActivityDecreaseExplorerIndent,
    ActivityResetExplorerIndent,
    ActivityExplorerNewFile,
    ActivityExplorerNewFolder,
    ActivityRefreshExplorer,
    ActivityRevealActiveFile,
    ActivityAddSelectionToChat,
    ActivityCycleAiModel,
    TopMenuToggle(TopMenuKind),
    TopMenuExecute(CommandPaletteAction),
    ActivityFocusChat,
    ActivitySidebarSearch,
    ActivitySidebarGit,
    ActivitySidebarProblems,
    ActivitySidebarOutline,
    ActivitySidebarAppearance,
    ActivitySidebarSecurity,
    OutlineSelect {
        pane: EditorPane,
        line: usize,
        column: usize,
    },
}

/// Panel de editor al que aplica una interacción.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPane {
    Primary,
    Secondary,
}

/// Superficies de foco de la aplicación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    EditorPrimary,
    EditorSecondary,
    ChatInput,
}

/// Región clicable en pantalla.
#[derive(Debug, Clone)]
pub struct ClickTarget {
    pub bounds: Bounds,
    pub action: ClickTargetAction,
}

/// Entrada del árbol de explorador de archivos.
#[derive(Debug, Clone)]
pub struct ExplorerEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
}

/// Coincidencia de búsqueda en el buffer activo.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}

/// Scope de offsets para operaciones de replace limitadas a selección.
#[derive(Debug, Clone, Copy)]
pub struct SearchScope {
    pub start_offset: usize,
    pub end_offset: usize,
}

/// Campo activo durante edición de la barra Find/Replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchInputFocus {
    Find,
    Replace,
}

/// Panel activo en el sidebar lateral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPanel {
    Explorer,
    Search,
    Git,
    Problems,
    Outline,
    Appearance,
    Security,
}

/// Lado de acoplamiento de un panel principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelDock {
    Left,
    Right,
}

impl PanelDock {
    fn config_value(self) -> &'static str {
        match self {
            PanelDock::Left => "left",
            PanelDock::Right => "right",
        }
    }

    fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "left" => Some(PanelDock::Left),
            "right" => Some(PanelDock::Right),
            _ => None,
        }
    }
}

impl SidebarPanel {
    fn config_value(self) -> &'static str {
        match self {
            SidebarPanel::Explorer => "explorer",
            SidebarPanel::Search => "search",
            SidebarPanel::Git => "git",
            SidebarPanel::Problems => "problems",
            SidebarPanel::Outline => "outline",
            SidebarPanel::Appearance => "appearance",
            SidebarPanel::Security => "security",
        }
    }

    fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "explorer" => Some(SidebarPanel::Explorer),
            "search" => Some(SidebarPanel::Search),
            "git" => Some(SidebarPanel::Git),
            "problems" => Some(SidebarPanel::Problems),
            "outline" => Some(SidebarPanel::Outline),
            "appearance" => Some(SidebarPanel::Appearance),
            "security" => Some(SidebarPanel::Security),
            _ => None,
        }
    }
}

/// Densidad visual global de la UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiDensity {
    Compact,
    Normal,
    Comfortable,
}

impl UiDensity {
    pub fn config_value(self) -> &'static str {
        match self {
            UiDensity::Compact => "compact",
            UiDensity::Normal => "normal",
            UiDensity::Comfortable => "comfortable",
        }
    }

    fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "compact" => Some(UiDensity::Compact),
            "normal" => Some(UiDensity::Normal),
            "comfortable" => Some(UiDensity::Comfortable),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            UiDensity::Compact => "Compact",
            UiDensity::Normal => "Normal",
            UiDensity::Comfortable => "Comfortable",
        }
    }

    fn next(self) -> Self {
        match self {
            UiDensity::Compact => UiDensity::Normal,
            UiDensity::Normal => UiDensity::Comfortable,
            UiDensity::Comfortable => UiDensity::Compact,
        }
    }

    fn scale(self) -> f32 {
        match self {
            UiDensity::Compact => 0.90,
            UiDensity::Normal => 1.0,
            UiDensity::Comfortable => 1.12,
        }
    }
}

/// Preset de apariencia para aplicar configuraciones de UI de una vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAppearancePreset {
    Dev,
    Focus,
    Reading,
}

impl UiAppearancePreset {
    fn label(self) -> &'static str {
        match self {
            UiAppearancePreset::Dev => "Dev",
            UiAppearancePreset::Focus => "Focus",
            UiAppearancePreset::Reading => "Reading",
        }
    }
}

/// Severidad de problema en panel lateral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarProblemSeverity {
    Error,
    Warning,
    Info,
}

/// Entrada de problema operacional para panel lateral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarProblem {
    pub severity: SidebarProblemSeverity,
    pub title: String,
    pub detail: String,
}

/// Entrada de símbolo/outline del archivo activo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarOutlineItem {
    pub line: usize,
    pub column: usize,
    pub kind: String,
    pub label: String,
}

/// Snapshot resumido de `git status --porcelain --branch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSidebarStatus {
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub total: usize,
}

/// Origen de credenciales usadas para conectar con quiron-brain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuironAuthSource {
    None,
    EnvToken,
    FileToken,
}

impl QuironAuthSource {
    fn short_label(self) -> &'static str {
        match self {
            QuironAuthSource::None => "none",
            QuironAuthSource::EnvToken => "env",
            QuironAuthSource::FileToken => "file",
        }
    }
}

/// Overlay activo sobre la UI principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    QuickOpen,
    CommandPalette,
    GoToLine,
    Symbols,
    WorkspaceSymbols,
    WorkspaceTextSearch,
    Problems,
}

/// Acción ejecutable desde command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPaletteAction {
    NewTab,
    NewWindow,
    OpenFilePicker,
    OpenFolderPicker,
    QuickOpen,
    GoToLine,
    FindInWorkspace,
    GoToSymbol,
    GoToWorkspaceSymbol,
    ShowProblems,
    ShowProblemsEditor,
    ShowProblemsSearch,
    ShowProblemsGit,
    ShowProblemsTelemetry,
    ShowProblemsRuntime,
    ShowAppearancePanel,
    ShowExplorerPanel,
    ShowSearchPanel,
    ShowGitPanel,
    ShowOutlinePanel,
    ShowSecurityPanel,
    NextProblem,
    PreviousProblem,
    OpenToSide,
    ResetLayout,
    ToggleEditorSplit,
    MoveExplorerLeft,
    MoveExplorerRight,
    MoveChatLeft,
    MoveChatRight,
    CycleAiModel,
    ToggleTelemetryPanel,
    CycleTelemetryTimelineFilter,
    RefreshTelemetryPersistedCache,
    LoadMoreTelemetryPersistedCache,
    Save,
    SaveAs,
    CloseTab,
    Find,
    Replace,
    AddSelectionToChat,
    ExplorerNewFile,
    ExplorerNewFolder,
    RefreshExplorer,
    RevealActiveFile,
    FocusEditor,
    FocusChat,
    NextTab,
    PreviousTab,
    SetThemeQuironDark,
    SetThemeGraphiteDark,
    SetThemeCopperLight,
    SetThemeModernistLight,
    NewChat,
    CycleThemeNext,
    CycleThemePrevious,
    SetDensityCompact,
    SetDensityNormal,
    SetDensityComfortable,
    CycleDensityNext,
    ApplyAppearancePresetDev,
    ApplyAppearancePresetFocus,
    ApplyAppearancePresetReading,
    ResetAppearance,
    IncreaseEditorFontScale,
    DecreaseEditorFontScale,
    ResetEditorFontScale,
    /// Agranda la interfaz entera: letras y huecos.
    IncreaseUiScale,
    /// Encoge la interfaz entera.
    DecreaseUiScale,
    /// Devuelve la interfaz a la escala natural del monitor.
    ResetUiScale,
    IncreaseEditorHorizontalPadding,
    DecreaseEditorHorizontalPadding,
    ResetEditorHorizontalPadding,
    IncreaseExplorerIndent,
    DecreaseExplorerIndent,
    ResetExplorerIndent,
    SetRuntimeBudgetTight,
    SetRuntimeBudgetBalanced,
    SetRuntimeBudgetDeep,
    SetRuntimeThresholdBalanced,
    SetRuntimeThresholdWorkerBias,
    SetRuntimeThresholdPrimaryBias,
    SetRuntimeParallelConservative,
    SetRuntimeParallelBalanced,
    SetRuntimeParallelWide,
    ShowSessionTelemetryStatus,
    ShowSessionTelemetryReport,
    ReconnectQuironSecureLocal,
    ReconnectQuironSecureEnv,
    ShowQuironConnectionStatus,
}

/// Entrada visible dentro de un menú superior.
#[derive(Debug, Clone, Copy)]
pub struct TopMenuEntry {
    pub label: &'static str,
    pub action: CommandPaletteAction,
}

const TOP_MENU_FILE_ENTRIES: &[TopMenuEntry] = &[
    TopMenuEntry {
        label: "New Window",
        action: CommandPaletteAction::NewWindow,
    },
    TopMenuEntry {
        label: "New Tab",
        action: CommandPaletteAction::NewTab,
    },
    TopMenuEntry {
        label: "Open File...",
        action: CommandPaletteAction::OpenFilePicker,
    },
    TopMenuEntry {
        label: "Open Folder...",
        action: CommandPaletteAction::OpenFolderPicker,
    },
    TopMenuEntry {
        label: "Save",
        action: CommandPaletteAction::Save,
    },
    TopMenuEntry {
        label: "Save As...",
        action: CommandPaletteAction::SaveAs,
    },
    TopMenuEntry {
        label: "Close Tab",
        action: CommandPaletteAction::CloseTab,
    },
];

const TOP_MENU_EDIT_ENTRIES: &[TopMenuEntry] = &[
    TopMenuEntry {
        label: "Add Selection to Chat",
        action: CommandPaletteAction::AddSelectionToChat,
    },
    TopMenuEntry {
        label: "Find",
        action: CommandPaletteAction::Find,
    },
    TopMenuEntry {
        label: "Replace",
        action: CommandPaletteAction::Replace,
    },
    TopMenuEntry {
        label: "Next Tab",
        action: CommandPaletteAction::NextTab,
    },
    TopMenuEntry {
        label: "Previous Tab",
        action: CommandPaletteAction::PreviousTab,
    },
];

const TOP_MENU_VIEW_ENTRIES: &[TopMenuEntry] = &[
    TopMenuEntry {
        label: "Appearance Panel",
        action: CommandPaletteAction::ShowAppearancePanel,
    },
    TopMenuEntry {
        label: "Toggle Split",
        action: CommandPaletteAction::ToggleEditorSplit,
    },
    TopMenuEntry {
        label: "Move Explorer Left",
        action: CommandPaletteAction::MoveExplorerLeft,
    },
    TopMenuEntry {
        label: "Move Explorer Right",
        action: CommandPaletteAction::MoveExplorerRight,
    },
    TopMenuEntry {
        label: "Move Chat Left",
        action: CommandPaletteAction::MoveChatLeft,
    },
    TopMenuEntry {
        label: "Move Chat Right",
        action: CommandPaletteAction::MoveChatRight,
    },
    TopMenuEntry {
        label: "Cycle AI Model",
        action: CommandPaletteAction::CycleAiModel,
    },
    TopMenuEntry {
        label: "Toggle Telemetry",
        action: CommandPaletteAction::ToggleTelemetryPanel,
    },
    TopMenuEntry {
        label: "Reset Layout",
        action: CommandPaletteAction::ResetLayout,
    },
];

const TOP_MENU_GO_ENTRIES: &[TopMenuEntry] = &[
    TopMenuEntry {
        label: "Quick Open",
        action: CommandPaletteAction::QuickOpen,
    },
    TopMenuEntry {
        label: "Go to Line",
        action: CommandPaletteAction::GoToLine,
    },
    TopMenuEntry {
        label: "Go to Symbol",
        action: CommandPaletteAction::GoToSymbol,
    },
    TopMenuEntry {
        label: "Go to Workspace Symbol",
        action: CommandPaletteAction::GoToWorkspaceSymbol,
    },
    TopMenuEntry {
        label: "Show Problems",
        action: CommandPaletteAction::ShowProblems,
    },
];

const TOP_MENU_PROJECT_ENTRIES: &[TopMenuEntry] = &[
    TopMenuEntry {
        label: "New File",
        action: CommandPaletteAction::ExplorerNewFile,
    },
    TopMenuEntry {
        label: "New Folder",
        action: CommandPaletteAction::ExplorerNewFolder,
    },
    TopMenuEntry {
        label: "Refresh Explorer",
        action: CommandPaletteAction::RefreshExplorer,
    },
    TopMenuEntry {
        label: "Reveal Active File",
        action: CommandPaletteAction::RevealActiveFile,
    },
    TopMenuEntry {
        label: "Find in Workspace",
        action: CommandPaletteAction::FindInWorkspace,
    },
];

const TOP_MENU_HELP_ENTRIES: &[TopMenuEntry] = &[
    TopMenuEntry {
        label: "Command Palette",
        action: CommandPaletteAction::QuickOpen,
    },
    TopMenuEntry {
        label: "Secure Reconnect (Local)",
        action: CommandPaletteAction::ReconnectQuironSecureLocal,
    },
    TopMenuEntry {
        label: "Connection Status",
        action: CommandPaletteAction::ShowQuironConnectionStatus,
    },
    TopMenuEntry {
        label: "Session Telemetry Status",
        action: CommandPaletteAction::ShowSessionTelemetryStatus,
    },
    TopMenuEntry {
        label: "Session Telemetry Report",
        action: CommandPaletteAction::ShowSessionTelemetryReport,
    },
];

/// Devuelve entradas del menú superior solicitado.
pub fn top_menu_entries(menu: TopMenuKind) -> &'static [TopMenuEntry] {
    match menu {
        TopMenuKind::File => TOP_MENU_FILE_ENTRIES,
        TopMenuKind::Edit => TOP_MENU_EDIT_ENTRIES,
        TopMenuKind::View => TOP_MENU_VIEW_ENTRIES,
        TopMenuKind::Go => TOP_MENU_GO_ENTRIES,
        TopMenuKind::Project => TOP_MENU_PROJECT_ENTRIES,
        TopMenuKind::Help => TOP_MENU_HELP_ENTRIES,
    }
}

/// Acción interna de cada item de overlay.
#[derive(Debug, Clone)]
pub enum OverlayAction {
    OpenFile(PathBuf),
    OpenFileAtLocation {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    JumpToSymbol {
        pane: EditorPane,
        line: usize,
        column: usize,
    },
    JumpToLine {
        pane: EditorPane,
        line: usize,
        column: usize,
    },
    ProblemSelect {
        title: String,
        index: usize,
    },
    Command(CommandPaletteAction),
}

/// Item renderizable en quick open / command palette.
#[derive(Debug, Clone)]
pub struct OverlayItem {
    pub title: String,
    pub detail: String,
    pub action: OverlayAction,
}

#[derive(Debug, Clone)]
struct WorkspaceSymbolCandidate {
    path: PathBuf,
    line: usize,
    column: usize,
    kind: String,
    label: String,
}

#[derive(Debug, Clone, Copy)]
struct CommandDescriptor {
    action: CommandPaletteAction,
    label: &'static str,
    detail: &'static str,
    keywords: &'static str,
}

const COMMAND_DESCRIPTORS: &[CommandDescriptor] = &[
    CommandDescriptor {
        action: CommandPaletteAction::NewWindow,
        label: "New Window",
        detail: "Open this workspace in an independent window",
        keywords: "new window workspace ctrl shift n",
    },
    CommandDescriptor {
        action: CommandPaletteAction::NewTab,
        label: "New Tab",
        detail: "Create a new tab",
        keywords: "new tab create file",
    },
    CommandDescriptor {
        action: CommandPaletteAction::OpenFilePicker,
        label: "Open File...",
        detail: "Open native file picker",
        keywords: "open file picker dialog",
    },
    CommandDescriptor {
        action: CommandPaletteAction::QuickOpen,
        label: "Quick Open",
        detail: "Search files in workspace",
        keywords: "quick open ctrl p file",
    },
    CommandDescriptor {
        action: CommandPaletteAction::GoToLine,
        label: "Go to Line",
        detail: "Jump cursor to line/column in active editor",
        keywords: "go to line ctrl g line column jump",
    },
    CommandDescriptor {
        action: CommandPaletteAction::FindInWorkspace,
        label: "Find in Workspace",
        detail: "Search text across workspace files",
        keywords: "find workspace project search ctrl shift f grep",
    },
    CommandDescriptor {
        action: CommandPaletteAction::GoToSymbol,
        label: "Go to Symbol",
        detail: "Search symbols in active file",
        keywords: "go to symbol outline ctrl shift o",
    },
    CommandDescriptor {
        action: CommandPaletteAction::GoToWorkspaceSymbol,
        label: "Go to Workspace Symbol",
        detail: "Search symbols across workspace files",
        keywords: "go to workspace symbol global project",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowProblems,
        label: "Show Problems",
        detail: "Open problems list overlay",
        keywords: "show problems list diagnostics issues",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowProblemsEditor,
        label: "Show Problems: Editor",
        detail: "Filter problems to editor context",
        keywords: "show problems editor unsaved tabs exit confirm",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowProblemsSearch,
        label: "Show Problems: Search",
        detail: "Filter problems to search context",
        keywords: "show problems search find replace no matches",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowProblemsGit,
        label: "Show Problems: Git",
        detail: "Filter problems to git context",
        keywords: "show problems git status repository",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowProblemsTelemetry,
        label: "Show Problems: Telemetry",
        detail: "Filter problems to telemetry context",
        keywords: "show problems telemetry persisted sync",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowProblemsRuntime,
        label: "Show Problems: Runtime",
        detail: "Filter problems to runtime/request context",
        keywords: "show problems runtime request loading status",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowAppearancePanel,
        label: "Show Appearance Panel",
        detail: "Open sidebar appearance controls",
        keywords: "show appearance panel sidebar theme density font",
    },
    // Hasta aquí, la barra de actividad era la única puerta a estos cinco
    // paneles: ni el menú View ni la paleta los ofrecían. Al retirar esa barra
    // habrían quedado inalcanzables, así que la paleta pasa a ser su entrada.
    CommandDescriptor {
        action: CommandPaletteAction::ShowExplorerPanel,
        label: "Show Explorer Panel",
        detail: "Open sidebar file explorer",
        keywords: "show explorer panel sidebar files tree folder",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowSearchPanel,
        label: "Show Search Panel",
        detail: "Open sidebar workspace search",
        keywords: "show search panel sidebar find grep workspace",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowGitPanel,
        label: "Show Git Panel",
        detail: "Open sidebar source control",
        keywords: "show git panel sidebar source control status diff",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowOutlinePanel,
        label: "Show Outline Panel",
        detail: "Open sidebar symbol outline",
        keywords: "show outline panel sidebar symbols structure",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowSecurityPanel,
        label: "Show Security Panel",
        detail: "Open sidebar workspace confinement status",
        keywords: "show security panel sidebar workspace guard confinement",
    },
    CommandDescriptor {
        action: CommandPaletteAction::NextProblem,
        label: "Next Problem",
        detail: "Navigate to next operational problem",
        keywords: "next problem diagnostic f8",
    },
    CommandDescriptor {
        action: CommandPaletteAction::PreviousProblem,
        label: "Previous Problem",
        detail: "Navigate to previous operational problem",
        keywords: "previous problem diagnostic shift f8",
    },
    CommandDescriptor {
        action: CommandPaletteAction::OpenToSide,
        label: "Open to Side",
        detail: "Open selected/active file in secondary pane",
        keywords: "open to side split secondary pane right",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ResetLayout,
        label: "Reset Layout",
        detail: "Restore default panel sizes",
        keywords: "reset layout split sidebar panels",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ToggleEditorSplit,
        label: "Toggle Editor Split",
        detail: "Switch between single and split editor",
        keywords: "toggle split editor dual pane",
    },
    CommandDescriptor {
        action: CommandPaletteAction::MoveExplorerLeft,
        label: "View: Move Explorer Left",
        detail: "Dock the activity bar and explorer on the left",
        keywords: "view move explorer sidebar left dock",
    },
    CommandDescriptor {
        action: CommandPaletteAction::MoveExplorerRight,
        label: "View: Move Explorer Right",
        detail: "Dock the activity bar and explorer on the right",
        keywords: "view move explorer sidebar right dock",
    },
    CommandDescriptor {
        action: CommandPaletteAction::MoveChatLeft,
        label: "View: Move Chat Left",
        detail: "Dock chat to the left of the editor",
        keywords: "view move chat left dock",
    },
    CommandDescriptor {
        action: CommandPaletteAction::MoveChatRight,
        label: "View: Move Chat Right",
        detail: "Dock chat to the right of the editor",
        keywords: "view move chat right dock",
    },
    CommandDescriptor {
        action: CommandPaletteAction::CycleAiModel,
        label: "AI: Select Next Model",
        detail: "Cycle models without changing the connection",
        keywords: "ai model gpt select cycle",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ToggleTelemetryPanel,
        label: "Toggle Telemetry Panel",
        detail: "Show/hide live telemetry box in chat",
        keywords: "toggle telemetry panel chat",
    },
    CommandDescriptor {
        action: CommandPaletteAction::CycleTelemetryTimelineFilter,
        label: "Telemetry: Cycle Timeline Filter",
        detail: "Switch timeline filter all/checkpoints/anomalies",
        keywords: "telemetry timeline filter all checkpoints anomalies",
    },
    CommandDescriptor {
        action: CommandPaletteAction::RefreshTelemetryPersistedCache,
        label: "Telemetry: Sync Persisted Cache",
        detail: "Fetch persisted session telemetry into timeline cache",
        keywords: "telemetry sync persisted cache refresh",
    },
    CommandDescriptor {
        action: CommandPaletteAction::LoadMoreTelemetryPersistedCache,
        label: "Telemetry: Load Older Persisted",
        detail: "Load older persisted telemetry page into timeline",
        keywords: "telemetry load older page persisted timeline",
    },
    CommandDescriptor {
        action: CommandPaletteAction::Save,
        label: "Save",
        detail: "Save active file",
        keywords: "save write ctrl s",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SaveAs,
        label: "Save As...",
        detail: "Save active file as",
        keywords: "save as write picker",
    },
    CommandDescriptor {
        action: CommandPaletteAction::CloseTab,
        label: "Close Tab",
        detail: "Close active tab",
        keywords: "close tab ctrl w",
    },
    CommandDescriptor {
        action: CommandPaletteAction::Find,
        label: "Find",
        detail: "Open search mode",
        keywords: "find search ctrl f",
    },
    CommandDescriptor {
        action: CommandPaletteAction::Replace,
        label: "Replace",
        detail: "Open replace mode",
        keywords: "replace ctrl h",
    },
    CommandDescriptor {
        action: CommandPaletteAction::AddSelectionToChat,
        label: "Chat: Add Selection as Context",
        detail: "Append selected editor text into chat composer",
        keywords: "chat add selection context include quote editor ctrl shift i",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ExplorerNewFile,
        label: "Explorer: New File",
        detail: "Create a new file in selected folder",
        keywords: "explorer new file create folder",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ExplorerNewFolder,
        label: "Explorer: New Folder",
        detail: "Create a new folder in selected folder",
        keywords: "explorer new folder create directory",
    },
    CommandDescriptor {
        action: CommandPaletteAction::RefreshExplorer,
        label: "Refresh Explorer",
        detail: "Reload file tree",
        keywords: "refresh explorer reload",
    },
    CommandDescriptor {
        action: CommandPaletteAction::RevealActiveFile,
        label: "Reveal Active File",
        detail: "Select active file in explorer",
        keywords: "reveal active file explorer",
    },
    CommandDescriptor {
        action: CommandPaletteAction::FocusEditor,
        label: "Focus Editor",
        detail: "Move focus to editor panel",
        keywords: "focus editor",
    },
    CommandDescriptor {
        action: CommandPaletteAction::FocusChat,
        label: "Focus Chat",
        detail: "Move focus to chat input",
        keywords: "focus chat input",
    },
    CommandDescriptor {
        action: CommandPaletteAction::NextTab,
        label: "Next Tab",
        detail: "Cycle to next tab",
        keywords: "next tab cycle",
    },
    CommandDescriptor {
        action: CommandPaletteAction::PreviousTab,
        label: "Previous Tab",
        detail: "Cycle to previous tab",
        keywords: "previous tab cycle",
    },
    // Era la única de las 35 acciones de los menús superiores que la paleta no
    // ofrecía. Sin ella, retirar la barra de menús dejaría al usuario sin forma
    // de abrir una carpeta.
    CommandDescriptor {
        action: CommandPaletteAction::NewChat,
        label: "New Chat",
        detail: "Clear the conversation and start fresh",
        keywords: "new chat clear conversation nuevo limpiar",
    },
    CommandDescriptor {
        action: CommandPaletteAction::OpenFolderPicker,
        label: "Open Folder...",
        detail: "Choose a project folder to open",
        keywords: "open folder project workspace abrir carpeta",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetThemeQuironDark,
        label: "Theme: Quiron Dark",
        detail: "Set dark blue Quiron palette",
        keywords: "theme quiron dark appearance",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetThemeGraphiteDark,
        label: "Theme: Graphite Dark",
        detail: "Set neutral graphite dark palette",
        keywords: "theme graphite dark zed inspired appearance",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetThemeCopperLight,
        label: "Theme: Copper Light",
        detail: "Set warm light palette",
        keywords: "theme copper light appearance",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetThemeModernistLight,
        label: "Theme: Modernist Light",
        detail: "Set warm light palette with navy accent",
        keywords: "theme modernist light navy redesign appearance",
    },
    CommandDescriptor {
        action: CommandPaletteAction::CycleThemeNext,
        label: "Theme: Next",
        detail: "Cycle to next visual theme",
        keywords: "theme next cycle appearance",
    },
    CommandDescriptor {
        action: CommandPaletteAction::CycleThemePrevious,
        label: "Theme: Previous",
        detail: "Cycle to previous visual theme",
        keywords: "theme previous cycle appearance",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetDensityCompact,
        label: "Appearance: Density Compact",
        detail: "Set compact UI spacing",
        keywords: "appearance density compact spacing",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetDensityNormal,
        label: "Appearance: Density Normal",
        detail: "Set normal UI spacing",
        keywords: "appearance density normal spacing default",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetDensityComfortable,
        label: "Appearance: Density Comfortable",
        detail: "Set comfortable UI spacing",
        keywords: "appearance density comfortable spacing",
    },
    CommandDescriptor {
        action: CommandPaletteAction::CycleDensityNext,
        label: "Appearance: Density Next",
        detail: "Cycle compact/normal/comfortable",
        keywords: "appearance density cycle next",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ApplyAppearancePresetDev,
        label: "Appearance Preset: Dev",
        detail: "Graphite + compact + 95% font",
        keywords: "appearance preset dev graphite compact font",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ApplyAppearancePresetFocus,
        label: "Appearance Preset: Focus",
        detail: "Quiron + normal + 110% font",
        keywords: "appearance preset focus quiron normal font",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ApplyAppearancePresetReading,
        label: "Appearance Preset: Reading",
        detail: "Copper + comfortable + 120% font",
        keywords: "appearance preset reading copper comfortable font",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ResetAppearance,
        label: "Appearance: Reset All",
        detail: "Reset theme, density, font and spacing",
        keywords: "appearance reset all theme density font spacing",
    },
    CommandDescriptor {
        action: CommandPaletteAction::IncreaseUiScale,
        label: "Appearance: Increase Interface Scale",
        detail: "Agranda toda la interfaz: letras y espaciados",
        keywords: "appearance ui interface scale zoom increase agrandar escala",
    },
    CommandDescriptor {
        action: CommandPaletteAction::DecreaseUiScale,
        label: "Appearance: Decrease Interface Scale",
        detail: "Encoge toda la interfaz: letras y espaciados",
        keywords: "appearance ui interface scale zoom decrease encoger escala",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ResetUiScale,
        label: "Appearance: Reset Interface Scale",
        detail: "Vuelve a la escala natural del monitor",
        keywords: "appearance ui interface scale reset restablecer escala",
    },
    CommandDescriptor {
        action: CommandPaletteAction::IncreaseEditorFontScale,
        label: "Appearance: Increase Editor Font",
        detail: "Increase code font and line spacing",
        keywords: "appearance editor font size increase zoom in",
    },
    CommandDescriptor {
        action: CommandPaletteAction::DecreaseEditorFontScale,
        label: "Appearance: Decrease Editor Font",
        detail: "Decrease code font and line spacing",
        keywords: "appearance editor font size decrease zoom out",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ResetEditorFontScale,
        label: "Appearance: Reset Editor Font",
        detail: "Restore editor font scale to 100%",
        keywords: "appearance editor font size reset default 100",
    },
    CommandDescriptor {
        action: CommandPaletteAction::IncreaseEditorHorizontalPadding,
        label: "Appearance: Increase Editor Left Padding",
        detail: "Increase horizontal distance between gutter and text",
        keywords: "appearance editor horizontal left padding spacing increase",
    },
    CommandDescriptor {
        action: CommandPaletteAction::DecreaseEditorHorizontalPadding,
        label: "Appearance: Decrease Editor Left Padding",
        detail: "Decrease horizontal distance between gutter and text",
        keywords: "appearance editor horizontal left padding spacing decrease",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ResetEditorHorizontalPadding,
        label: "Appearance: Reset Editor Left Padding",
        detail: "Restore editor left padding to default",
        keywords: "appearance editor horizontal left padding reset default",
    },
    CommandDescriptor {
        action: CommandPaletteAction::IncreaseExplorerIndent,
        label: "Appearance: Increase Explorer Indent",
        detail: "Increase tree indentation in explorer",
        keywords: "appearance explorer indent spacing increase tree",
    },
    CommandDescriptor {
        action: CommandPaletteAction::DecreaseExplorerIndent,
        label: "Appearance: Decrease Explorer Indent",
        detail: "Decrease tree indentation in explorer",
        keywords: "appearance explorer indent spacing decrease tree",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ResetExplorerIndent,
        label: "Appearance: Reset Explorer Indent",
        detail: "Restore explorer indentation to default",
        keywords: "appearance explorer indent spacing reset default",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeBudgetTight,
        label: "Runtime Budget: Tight",
        detail: "Set token budget to 60k",
        keywords: "runtime budget tight tokens 60000 fast",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeBudgetBalanced,
        label: "Runtime Budget: Balanced",
        detail: "Set token budget to 100k",
        keywords: "runtime budget balanced tokens 100000 default",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeBudgetDeep,
        label: "Runtime Budget: Deep",
        detail: "Set token budget to 180k",
        keywords: "runtime budget deep tokens 180000 long",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeThresholdBalanced,
        label: "Runtime Thresholds: Balanced",
        detail: "Set threshold scales to 1.00 / 1.00",
        keywords: "runtime thresholds balanced worker primary",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeThresholdWorkerBias,
        label: "Runtime Thresholds: Worker Bias",
        detail: "Favor worker route, preserve primary reserve",
        keywords: "runtime thresholds worker bias reserve primary",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeThresholdPrimaryBias,
        label: "Runtime Thresholds: Primary Bias",
        detail: "Favor early fallback to primary route",
        keywords: "runtime thresholds primary bias fallback",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeParallelConservative,
        label: "Runtime Parallel Cap: 2",
        detail: "Set max planner subtasks to 2",
        keywords: "runtime parallel cap 2 conservative",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeParallelBalanced,
        label: "Runtime Parallel Cap: 3",
        detail: "Set max planner subtasks to 3",
        keywords: "runtime parallel cap 3 balanced default",
    },
    CommandDescriptor {
        action: CommandPaletteAction::SetRuntimeParallelWide,
        label: "Runtime Parallel Cap: 5",
        detail: "Set max planner subtasks to 5",
        keywords: "runtime parallel cap 5 wide",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowSessionTelemetryStatus,
        label: "Telemetry: Session Status",
        detail: "Show live + persisted telemetry counters",
        keywords: "telemetry session status checkpoints anomalies persisted",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowSessionTelemetryReport,
        label: "Telemetry: Session Report",
        detail: "Post latest persisted session telemetry summary",
        keywords: "telemetry session report latest checkpoint anomaly detail",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ReconnectQuironSecureLocal,
        label: "Connection: Secure Reconnect (Local)",
        detail: "Reconnect to localhost:8766 in secure mode",
        keywords: "connection secure reconnect local localhost quiron brain",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ReconnectQuironSecureEnv,
        label: "Connection: Secure Reconnect (Env URL)",
        detail: "Reconnect using QUIRON_BRAIN_URL in secure mode",
        keywords: "connection secure reconnect env url quiron_brain_url",
    },
    CommandDescriptor {
        action: CommandPaletteAction::ShowQuironConnectionStatus,
        label: "Connection: Show Status",
        detail: "Post active quiron connection details in chat",
        keywords: "connection status secure auth health token",
    },
];

/// Pestaña abierta en el editor central.
pub struct OpenTab {
    pub path: Option<PathBuf>,
    pub editor: Editor,
}

/// Snapshot liviano para render de tabs en la UI.
#[derive(Debug, Clone)]
pub struct TabSnapshot {
    pub index: usize,
    pub title: String,
    pub modified: bool,
    pub active: bool,
}

/// Zona de hit-test para tabs renderizadas en un panel de editor.
#[derive(Debug, Clone, Copy)]
pub struct TabHitbox {
    pub pane: EditorPane,
    pub index: usize,
    pub bounds: Bounds,
}

/// Estado temporal de drag/reorden de tabs.
#[derive(Debug, Clone, Copy)]
pub struct TabDragState {
    pub pane: EditorPane,
    pub index: usize,
    pub press_x: f32,
    pub press_y: f32,
    pub moved: bool,
}

/// Resumen liviano de telemetría para render de panel en chat.
#[derive(Debug, Clone, Default)]
pub struct SessionTelemetryPanelInfo {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub segment_seq: u32,
    pub checkpoints: usize,
    pub anomalies: usize,
    pub samples: usize,
    pub model_tokens_used_total: u32,
    pub token_budget: u32,
    pub token_budget_remaining: u32,
    pub fallback_rate: f32,
    pub last_checkpoint_step_start: Option<u64>,
    pub last_checkpoint_step_end: Option<u64>,
    pub last_checkpoint_flags: Vec<String>,
    pub last_anomaly_kind: Option<String>,
    pub last_anomaly_step: Option<u64>,
    pub persisted_checkpoints: usize,
    pub persisted_anomalies: usize,
    pub persisted_checkpoints_total: usize,
    pub persisted_anomalies_total: usize,
    pub persisted_sync: String,
    pub timeline: Vec<SessionTelemetryTimelineEntry>,
}

/// Filtro activo del timeline de telemetría visual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TelemetryTimelineFilter {
    #[default]
    All,
    Checkpoints,
    Anomalies,
}

impl TelemetryTimelineFilter {
    fn next(self) -> Self {
        match self {
            TelemetryTimelineFilter::All => TelemetryTimelineFilter::Checkpoints,
            TelemetryTimelineFilter::Checkpoints => TelemetryTimelineFilter::Anomalies,
            TelemetryTimelineFilter::Anomalies => TelemetryTimelineFilter::All,
        }
    }
}

/// Entrada de timeline para render compacta en el panel de chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionTelemetryTimelineSource {
    Local,
    Persisted,
}

/// Entrada de timeline para render compacta en el panel de chat.
#[derive(Debug, Clone)]
pub struct SessionTelemetryTimelineEntry {
    pub is_anomaly: bool,
    pub source: SessionTelemetryTimelineSource,
    pub segment_seq: u32,
    pub step: u64,
    pub label: String,
}

/// Estado compartido de la aplicación
pub struct AppState {
    pub window_width: u32,
    pub window_height: u32,
    /// Si necesita re-render
    pub needs_render: bool,
    /// Estado de modificadores de teclado
    pub modifiers: ModifiersState,
    /// Proxy para enviar eventos al event loop principal
    pub event_proxy: Option<EventLoopProxy<AppEvent>>,
    /// Sistema de texto
    pub text_system: TextSystem,
    /// Motor de layout
    pub layout_engine: LayoutEngine,
    /// Texto del input actual
    pub input_text: String,
    /// Si hay input activo
    pub input_focused: bool,
    /// Si el panel editor tiene foco
    pub editor_focused: bool,
    /// Historial de mensajes del chat
    pub messages: Vec<ChatMessage>,
    /// Pestañas abiertas del editor
    pub open_tabs: Vec<OpenTab>,
    /// Índice de pestaña activa del panel primario.
    pub active_tab: usize,
    /// Índice de pestaña activa del panel secundario (si split está activo).
    pub secondary_tab: Option<usize>,
    /// Si el editor está en modo split (dos paneles visibles).
    pub editor_pane_split_enabled: bool,
    /// Ratio del split interno de panes de editor.
    pub editor_pane_split_ratio: f32,
    /// Panel de editor actualmente enfocado.
    pub focused_editor_pane: EditorPane,
    /// Registro de lenguajes para detectar tipo de archivo
    pub language_registry: LanguageRegistry,
    /// Raíz del workspace visible en explorador
    pub workspace_root: PathBuf,
    /// Entradas renderizables del explorador
    pub explorer_entries: Vec<ExplorerEntry>,
    /// Ruta seleccionada actualmente en el explorador.
    pub explorer_selected_path: Option<PathBuf>,
    /// Mostrar artefactos generados (`target/`, `node_modules/`, binarios).
    ///
    /// Los secretos no se muestran nunca, sea cual sea este valor.
    pub explorer_show_noise: bool,
    /// Escala de la interfaz: preferencia del usuario por factor del monitor.
    ///
    /// Afecta a las letras y a los huecos por igual. Antes no existía: el ajuste
    /// de fuente solo alcanzaba al editor de código.
    pub ui_scale: Scale,
    /// Si está esperando respuesta de Quirón
    pub loading: bool,
    /// Si el panel de telemetría en chat está visible.
    pub telemetry_panel_enabled: bool,
    /// Runtime de tokio para async
    pub runtime: Runtime,
    /// Conexión con quiron-brain
    pub quiron: Arc<Mutex<Quiron>>,
    /// Modelo solicitado al gateway para el chat actual.
    selected_ai_model: String,
    /// URL base actual de quiron-brain usada por la UI.
    quiron_brain_url: String,
    /// Modo de conexión: true = seguro (token solo env/archivo).
    quiron_secure_mode: bool,
    /// Origen de credencial aplicada al cliente actual.
    quiron_auth_source: QuironAuthSource,
    /// Último resultado conocido de health del backend.
    quiron_last_health_ok: Option<bool>,
    /// Último estado completo del índice: unidades y nodos del grafo.
    pub quiron_index_health: Option<HealthResponse>,
    /// Identidad del proyecto abierto, leída de `.llore/project.id`.
    ///
    /// Sin proyecto abierto no hay identidad, y sin identidad el contexto
    /// recuperado no pertenece a ningún mundo.
    pub project_id: Option<String>,
    /// Proyectos abiertos recientemente, el más reciente primero.
    pub recent_projects: Vec<PathBuf>,
    /// Fichero donde se persisten los recientes.
    ///
    /// Se inyecta para que las pruebas no escriban en la lista real del usuario.
    recents_file: Option<PathBuf>,
    /// Timestamp del último polling de health.
    quiron_last_health_poll: Instant,
    /// Health check de quiron-brain en vuelo (si existe).
    quiron_health_task: Option<JoinHandle<Option<HealthResponse>>>,
    /// Tema visual activo de la UI.
    ui_theme: UiTheme,
    /// Densidad visual de espaciado de la UI.
    ui_density: UiDensity,
    /// Escala visual del texto y line-height del editor.
    editor_font_scale: f32,
    /// Padding horizontal entre gutter y bloque de código.
    editor_horizontal_padding: f32,
    /// Sangría horizontal por nivel en árbol del explorer.
    explorer_indent_step: f32,
    /// Último estado cognitivo visible en status bar
    pub status_text: String,
    /// Zonas clicables registradas durante el render actual
    pub click_targets: Vec<ClickTarget>,
    /// Última posición del cursor
    pub cursor_pos: Option<(f32, f32)>,
    /// Bounds del panel editor primario (actualizados en cada frame).
    pub editor_primary_bounds: Option<Bounds>,
    /// Bounds del panel editor secundario (actualizados en cada frame).
    pub editor_secondary_bounds: Option<Bounds>,
    /// Bounds del input de chat (actualizados en cada frame)
    pub input_bounds: Option<Bounds>,
    /// Bounds del panel de chat (actualizados en cada frame).
    pub chat_bounds: Option<Bounds>,
    /// Bounds de la columna sidebar (explorer)
    pub sidebar_bounds: Option<Bounds>,
    /// Panel activo del sidebar (explorer/search/git).
    pub sidebar_panel: SidebarPanel,
    /// Lado donde se acoplan activity bar y explorador.
    pub explorer_dock: PanelDock,
    /// Lado del editor donde se acopla el chat.
    pub chat_dock: PanelDock,
    /// Bounds del área principal donde vive el split editor/chat.
    pub main_content_bounds: Option<Bounds>,
    /// Gap horizontal entre panel editor y chat.
    pub main_split_gap: f32,
    /// Panel de segundo plano visible.
    ///
    /// La maqueta arranca con él abierto (`fondo: true`), pero aquí nace
    /// cerrado porque la columna todavía no se dibuja: si estuviera a `true`,
    /// `cabe_el_editor` reservaría sus 252 px más el hueco para algo que no
    /// ocupa nada, y el editor no llegaría a montarse en una ventana de 1280.
    /// Pasa a `true` cuando la columna exista.
    pub background_panel_visible: bool,
    /// Bloques de pensamiento desplegados, por índice del mensaje en `messages`.
    /// Nacen plegados, como el `pensado: false` de la maqueta: el encabezado
    /// ya dice cuánto tardó y qué tocó; el detalle se abre a demanda.
    pub expanded_thoughts: HashSet<usize>,
    /// Bounds del handle de resize de sidebar.
    pub sidebar_resizer_bounds: Option<Bounds>,
    /// Bounds del handle de resize editor/chat.
    pub editor_resizer_bounds: Option<Bounds>,
    /// Bounds del handle de resize entre panel primario/secundario.
    pub editor_pane_resizer_bounds: Option<Bounds>,
    /// Bounds del panel de resultados de búsqueda en editor.
    pub search_results_bounds: Option<Bounds>,
    /// Ancho actual de sidebar.
    pub sidebar_width: f32,
    /// Ratio de ancho editor dentro del split central (sin gap).
    pub editor_split_ratio: f32,
    /// Si se está arrastrando resize de sidebar.
    pub dragging_sidebar_resizer: bool,
    /// Si se está arrastrando resize de split editor/chat.
    pub dragging_editor_resizer: bool,
    /// Si se está arrastrando resize de split interno de editor.
    pub dragging_editor_pane_resizer: bool,
    /// Scroll del explorador (primera fila visible)
    pub explorer_scroll: usize,
    /// Scroll del panel Search del sidebar.
    pub sidebar_search_scroll: usize,
    /// Scroll del panel Problems del sidebar.
    pub sidebar_problems_scroll: usize,
    /// Scroll del panel Outline del sidebar.
    pub sidebar_outline_scroll: usize,
    /// Scroll del panel de resultados de búsqueda.
    pub search_results_scroll: usize,
    /// Conjunto de directorios expandidos en el explorador
    pub expanded_dirs: HashSet<PathBuf>,
    /// Confirmación pendiente para cierre forzado de tab con cambios sin guardar
    pub pending_close_tab_confirm: Option<usize>,
    /// Confirmación pendiente para cierre de ventana con cambios sin guardar
    pub pending_exit_confirm: bool,
    /// Estado del botón izquierdo del mouse.
    pub mouse_left_down: bool,
    /// Anchor de selección por drag (panel, linea, columna) en editor.
    pub mouse_selection_anchor: Option<(EditorPane, usize, usize)>,
    /// Último click en editor para multi-click (instante, tab, línea, columna, contador).
    pub last_editor_click: Option<(Instant, usize, usize, usize, u8)>,
    /// Hitboxes de tabs registradas en el frame actual.
    pub tab_hitboxes: Vec<TabHitbox>,
    /// Estado de drag de tab (si existe arrastre activo).
    pub tab_drag_state: Option<TabDragState>,
    /// Fallback local de clipboard cuando el sistema no responde.
    pub clipboard_fallback: String,
    /// Si el modo búsqueda incremental está activo.
    pub search_active: bool,
    /// Query de búsqueda incremental.
    pub search_query: String,
    /// Si está activo el modo replace.
    pub replace_active: bool,
    /// Query de reemplazo.
    pub replace_query: String,
    /// Campo activo en la barra find/replace.
    pub search_input_focus: SearchInputFocus,
    /// Opción de búsqueda: distinguir mayúsculas/minúsculas.
    pub search_match_case: bool,
    /// Opción de búsqueda: coincidencia de palabra completa.
    pub search_whole_word: bool,
    /// Opción de búsqueda: tratar query como regex real (sin escape).
    pub search_regex_mode: bool,
    /// Opción de replace: limitar a un scope de selección capturado.
    pub replace_in_selection_only: bool,
    /// Scope de selección capturado para replace in selection.
    pub replace_selection_scope: Option<SearchScope>,
    /// Confirmación pendiente para replace all (preview en dos pasos).
    pub replace_all_confirm_pending: bool,
    /// Matches sobre el buffer activo.
    pub search_matches: Vec<SearchMatch>,
    /// Índice de match activo en `search_matches`.
    pub active_search_match: Option<usize>,
    /// Índice activo del último problema navegado.
    active_problem_index: Option<usize>,
    /// Overlay activo (quick open / command palette).
    pub overlay_mode: Option<OverlayMode>,
    /// Menú superior desplegable activo.
    pub top_menu_open: Option<TopMenuKind>,
    /// Índice seleccionado dentro del menú superior activo.
    pub top_menu_selected_index: usize,
    /// Query actual del overlay activo.
    pub overlay_query: String,
    /// Items visibles en overlay.
    pub overlay_items: Vec<OverlayItem>,
    /// Índice activo dentro de `overlay_items`.
    pub overlay_selected: usize,
    /// Cache de archivos para quick open.
    pub quick_open_candidates: Vec<PathBuf>,
    /// Marca de invalidez de cache quick-open.
    pub quick_open_cache_stale: bool,
    /// Cache de símbolos de workspace para overlay global.
    workspace_symbol_candidates: Vec<WorkspaceSymbolCandidate>,
    /// Marca de invalidez de cache de símbolos de workspace.
    workspace_symbol_cache_stale: bool,
    /// Filtro activo del timeline de telemetría.
    pub telemetry_timeline_filter: TelemetryTimelineFilter,
    /// Scroll del timeline de telemetría (índice inicial visible).
    pub telemetry_timeline_scroll: usize,
    /// Último snapshot persistido de telemetría de sesión (si disponible).
    pub telemetry_persisted_cache: Option<SessionTelemetryResponse>,
    /// Último error de sync de telemetría persistida.
    pub telemetry_persisted_last_error: Option<String>,
    /// Resumen cacheado de git status para panel sidebar.
    pub git_sidebar_status: Option<GitSidebarStatus>,
    /// Último error de lectura git status del sidebar.
    pub git_sidebar_error: Option<String>,
    /// Timestamp del último polling de git status.
    pub last_git_status_poll: Instant,
    /// Prefetch persistido en ejecución (si existe).
    pub telemetry_persisted_prefetch_task:
        Option<JoinHandle<Result<SessionTelemetryResponse, String>>>,
    /// Session id asociado al prefetch en ejecución.
    pub telemetry_persisted_prefetch_session: Option<String>,
    /// Timestamp de último intento de prefetch automático.
    pub telemetry_persisted_prefetch_last_at: Instant,
    /// Firma del workspace para detectar cambios externos.
    pub workspace_signature: u64,
    /// Timestamp del último polling de workspace.
    pub last_workspace_poll: Instant,
}

impl AppState {
    /// Arranca sin ningún proyecto abierto.
    ///
    /// La raíz vacía es el estado «sin proyecto»: [`workspace_guard`] no puede
    /// canonicalizarla y niega toda lectura. El editor falla cerrado sin
    /// depender de que nadie recuerde comprobar una bandera.
    ///
    /// Antes se tomaba `current_dir()`, lo que abría la carpeta desde la que se
    /// hubiera lanzado el binario —el directorio personal completo, al invocarlo
    /// desde el menú de aplicaciones.
    pub fn new() -> Self {
        Self::new_with_workspace_root(PathBuf::new())
    }

    /// Pestaña inicial: el README del proyecto, si la guardia lo permite.
    fn initial_tab_for(workspace_root: &Path, language_registry: &LanguageRegistry) -> OpenTab {
        let vacia = || OpenTab {
            path: None,
            editor: Editor::with_text(""),
        };

        if workspace_root.as_os_str().is_empty() {
            return vacia();
        }

        let default_file = workspace_root.join("README.md");
        if !workspace_guard::classify(workspace_root, &default_file).is_allowed() {
            return vacia();
        }

        match fs::read_to_string(&default_file) {
            Ok(content) => {
                let mut editor = Editor::open_file(default_file.clone(), &content, language_registry);
                editor.mark_saved();
                OpenTab {
                    path: Some(default_file),
                    editor,
                }
            }
            Err(_) => vacia(),
        }
    }

    /// Cierto si hay un proyecto abierto.
    pub fn workspace_is_open(&self) -> bool {
        !self.workspace_root.as_os_str().is_empty()
    }

    /// Instrucciones que acompañan a cada pregunta del chat.
    ///
    /// Describen lo que el modelo tiene delante y, sobre todo, lo que **no**
    /// tiene: no puede abrir archivos por su cuenta. Prometerle capacidades que
    /// no existen produce respuestas inventadas.
    fn chat_system_prompt(&self) -> String {
        let proyecto = self
            .workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sin nombre");

        format!(
            "Eres el asistente de Llore, un editor de código. Trabajas sobre el \
             proyecto «{proyecto}».\n\n\
             Recibes como contexto el archivo que el usuario tiene abierto y su \
             selección si la hay. Además tienes manos: las herramientas \
             read_file (leer un archivo por su ruta relativa a la raíz), \
             list_files (enumerar rutas, con prefijo opcional) y search_text \
             (buscar texto en el proyecto). Cuando necesites ver algo que no \
             está en el contexto, úsalas: no le pidas al usuario que abra o \
             pegue archivos, y no supongas contenidos que puedes leer.\n\n\
             Tus manos pasan por la guardia del proyecto: los secretos (.env, \
             credenciales), lo que queda fuera de la carpeta y los artefactos \
             generados se deniegan. Si una herramienta te niega un acceso, \
             dilo tal cual y sigue sin ese contenido; no insistas ni especules \
             sobre lo denegado.\n\n\
             Responde en español, sin rodeos. Al citar código indica la ruta y \
             el número de línea. Si ni el contexto ni las herramientas bastan \
             para responder, dilo en lugar de suponer."
        )
    }

    /// Contexto que se adjunta a cada pregunta: proyecto, archivo activo y
    /// selección.
    ///
    /// El contenido se recorta al presupuesto para no agotar la ventana del
    /// modelo, y el recorte se declara: un archivo truncado en silencio induce a
    /// concluir que algo no existe.
    fn build_chat_context(&self) -> String {
        if !self.workspace_is_open() {
            return String::new();
        }

        let mut secciones = Vec::new();

        let raiz = self.workspace_root.display();
        let identidad = self.project_id.as_deref().unwrap_or("desconocida");
        secciones.push(format!("Raíz del proyecto: {raiz}\nIdentidad: {identidad}"));

        let ruta_relativa = |path: &Path| -> String {
            path.strip_prefix(&self.workspace_root)
                .unwrap_or(path)
                .display()
                .to_string()
        };

        let tab = &self.open_tabs[self.active_tab.min(self.open_tabs.len() - 1)];
        if let Some(path) = tab.path.as_ref() {
            let relativa = ruta_relativa(path);
            let contenido = tab.editor.text();
            let lineas = contenido.lines().count();

            let (cuerpo, recorte) = if contenido.len() > CHAT_CONTEXT_MAX_CHARS {
                let corte = contenido
                    .char_indices()
                    .take(CHAT_CONTEXT_MAX_CHARS)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                (&contenido[..corte], true)
            } else {
                (contenido.as_str(), false)
            };

            let aviso = if recorte {
                format!(
                    "\n\n[recortado: se muestran los primeros {} caracteres de {}]",
                    CHAT_CONTEXT_MAX_CHARS,
                    contenido.len()
                )
            } else {
                String::new()
            };

            secciones.push(format!(
                "Archivo abierto: {relativa} ({lineas} líneas)\n\
                 ```\n{cuerpo}\n```{aviso}"
            ));

            let seleccionado = tab.editor.selection().selected_text(&contenido);
            if !seleccionado.trim().is_empty() {
                secciones.push(format!("Selección actual:\n```\n{seleccionado}\n```"));
            }
        } else {
            secciones.push("No hay ningún archivo abierto en el editor.".to_string());
        }

        let otros: Vec<String> = self
            .open_tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.active_tab)
            .filter_map(|(_, t)| t.path.as_ref())
            .map(|p| ruta_relativa(p))
            .collect();
        if !otros.is_empty() {
            secciones.push(format!(
                "Otras pestañas abiertas (contenido no incluido): {}",
                otros.join(", ")
            ));
        }

        secciones.join("\n\n")
    }

    /// Instala la identidad del proyecto en el cliente de quiron-brain.
    fn set_quiron_project_id(&mut self, project_id: Option<String>) {
        let quiron = self.quiron.clone();
        self.runtime.block_on(async move {
            quiron.lock().await.set_project_id(project_id);
        });
    }

    /// Adopta `folder` como proyecto activo.
    ///
    /// La raíz se canonicaliza porque la guardia compara rutas canónicas: si
    /// llegara con enlaces sin resolver, su propio contenido parecería estar
    /// fuera. Después se restauran las pestañas de la sesión anterior que
    /// pertenezcan a este proyecto; el resto las descarta la guardia.
    pub fn open_workspace(&mut self, folder: PathBuf) {
        let folder = folder.canonicalize().unwrap_or(folder);

        // Conceder acceso a un directorio es lo que lo convierte en proyecto.
        // Sin identidad no hay proyecto: no se abre, y el estado no cambia.
        let project_id = match project_id::load_or_create(&folder) {
            Ok(id) => id,
            Err(error) => {
                self.status_text = format!("no se puede abrir el proyecto: {error}");
                self.needs_render = true;
                return;
            }
        };

        self.project_id = Some(project_id.clone());
        self.set_quiron_project_id(Some(project_id));

        if let Some(lista) = self.recents_file.clone() {
            recents::remember_in(&lista, &folder);
            self.recent_projects = recents::load_from(&lista);
        }
        self.workspace_root = folder.clone();
        self.expanded_dirs.clear();
        self.expanded_dirs.insert(folder);
        self.explorer_scroll = 0;
        self.refresh_explorer();
        // El estado se guarda dentro del proyecto: hasta ahora no había ninguno
        // que restaurar.
        self.restore_layout_snapshot();
        self.restore_session_snapshot();
        self.needs_render = true;
    }

    /// Veredicto de la guardia para una ruta. Sin proyecto no se lee nada.
    pub fn access_to(&self, path: &Path) -> Access {
        if !self.workspace_is_open() {
            return Access::Outside;
        }
        workspace_guard::classify(&self.workspace_root, path)
    }

    /// Lee un archivo del proyecto. Devuelve `None` si la guardia lo niega o si
    /// el archivo no puede leerse. Para los escaneos, que no informan al usuario.
    fn read_project_file(&self, path: &Path) -> Option<String> {
        if !self.access_to(path).is_allowed() {
            return None;
        }
        fs::read_to_string(path).ok()
    }

    fn new_with_workspace_root(workspace_root: PathBuf) -> Self {
        // Crear runtime de tokio
        let runtime = Runtime::new().expect("Failed to create tokio runtime");

        // Los almacenes viven con la aplicación: al abrir el editor se pide a
        // systemd que arranque el cerebro, cuyo ExecStartPre levanta Qdrant y
        // Neo4j. No se bloquea la interfaz: el health task reconecta cuando el
        // servicio termina de estar listo.
        Self::ensure_brain_service();

        // Crear Quiron (conexión a quiron-brain, configurable desde entorno en modo seguro)
        let (quiron, quiron_brain_url, quiron_secure_mode, quiron_auth_source, startup_status) =
            Self::build_quiron_from_env(&workspace_root);
        let language_registry = LanguageRegistry::with_builtin_languages();
        let initial_tab = Self::initial_tab_for(&workspace_root, &language_registry);

        let mut expanded_dirs = HashSet::new();
        if !workspace_root.as_os_str().is_empty() {
            expanded_dirs.insert(workspace_root.clone());
        }

        let mut state = Self {
            window_width: 1280,
            window_height: 720,
            needs_render: true,
            modifiers: ModifiersState::empty(),
            event_proxy: None,
            text_system: TextSystem::new(),
            layout_engine: LayoutEngine::new(),
            input_text: String::new(),
            input_focused: false,
            editor_focused: true,
            messages: vec![ChatMessage {
                is_user: false,
                content: "Listo. Abre un proyecto o pregunta sobre el código.".to_string(),
                meta: Some("system".to_string()),
                citations: vec![],
            }],
            open_tabs: vec![initial_tab],
            active_tab: 0,
            secondary_tab: None,
            editor_pane_split_enabled: false,
            editor_pane_split_ratio: DEFAULT_EDITOR_PANE_SPLIT_RATIO,
            focused_editor_pane: EditorPane::Primary,
            language_registry,
            workspace_root,
            explorer_entries: vec![],
            explorer_selected_path: None,
            explorer_show_noise: false,
            ui_scale: Scale::default(),
            loading: false,
            telemetry_panel_enabled: false,
            runtime,
            quiron: Arc::new(Mutex::new(quiron)),
            selected_ai_model: std::env::var("QUIRON_LLM_MODEL_PRIMARY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| AI_MODEL_OPTIONS[0].to_string()),
            quiron_brain_url,
            quiron_secure_mode,
            quiron_auth_source,
            quiron_last_health_ok: None,
            quiron_index_health: None,
            project_id: None,
            recent_projects: recents::load(),
            recents_file: recents::recents_path(),
            quiron_last_health_poll: Instant::now() - QUIRON_HEALTH_POLL_INTERVAL,
            quiron_health_task: None,
            // El rediseño «Modernist» es el aspecto de la aplicación, no un
            // tema alternativo: arranca puesto. Los tres anteriores siguen en la
            // paleta de comandos para quien los quiera.
            ui_theme: UiTheme::ModernistLight,
            ui_density: UiDensity::Normal,
            editor_font_scale: DEFAULT_EDITOR_FONT_SCALE,
            editor_horizontal_padding: DEFAULT_EDITOR_HORIZONTAL_PADDING,
            explorer_indent_step: DEFAULT_EXPLORER_INDENT_STEP,
            status_text: startup_status.unwrap_or_else(|| "ready".to_string()),
            click_targets: vec![],
            cursor_pos: None,
            editor_primary_bounds: None,
            editor_secondary_bounds: None,
            input_bounds: None,
            chat_bounds: None,
            sidebar_bounds: None,
            sidebar_panel: SidebarPanel::Explorer,
            explorer_dock: PanelDock::Left,
            // La maqueta pone el chat en el centro, justo tras la lateral, y el
            // editor a su derecha. Acoplado a la derecha quedaba en el borde y el
            // centro se lo llevaba el editor: al revés de lo que pide el diseño.
            chat_dock: PanelDock::Left,
            main_content_bounds: None,
            main_split_gap: COLUMN_GAP,
            background_panel_visible: false,
            expanded_thoughts: HashSet::new(),
            sidebar_resizer_bounds: None,
            editor_resizer_bounds: None,
            editor_pane_resizer_bounds: None,
            search_results_bounds: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            editor_split_ratio: DEFAULT_EDITOR_SPLIT_RATIO,
            dragging_sidebar_resizer: false,
            dragging_editor_resizer: false,
            dragging_editor_pane_resizer: false,
            explorer_scroll: 0,
            sidebar_search_scroll: 0,
            sidebar_problems_scroll: 0,
            sidebar_outline_scroll: 0,
            search_results_scroll: 0,
            expanded_dirs,
            pending_close_tab_confirm: None,
            pending_exit_confirm: false,
            mouse_left_down: false,
            mouse_selection_anchor: None,
            last_editor_click: None,
            tab_hitboxes: vec![],
            tab_drag_state: None,
            clipboard_fallback: String::new(),
            search_active: false,
            search_query: String::new(),
            replace_active: false,
            replace_query: String::new(),
            search_input_focus: SearchInputFocus::Find,
            search_match_case: true,
            search_whole_word: false,
            search_regex_mode: false,
            replace_in_selection_only: false,
            replace_selection_scope: None,
            replace_all_confirm_pending: false,
            search_matches: vec![],
            active_search_match: None,
            active_problem_index: None,
            overlay_mode: None,
            top_menu_open: None,
            top_menu_selected_index: 0,
            overlay_query: String::new(),
            overlay_items: vec![],
            overlay_selected: 0,
            quick_open_candidates: vec![],
            quick_open_cache_stale: true,
            workspace_symbol_candidates: vec![],
            workspace_symbol_cache_stale: true,
            telemetry_timeline_filter: TelemetryTimelineFilter::All,
            telemetry_timeline_scroll: 0,
            telemetry_persisted_cache: None,
            telemetry_persisted_last_error: None,
            git_sidebar_status: None,
            git_sidebar_error: None,
            last_git_status_poll: Instant::now() - GIT_STATUS_POLL_INTERVAL,
            telemetry_persisted_prefetch_task: None,
            telemetry_persisted_prefetch_session: None,
            telemetry_persisted_prefetch_last_at: Instant::now()
                - TELEMETRY_PERSISTED_PREFETCH_INTERVAL,
            workspace_signature: 0,
            last_workspace_poll: Instant::now(),
        };
        state.refresh_explorer();
        state.workspace_signature = state.compute_workspace_signature();
        state.restore_session_snapshot();
        state.restore_layout_snapshot();
        let _ = state.poll_quiron_health();
        state
    }

    #[cfg(test)]
    fn new_for_tests(workspace_root: PathBuf) -> Self {
        let mut state = Self::new_with_workspace_root(workspace_root);
        // Las pruebas no deben tocar la lista de recientes del usuario.
        state.recents_file = Some(
            std::env::temp_dir()
                .join(format!("llore_test_recents_{}", std::process::id()))
                .join("recents.txt"),
        );
        state.recent_projects.clear();
        state
    }

    /// Pide a systemd (--user) que arranque el servicio del cerebro, que a su
    /// vez levanta los almacenes. Best-effort e idempotente: si systemd no está,
    /// el servicio no existe o ya corre, no pasa nada y el editor conecta igual.
    /// No espera: `systemctl start` bloquearía mientras Neo4j se inicializa, así
    /// que se lanza y se deja al health task reintentar la conexión.
    fn ensure_brain_service() {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "start", "quiron-brain.service"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    fn build_quiron_from_env(
        workspace_root: &Path,
    ) -> (Quiron, String, bool, QuironAuthSource, Option<String>) {
        let defaults = QuironConfig::default();
        let secure_mode = Self::read_secure_mode_flag_from_env();

        let requested_url =
            Self::env_non_empty(QUIRON_BRAIN_URL_ENV).unwrap_or_else(|| defaults.brain_url.clone());
        let (brain_url, mut startup_warning) = match Self::normalize_brain_url(&requested_url) {
            Ok(url) => (url, None),
            Err(err) => (
                defaults.brain_url.clone(),
                Some(format!(
                    "invalid {} '{}': {}",
                    QUIRON_BRAIN_URL_ENV, requested_url, err
                )),
            ),
        };

        let (api_token, auth_source) = if secure_mode {
            match Self::resolve_secure_token() {
                Ok(values) => values,
                Err(err) => {
                    startup_warning = Some(err);
                    (None, QuironAuthSource::None)
                }
            }
        } else {
            let token = Self::env_non_empty(QUIRON_API_TOKEN_ENV);
            let source = if token.is_some() {
                QuironAuthSource::EnvToken
            } else {
                QuironAuthSource::None
            };
            (token, source)
        };

        let config = QuironConfig {
            brain_url: brain_url.clone(),
            api_token,
            agent_path: workspace_root.join(".llore"),
            gates_enabled: defaults.gates_enabled,
            max_iterations: defaults.max_iterations,
        };

        (
            Quiron::new(config),
            brain_url,
            secure_mode,
            auth_source,
            startup_warning.map(|msg| format!("connection warning: {}", msg)),
        )
    }

    fn env_non_empty(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn read_secure_mode_flag_from_env() -> bool {
        let Some(raw) = Self::env_non_empty(QUIRON_SECURE_MODE_ENV) else {
            return true;
        };
        match raw.to_ascii_lowercase().as_str() {
            "0" | "false" | "no" | "off" => false,
            _ => true,
        }
    }

    fn normalize_brain_url(raw: &str) -> Result<String, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("url is empty".to_string());
        }
        if trimmed.chars().any(|ch| ch.is_whitespace()) {
            return Err("url contains whitespace".to_string());
        }

        let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("http://{}", trimmed)
        };

        let normalized = with_scheme.trim_end_matches('/').to_string();
        if normalized == "http://" || normalized == "https://" {
            return Err("url host is missing".to_string());
        }

        Ok(normalized)
    }

    fn resolve_secure_token() -> Result<(Option<String>, QuironAuthSource), String> {
        if let Some(token_file) = Self::env_non_empty(QUIRON_API_TOKEN_FILE_ENV) {
            let token_path = PathBuf::from(token_file.clone());
            let token = Self::read_token_from_file_secure(&token_path)?;
            return Ok((Some(token), QuironAuthSource::FileToken));
        }

        if let Some(token) = Self::env_non_empty(QUIRON_API_TOKEN_ENV) {
            return Ok((Some(token), QuironAuthSource::EnvToken));
        }

        if let Some((token, source)) = Self::resolve_token_from_brain_env_file()? {
            return Ok((Some(token), source));
        }

        Ok((None, QuironAuthSource::None))
    }

    fn resolve_token_from_brain_env_file() -> Result<Option<(String, QuironAuthSource)>, String> {
        let env_path = Self::env_non_empty(QUIRON_BRAIN_ENV_FILE_ENV)
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(QUIRON_BRAIN_ENV_FILE_DEFAULT_REL))
            });

        let Some(env_path) = env_path else {
            return Ok(None);
        };
        if !env_path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&env_path).map_err(|err| {
            format!(
                "cannot read quiron env file '{}': {}",
                env_path.display(),
                err
            )
        })?;

        if let Some(token_file_raw) = Self::parse_env_assignment(&raw, QUIRON_API_TOKEN_FILE_ENV) {
            let file_path = PathBuf::from(&token_file_raw);
            let token_path = if file_path.is_absolute() {
                file_path
            } else {
                env_path.parent().unwrap_or(Path::new(".")).join(file_path)
            };
            let token = Self::read_token_from_file_secure(&token_path)?;
            return Ok(Some((token, QuironAuthSource::FileToken)));
        }

        if let Some(token) = Self::parse_env_assignment(&raw, QUIRON_API_TOKEN_ENV) {
            return Ok(Some((token, QuironAuthSource::EnvToken)));
        }

        Ok(None)
    }

    fn parse_env_assignment(raw: &str, key: &str) -> Option<String> {
        for line in raw.lines() {
            let mut line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(stripped) = line.strip_prefix("export ") {
                line = stripped.trim();
            }

            let Some((left, right)) = line.split_once('=') else {
                continue;
            };
            if left.trim() != key {
                continue;
            }

            let value = right
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim()
                .to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
        None
    }

    fn read_token_from_file_secure(path: &Path) -> Result<String, String> {
        let metadata = fs::metadata(path)
            .map_err(|err| format!("cannot read token file '{}': {}", path.display(), err))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(format!(
                    "token file '{}' has insecure mode {:o}; use 600/400",
                    path.display(),
                    mode
                ));
            }
        }

        let token = fs::read_to_string(path)
            .map_err(|err| format!("cannot read token file '{}': {}", path.display(), err))?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(format!("token file '{}' is empty", path.display()));
        }
        Ok(token)
    }

    fn connection_endpoint_label(url: &str) -> String {
        let without_scheme = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .unwrap_or(url);
        let endpoint = without_scheme
            .split('/')
            .next()
            .unwrap_or(without_scheme)
            .trim();
        if endpoint.is_empty() {
            "gateway:?".to_string()
        } else {
            format!("gateway:{}", endpoint)
        }
    }

    pub fn quiron_connection_endpoint_label(&self) -> String {
        Self::connection_endpoint_label(&self.quiron_brain_url)
    }

    pub fn quiron_connection_mode_label(&self) -> String {
        format!(
            "secure={} auth={}",
            if self.quiron_secure_mode { "on" } else { "off" },
            self.quiron_auth_source.short_label()
        )
    }

    pub fn quiron_connection_health_label(&self) -> String {
        let value = if self.quiron_health_task.is_some() {
            "checking"
        } else {
            match self.quiron_last_health_ok {
                Some(true) => "ok",
                Some(false) => "down",
                None => "?",
            }
        };
        format!("health={}", value)
    }

    fn start_quiron_health_check(&mut self) {
        if self.quiron_health_task.is_some() {
            return;
        }
        self.quiron_last_health_poll = Instant::now();
        let quiron = self.quiron.clone();
        self.quiron_health_task = Some(self.runtime.spawn(async move {
            let q = quiron.lock().await;
            q.health_snapshot().await
        }));
    }

    fn reconnect_quiron_with_mode(
        &mut self,
        target_url: &str,
        secure_mode: bool,
    ) -> Result<(), String> {
        let normalized = Self::normalize_brain_url(target_url)?;
        let defaults = QuironConfig::default();

        let (api_token, auth_source) = if secure_mode {
            Self::resolve_secure_token()?
        } else {
            let token = Self::env_non_empty(QUIRON_API_TOKEN_ENV);
            let source = if token.is_some() {
                QuironAuthSource::EnvToken
            } else {
                QuironAuthSource::None
            };
            (token, source)
        };

        let config = QuironConfig {
            brain_url: normalized.clone(),
            api_token,
            agent_path: self.workspace_root.join(".llore"),
            gates_enabled: defaults.gates_enabled,
            max_iterations: defaults.max_iterations,
        };
        let new_quiron = Arc::new(Mutex::new(Quiron::new(config)));

        self.quiron = new_quiron;
        self.quiron_brain_url = normalized;
        self.quiron_secure_mode = secure_mode;
        self.quiron_auth_source = auth_source;
        self.quiron_last_health_ok = None;
        self.quiron_last_health_poll = Instant::now() - QUIRON_HEALTH_POLL_INTERVAL;
        self.quiron_health_task = None;
        self.telemetry_persisted_cache = None;
        self.telemetry_persisted_last_error = None;
        self.reset_telemetry_prefetch_state();
        self.start_quiron_health_check();
        self.needs_render = true;
        Ok(())
    }

    fn post_connection_status_message(&mut self, source: &str) {
        let content = format!(
            "Connection status\nendpoint: {}\nurl: {}\nsecure_mode: {}\nauth_source: {}\nhealth: {}\nsource: {}\n\nAll model traffic goes through the configured local gateway.\nSecure mode reads credentials from '{}', '{}' or fallback env file '{}'.",
            self.quiron_connection_endpoint_label(),
            self.quiron_brain_url,
            if self.quiron_secure_mode { "on" } else { "off" },
            self.quiron_auth_source.short_label(),
            self.quiron_connection_health_label(),
            source,
            QUIRON_API_TOKEN_FILE_ENV,
            QUIRON_API_TOKEN_ENV,
            QUIRON_BRAIN_ENV_FILE_DEFAULT_REL,
        );
        self.messages.push(ChatMessage {
            is_user: false,
            content,
            meta: Some("connection_status".to_string()),
            citations: vec![],
        });
        self.status_text = format!(
            "{} | {} | {}",
            self.quiron_connection_endpoint_label(),
            self.quiron_connection_mode_label(),
            self.quiron_connection_health_label()
        );
        self.needs_render = true;
    }

    fn try_handle_chat_control_command(&mut self, content: &str) -> bool {
        let trimmed = content.trim();
        if !trimmed.starts_with('/') {
            return false;
        }

        let mut parts = trimmed.split_whitespace();
        let Some(command) = parts.next() else {
            return false;
        };
        let command = command.to_ascii_lowercase();
        let recognized = matches!(
            command.as_str(),
            "/connection"
                | "/conn"
                | "/connect"
                | "/connect-local"
                | "/connect-env"
                | "/connect-help"
                | "/help-connect"
        );
        if !recognized {
            return false;
        }

        self.messages.push(ChatMessage {
            is_user: true,
            content: trimmed.to_string(),
            meta: Some("control".to_string()),
            citations: vec![],
        });

        match command.as_str() {
            "/connection" | "/conn" => {
                self.post_connection_status_message("slash-command");
            }
            "/connect" => {
                let Some(target_url) = parts.next() else {
                    self.messages.push(ChatMessage {
                        is_user: false,
                        content: "Usage: /connect <url>\nExample: /connect http://localhost:8766"
                            .to_string(),
                        meta: Some("connection_usage".to_string()),
                        citations: vec![],
                    });
                    self.status_text = "connection usage: /connect <url>".to_string();
                    self.needs_render = true;
                    return true;
                };
                match self.reconnect_quiron_with_mode(target_url, true) {
                    Ok(()) => {
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!(
                                "Secure reconnect applied\n{}\n{}\nhealth check running...",
                                self.quiron_connection_endpoint_label(),
                                self.quiron_connection_health_label()
                            ),
                            meta: Some("connection_reconnect".to_string()),
                            citations: vec![],
                        });
                        self.status_text = format!(
                            "secure reconnect applied | {} | {} | health=checking",
                            self.quiron_connection_endpoint_label(),
                            self.quiron_connection_mode_label(),
                        );
                        self.needs_render = true;
                    }
                    Err(err) => {
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!("Secure reconnect failed\n{}", err),
                            meta: Some("connection_error".to_string()),
                            citations: vec![],
                        });
                        self.status_text = format!("secure reconnect failed: {}", err);
                        self.needs_render = true;
                    }
                }
            }
            "/connect-local" => {
                match self.reconnect_quiron_with_mode("http://localhost:8766", true) {
                    Ok(()) => {
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!(
                                "Secure reconnect local applied\n{}\n{}\nhealth check running...",
                                self.quiron_connection_endpoint_label(),
                                self.quiron_connection_health_label()
                            ),
                            meta: Some("connection_reconnect".to_string()),
                            citations: vec![],
                        });
                        self.status_text = format!(
                            "secure reconnect local applied | {} | {} | health=checking",
                            self.quiron_connection_endpoint_label(),
                            self.quiron_connection_mode_label(),
                        );
                        self.needs_render = true;
                    }
                    Err(err) => {
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!("Secure reconnect local failed\n{}", err),
                            meta: Some("connection_error".to_string()),
                            citations: vec![],
                        });
                        self.status_text = format!("secure reconnect local failed: {}", err);
                        self.needs_render = true;
                    }
                }
            }
            "/connect-env" => {
                let target = Self::env_non_empty(QUIRON_BRAIN_URL_ENV)
                    .unwrap_or_else(|| "http://localhost:8766".to_string());
                match self.reconnect_quiron_with_mode(&target, true) {
                    Ok(()) => {
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!(
                                "Secure reconnect env applied\n{}\n{}\nhealth check running...",
                                self.quiron_connection_endpoint_label(),
                                self.quiron_connection_health_label()
                            ),
                            meta: Some("connection_reconnect".to_string()),
                            citations: vec![],
                        });
                        self.status_text = format!(
                            "secure reconnect env applied | {} | {} | health=checking",
                            self.quiron_connection_endpoint_label(),
                            self.quiron_connection_mode_label(),
                        );
                        self.needs_render = true;
                    }
                    Err(err) => {
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!("Secure reconnect env failed\n{}", err),
                            meta: Some("connection_error".to_string()),
                            citations: vec![],
                        });
                        self.status_text = format!("secure reconnect env failed: {}", err);
                        self.needs_render = true;
                    }
                }
            }
            "/connect-help" | "/help-connect" => {
                self.messages.push(ChatMessage {
                    is_user: false,
                    content: format!(
                        "Connection commands\n- /connection\n- /connect <url>\n- /connect-local\n- /connect-env\n\nModel traffic always uses the configured local gateway.\nSecure mode only reads token from '{}' (preferred) or '{}'.",
                        QUIRON_API_TOKEN_FILE_ENV,
                        QUIRON_API_TOKEN_ENV
                    ),
                    meta: Some("connection_help".to_string()),
                    citations: vec![],
                });
                self.status_text = "connection help posted".to_string();
                self.needs_render = true;
            }
            _ => {}
        }

        true
    }

    /// Ruta del snapshot de sesión, si hay proyecto abierto.
    ///
    /// El estado se guarda dentro del proyecto. Sin raíz, `join` devolvería una
    /// ruta relativa y el editor escribiría en el directorio de trabajo, sea
    /// cual sea.
    fn session_snapshot_path(&self) -> Option<PathBuf> {
        self.workspace_is_open()
            .then(|| self.workspace_root.join(SESSION_SNAPSHOT_RELATIVE_PATH))
    }

    /// Ruta del snapshot de layout, si hay proyecto abierto.
    fn layout_snapshot_path(&self) -> Option<PathBuf> {
        self.workspace_is_open()
            .then(|| self.workspace_root.join(LAYOUT_SNAPSHOT_RELATIVE_PATH))
    }

    /// Persiste sesión actual (tabs con path real + pestaña activa).
    pub fn persist_session_snapshot(&self) {
        let mut saved_paths: Vec<PathBuf> = Vec::new();
        for tab in &self.open_tabs {
            if let Some(path) = tab.path.as_ref() {
                if !saved_paths.iter().any(|p| p == path) {
                    saved_paths.push(path.clone());
                }
            }
        }

        let Some(snapshot_path) = self.session_snapshot_path() else {
            return;
        };
        if saved_paths.is_empty() {
            if snapshot_path.exists() {
                if let Err(err) = fs::remove_file(&snapshot_path) {
                    tracing::warn!("Failed to remove empty session snapshot: {}", err);
                }
            }
            return;
        }

        let active_saved_index = self
            .active_file_path()
            .and_then(|active| saved_paths.iter().position(|p| p == active))
            .unwrap_or(0);

        let mut lines = Vec::with_capacity(saved_paths.len() + 1);
        lines.push(format!("active={}", active_saved_index));
        lines.extend(saved_paths.iter().map(|p| p.display().to_string()));
        let payload = format!("{}\n", lines.join("\n"));

        if let Some(parent) = snapshot_path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                tracing::warn!(
                    "Failed to create session directory '{}': {}",
                    parent.display(),
                    err
                );
                return;
            }
        }

        if let Err(err) = fs::write(&snapshot_path, payload) {
            tracing::warn!(
                "Failed to write session snapshot '{}': {}",
                snapshot_path.display(),
                err
            );
        }
    }

    /// Restaura sesión previa de tabs si existe snapshot válido.
    fn restore_session_snapshot(&mut self) {
        let Some(snapshot_path) = self.session_snapshot_path() else {
            return;
        };
        let Ok(content) = fs::read_to_string(&snapshot_path) else {
            return;
        };

        let mut lines = content.lines();
        let active_line = lines.next().unwrap_or_default().trim();
        let active_saved_index = active_line
            .strip_prefix("active=")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        let mut restored_tabs: Vec<OpenTab> = Vec::new();
        for raw in lines {
            let candidate = raw.trim();
            if candidate.is_empty() {
                continue;
            }
            let path = PathBuf::from(candidate);
            if !path.is_file() {
                continue;
            }
            // Una sesión anterior pudo dejar abiertas rutas ajenas al proyecto
            // actual. La guardia decide de nuevo en cada restauración.
            let Some(content) = self.read_project_file(&path) else {
                continue;
            };
            let mut editor = Editor::open_file(path.clone(), &content, &self.language_registry);
            editor.mark_saved();
            restored_tabs.push(OpenTab {
                path: Some(path),
                editor,
            });
        }

        if restored_tabs.is_empty() {
            return;
        }

        self.open_tabs = restored_tabs;
        self.active_tab = active_saved_index.min(self.open_tabs.len().saturating_sub(1));
        self.secondary_tab = None;
        self.focused_editor_pane = EditorPane::Primary;
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        self.set_focus(FocusTarget::EditorPrimary);
        self.status_text = format!("session restored: {} tabs", self.open_tabs.len());
        let _ = self.reveal_active_file_in_explorer();
        self.refresh_search_matches_if_active();
        self.needs_render = true;
    }

    /// Persiste distribución visual actual (sidebar/split).
    pub fn persist_layout_snapshot(&self) {
        let Some(snapshot_path) = self.layout_snapshot_path() else {
            return;
        };
        let payload = format!(
            "sidebar_width={:.2}\neditor_split_ratio={:.4}\neditor_pane_split_enabled={}\neditor_pane_split_ratio={:.4}\nsidebar_panel={}\nexplorer_dock={}\nchat_dock={}\nai_model={}\nui_theme={}\nui_density={}\neditor_font_scale={:.3}\neditor_horizontal_padding={:.2}\nexplorer_indent_step={:.2}\n",
            self.sidebar_width,
            self.editor_split_ratio,
            if self.editor_pane_split_enabled { 1 } else { 0 },
            self.editor_pane_split_ratio,
            self.sidebar_panel.config_value(),
            self.explorer_dock.config_value(),
            self.chat_dock.config_value(),
            self.selected_ai_model,
            self.ui_theme.config_value(),
            self.ui_density.config_value(),
            self.editor_font_scale,
            self.editor_horizontal_padding,
            self.explorer_indent_step
        );

        if let Some(parent) = snapshot_path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                tracing::warn!(
                    "Failed to create layout directory '{}': {}",
                    parent.display(),
                    err
                );
                return;
            }
        }

        if let Err(err) = fs::write(&snapshot_path, payload) {
            tracing::warn!(
                "Failed to write layout snapshot '{}': {}",
                snapshot_path.display(),
                err
            );
        }
    }

    /// Restaura layout de paneles si existe snapshot válido.
    fn restore_layout_snapshot(&mut self) {
        let Some(snapshot_path) = self.layout_snapshot_path() else {
            return;
        };
        let Ok(content) = fs::read_to_string(&snapshot_path) else {
            return;
        };

        let mut sidebar = None;
        let mut ratio = None;
        let mut pane_split_enabled = None;
        let mut pane_split_ratio = None;
        let mut sidebar_panel = None;
        let mut explorer_dock = None;
        let mut chat_dock = None;
        let mut ai_model = None;
        let mut ui_theme = None;
        let mut ui_density = None;
        let mut editor_font_scale = None;
        let mut editor_horizontal_padding = None;
        let mut explorer_indent_step = None;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(value) = line.strip_prefix("sidebar_width=") {
                sidebar = value.parse::<f32>().ok();
            } else if let Some(value) = line.strip_prefix("editor_split_ratio=") {
                ratio = value.parse::<f32>().ok();
            } else if let Some(value) = line.strip_prefix("editor_pane_split_enabled=") {
                pane_split_enabled = match value.trim() {
                    "1" | "true" | "on" => Some(true),
                    "0" | "false" | "off" => Some(false),
                    _ => None,
                };
            } else if let Some(value) = line.strip_prefix("editor_pane_split_ratio=") {
                pane_split_ratio = value.parse::<f32>().ok();
            } else if let Some(value) = line.strip_prefix("sidebar_panel=") {
                sidebar_panel = SidebarPanel::from_config_value(value.trim());
            } else if let Some(value) = line.strip_prefix("explorer_dock=") {
                explorer_dock = PanelDock::from_config_value(value.trim());
            } else if let Some(value) = line.strip_prefix("chat_dock=") {
                chat_dock = PanelDock::from_config_value(value.trim());
            } else if let Some(value) = line.strip_prefix("ai_model=") {
                ai_model = AI_MODEL_OPTIONS
                    .contains(&value.trim())
                    .then(|| value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("ui_theme=") {
                ui_theme = UiTheme::from_config_value(value.trim());
            } else if let Some(value) = line.strip_prefix("ui_density=") {
                ui_density = UiDensity::from_config_value(value.trim());
            } else if let Some(value) = line.strip_prefix("editor_font_scale=") {
                editor_font_scale = value.parse::<f32>().ok();
            } else if let Some(value) = line.strip_prefix("editor_horizontal_padding=") {
                editor_horizontal_padding = value.parse::<f32>().ok();
            } else if let Some(value) = line.strip_prefix("explorer_indent_step=") {
                explorer_indent_step = value.parse::<f32>().ok();
            }
        }

        if let Some(value) = sidebar {
            self.sidebar_width = value.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        }
        if let Some(value) = ratio {
            self.editor_split_ratio = value.clamp(0.1, 0.9);
        }
        if let Some(value) = pane_split_enabled {
            self.editor_pane_split_enabled = value;
        }
        if let Some(value) = pane_split_ratio {
            self.editor_pane_split_ratio = value.clamp(0.2, 0.8);
        }
        if let Some(value) = sidebar_panel {
            self.sidebar_panel = value;
        }
        if let Some(value) = explorer_dock {
            self.explorer_dock = value;
        }
        if let Some(value) = chat_dock {
            self.chat_dock = value;
        }
        if let Some(value) = ai_model {
            self.selected_ai_model = value;
        }
        if let Some(value) = ui_theme {
            self.ui_theme = value;
        }
        if let Some(value) = ui_density {
            self.ui_density = value;
        }
        if let Some(value) = editor_font_scale {
            self.editor_font_scale = value.clamp(MIN_EDITOR_FONT_SCALE, MAX_EDITOR_FONT_SCALE);
        }
        if let Some(value) = editor_horizontal_padding {
            self.editor_horizontal_padding =
                value.clamp(MIN_EDITOR_HORIZONTAL_PADDING, MAX_EDITOR_HORIZONTAL_PADDING);
        }
        if let Some(value) = explorer_indent_step {
            self.explorer_indent_step =
                value.clamp(MIN_EXPLORER_INDENT_STEP, MAX_EXPLORER_INDENT_STEP);
        }
        self.normalize_editor_panes();
    }

    fn default_secondary_tab_index(&self) -> Option<usize> {
        if self.open_tabs.len() <= 1 {
            None
        } else if self.active_tab == 0 {
            Some(1)
        } else {
            Some(0)
        }
    }

    fn normalize_editor_panes(&mut self) {
        if self.open_tabs.is_empty() {
            self.open_tabs.push(OpenTab {
                path: None,
                editor: Editor::new(),
            });
        }

        self.active_tab = self.active_tab.min(self.open_tabs.len().saturating_sub(1));

        if self.open_tabs.len() <= 1 {
            self.secondary_tab = None;
            self.editor_pane_split_enabled = false;
            self.focused_editor_pane = EditorPane::Primary;
            return;
        }

        if self.editor_pane_split_enabled && self.secondary_tab.is_none() {
            self.secondary_tab = self.default_secondary_tab_index();
        }

        if let Some(mut secondary_idx) = self.secondary_tab {
            secondary_idx = secondary_idx.min(self.open_tabs.len().saturating_sub(1));
            if secondary_idx == self.active_tab {
                secondary_idx = if secondary_idx + 1 < self.open_tabs.len() {
                    secondary_idx + 1
                } else {
                    secondary_idx.saturating_sub(1)
                };
            }
            self.secondary_tab = Some(secondary_idx);
        }

        if !self.editor_pane_split_enabled {
            self.secondary_tab = None;
            if self.focused_editor_pane == EditorPane::Secondary {
                self.focused_editor_pane = EditorPane::Primary;
            }
        }
    }

    pub fn is_editor_split_active(&self) -> bool {
        self.editor_pane_split_enabled && self.secondary_tab.is_some()
    }

    pub fn pane_tab_index(&self, pane: EditorPane) -> usize {
        match pane {
            EditorPane::Primary => self.active_tab,
            EditorPane::Secondary => {
                if self.is_editor_split_active() {
                    self.secondary_tab.unwrap_or(self.active_tab)
                } else {
                    self.active_tab
                }
            }
        }
    }

    fn focused_tab_index(&self) -> usize {
        self.pane_tab_index(self.focused_editor_pane)
    }

    pub fn is_editor_pane_focused(&self, pane: EditorPane) -> bool {
        self.editor_focused
            && matches!(
                (pane, self.focused_editor_pane),
                (EditorPane::Primary, EditorPane::Primary)
                    | (EditorPane::Secondary, EditorPane::Secondary)
            )
    }

    pub fn set_tab_for_pane(&mut self, pane: EditorPane, index: usize) {
        if index >= self.open_tabs.len() {
            return;
        }
        match pane {
            EditorPane::Primary => self.active_tab = index,
            EditorPane::Secondary => self.secondary_tab = Some(index),
        }
        self.normalize_editor_panes();
    }

    pub fn toggle_editor_pane_split(&mut self) {
        if self.open_tabs.len() <= 1 {
            self.status_text = "cannot close last tab".to_string();
            self.needs_render = true;
            return;
        }

        self.editor_pane_split_enabled = !self.editor_pane_split_enabled;
        if self.editor_pane_split_enabled {
            if self.secondary_tab.is_none() {
                self.secondary_tab = self.default_secondary_tab_index();
            }
            self.status_text = format!(
                "split on ({:.0}%/{:.0}%)",
                self.editor_pane_split_ratio * 100.0,
                (1.0 - self.editor_pane_split_ratio) * 100.0
            );
        } else {
            self.focused_editor_pane = EditorPane::Primary;
            self.secondary_tab = None;
            self.status_text = "split off".to_string();
        }
        self.normalize_editor_panes();
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    /// Limpia las zonas clicables para el próximo frame.
    pub fn clear_click_targets(&mut self) {
        self.click_targets.clear();
        self.tab_hitboxes.clear();
    }

    /// Registra una zona clicable.
    pub fn add_click_target(&mut self, bounds: Bounds, action: ClickTargetAction) {
        self.click_targets.push(ClickTarget { bounds, action });
    }

    /// Registra hitbox de tab para hit-test y drag/reorder.
    pub fn add_tab_hitbox(&mut self, pane: EditorPane, index: usize, bounds: Bounds) {
        self.tab_hitboxes.push(TabHitbox {
            pane,
            index,
            bounds,
        });
    }

    /// Actualiza bounds del panel editor primario.
    pub fn set_editor_primary_bounds(&mut self, bounds: Bounds) {
        self.editor_primary_bounds = Some(bounds);
    }

    /// Actualiza bounds del panel editor secundario.
    pub fn set_editor_secondary_bounds(&mut self, bounds: Option<Bounds>) {
        self.editor_secondary_bounds = bounds;
    }

    /// Actualiza bounds del handle de resize entre paneles de editor.
    pub fn set_editor_pane_resizer_bounds(&mut self, bounds: Option<Bounds>) {
        self.editor_pane_resizer_bounds = bounds;
    }

    /// Bounds de un panel editor específico.
    pub fn editor_pane_bounds(&self, pane: EditorPane) -> Option<&Bounds> {
        match pane {
            EditorPane::Primary => self.editor_primary_bounds.as_ref(),
            EditorPane::Secondary => self.editor_secondary_bounds.as_ref(),
        }
    }

    /// Devuelve el panel editor que contiene un punto de pantalla.
    pub fn editor_pane_at_point(&self, x: f32, y: f32) -> Option<EditorPane> {
        if self
            .editor_secondary_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            return Some(EditorPane::Secondary);
        }
        if self
            .editor_primary_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            return Some(EditorPane::Primary);
        }
        None
    }

    /// Actualiza bounds del input de chat.
    pub fn set_input_bounds(&mut self, bounds: Bounds) {
        self.input_bounds = Some(bounds);
    }

    /// Actualiza bounds del panel de chat.
    pub fn set_chat_bounds(&mut self, bounds: Bounds) {
        self.chat_bounds = Some(bounds);
    }

    /// Actualiza bounds del sidebar/explorer.
    pub fn set_sidebar_bounds(&mut self, bounds: Bounds) {
        self.sidebar_bounds = Some(bounds);
    }

    /// Actualiza bounds del área principal (split editor/chat) y su gap.
    /// ¿Cabe el editor en la fila?
    ///
    /// Regla de la maqueta, literal: «el editor solo cabe si queda sitio para
    /// el chat y para el lateral derecho; si no, no se monta». Es decir, el
    /// editor no se cuela a costa de dejar el chat inservible — más vale un
    /// chat legible ocupando el centro que dos columnas estranguladas.
    ///
    /// De aquí sale el comportamiento que se ve al estrechar la ventana: el
    /// editor desaparece y el chat vuelve al centro, en vez de repartirse un
    /// espacio en el que ninguno de los dos sirve.
    /// ¿Hay algún archivo abierto?
    ///
    /// Es la otra mitad de la regla de la maqueta, su `!!s.editor`: la columna
    /// del editor existe cuando hay un archivo que enseñar, no siempre. Llore
    /// arranca con una pestaña sin título, que no cuenta: una pestaña vacía no
    /// es un archivo abierto.
    pub fn hay_archivo_abierto(&self) -> bool {
        self.open_tabs.iter().any(|tab| tab.path.is_some())
    }

    pub fn cabe_el_editor(&self, ancho_fila: f32) -> bool {
        let necesario = self.sidebar_width
            + COLUMN_GAP
            + MIN_CHAT_WIDTH
            + COLUMN_GAP
            + MIN_EDITOR_WIDTH
            + if self.background_panel_visible {
                COLUMN_GAP + MIN_BACKGROUND_WIDTH
            } else {
                0.0
            };
        ancho_fila >= necesario
    }

    pub fn set_main_content_bounds(&mut self, bounds: Bounds, split_gap: f32) {
        self.main_content_bounds = Some(bounds);
        self.main_split_gap = split_gap.max(0.0);
    }

    /// Actualiza bounds del handle de resize de sidebar.
    pub fn set_sidebar_resizer_bounds(&mut self, bounds: Bounds) {
        self.sidebar_resizer_bounds = Some(bounds);
    }

    /// Actualiza bounds del handle de resize editor/chat.
    pub fn set_editor_resizer_bounds(&mut self, bounds: Bounds) {
        self.editor_resizer_bounds = Some(bounds);
    }

    /// Inicia drag de resize si el cursor cae sobre un handle.
    pub fn begin_layout_resize(&mut self, x: f32, y: f32) -> bool {
        if self.is_overlay_active() {
            return false;
        }
        if self
            .sidebar_resizer_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            self.dragging_sidebar_resizer = true;
            self.dragging_editor_resizer = false;
            self.dragging_editor_pane_resizer = false;
            self.needs_render = true;
            return true;
        }
        if self
            .editor_resizer_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            self.dragging_editor_resizer = true;
            self.dragging_sidebar_resizer = false;
            self.dragging_editor_pane_resizer = false;
            self.needs_render = true;
            return true;
        }
        if self.is_editor_split_active()
            && self
                .editor_pane_resizer_bounds
                .as_ref()
                .map(|b| b.contains(x, y))
                .unwrap_or(false)
        {
            self.dragging_editor_pane_resizer = true;
            self.dragging_sidebar_resizer = false;
            self.dragging_editor_resizer = false;
            self.needs_render = true;
            return true;
        }
        false
    }

    /// Indica si hay un drag activo de layout.
    pub fn is_layout_resizing(&self) -> bool {
        self.dragging_sidebar_resizer
            || self.dragging_editor_resizer
            || self.dragging_editor_pane_resizer
    }

    /// Actualiza medidas durante drag de resize.
    pub fn update_layout_resize_from_point(&mut self, x: f32, _y: f32) -> bool {
        let mut changed = false;

        if self.dragging_sidebar_resizer {
            let measured_width = match self.explorer_dock {
                PanelDock::Left => x,
                PanelDock::Right => self.window_width as f32 - x,
            };
            let next_width = measured_width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
            if (next_width - self.sidebar_width).abs() >= 0.5 {
                self.sidebar_width = next_width;
                changed = true;
            }
        }

        if self.dragging_editor_resizer {
            if let Some(main_bounds) = self.main_content_bounds.as_ref() {
                let available = (main_bounds.width - self.main_split_gap).max(220.0);
                // El editor y el chat no comparten suelo: la maqueta pide 340
                // para el editor y reserva 320 para el chat. Cuando la ventana
                // no da para ambos se reparten a partes iguales, que es peor
                // que respetar los mínimos pero mejor que dejar una columna de
                // cuarenta píxeles.
                let (min_editor, min_chat) = if available >= MIN_EDITOR_WIDTH + MIN_CHAT_WIDTH {
                    (MIN_EDITOR_WIDTH, MIN_CHAT_WIDTH)
                } else {
                    let mitad = (available * 0.5).max(80.0);
                    (mitad, mitad)
                };
                let max_editor = (available - min_chat).max(min_editor);
                let target_left = (x - main_bounds.x).max(0.0);
                let target_editor = match self.chat_dock {
                    PanelDock::Left => available - target_left,
                    PanelDock::Right => target_left,
                };
                let next_editor = target_editor.clamp(min_editor, max_editor);
                let next_ratio =
                    (next_editor / available).clamp(min_editor / available, 1.0 - min_chat / available);
                if (next_ratio - self.editor_split_ratio).abs() >= 0.002 {
                    self.editor_split_ratio = next_ratio;
                    changed = true;
                }
            }
        }

        if self.dragging_editor_pane_resizer {
            if let (Some(primary), Some(secondary)) = (
                self.editor_primary_bounds.as_ref(),
                self.editor_secondary_bounds.as_ref(),
            ) {
                let available = (primary.width + secondary.width).max(220.0);
                let min_panel = 140.0_f32.min((available - 40.0).max(80.0));
                let max_primary = (available - min_panel).max(min_panel);
                let target_left = (x - primary.x).max(0.0);
                let next_primary = target_left.clamp(min_panel, max_primary);
                let next_ratio = (next_primary / available).clamp(0.2, 0.8);
                if (next_ratio - self.editor_pane_split_ratio).abs() >= 0.002 {
                    self.editor_pane_split_ratio = next_ratio;
                    changed = true;
                }
            }
        }

        if changed {
            self.needs_render = true;
        }
        changed
    }

    /// Finaliza cualquier drag de resize de layout.
    pub fn end_layout_resize(&mut self) {
        if self.dragging_sidebar_resizer
            || self.dragging_editor_resizer
            || self.dragging_editor_pane_resizer
        {
            self.dragging_sidebar_resizer = false;
            self.dragging_editor_resizer = false;
            self.dragging_editor_pane_resizer = false;
            self.persist_layout_snapshot();
            self.status_text = format!(
                "layout {:.0}px | main {:.0}% | pane {:.0}%",
                self.sidebar_width,
                self.editor_split_ratio * 100.0,
                self.editor_pane_split_ratio * 100.0
            );
            self.needs_render = true;
        }
    }

    /// Restaura layout por defecto y lo persiste.
    pub fn reset_layout(&mut self) {
        self.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
        self.editor_split_ratio = DEFAULT_EDITOR_SPLIT_RATIO;
        self.editor_pane_split_ratio = DEFAULT_EDITOR_PANE_SPLIT_RATIO;
        self.explorer_dock = PanelDock::Left;
        self.chat_dock = PanelDock::Right;
        self.editor_pane_split_enabled = false;
        self.secondary_tab = None;
        self.focused_editor_pane = EditorPane::Primary;
        self.dragging_sidebar_resizer = false;
        self.dragging_editor_resizer = false;
        self.dragging_editor_pane_resizer = false;
        self.persist_layout_snapshot();
        self.status_text = "layout reset".to_string();
        self.needs_render = true;
    }

    pub fn selected_ai_model(&self) -> &str {
        &self.selected_ai_model
    }

    pub fn cycle_ai_model(&mut self) {
        let next = AI_MODEL_OPTIONS
            .iter()
            .position(|model| *model == self.selected_ai_model)
            .map(|index| (index + 1) % AI_MODEL_OPTIONS.len())
            .unwrap_or(0);
        self.selected_ai_model = AI_MODEL_OPTIONS[next].to_string();
        self.persist_layout_snapshot();
        self.status_text = format!("model {}", self.selected_ai_model);
        self.needs_render = true;
    }

    fn set_explorer_dock(&mut self, dock: PanelDock) {
        self.explorer_dock = dock;
        self.persist_layout_snapshot();
        self.status_text = format!("explorer {}", dock.config_value());
        self.needs_render = true;
    }

    fn set_chat_dock(&mut self, dock: PanelDock) {
        self.chat_dock = dock;
        self.persist_layout_snapshot();
        self.status_text = format!("chat {}", dock.config_value());
        self.needs_render = true;
    }

    /// Actualiza bounds del panel de resultados de búsqueda.
    pub fn set_search_results_bounds(&mut self, bounds: Bounds) {
        self.search_results_bounds = Some(bounds);
    }

    /// Limpia bounds del panel de resultados de búsqueda.
    pub fn clear_search_results_bounds(&mut self) {
        self.search_results_bounds = None;
    }

    /// Editor activo actual (invariante: siempre existe al menos una pestaña).
    pub fn active_editor(&self) -> &Editor {
        self.editor_for_pane(self.focused_editor_pane)
    }

    /// Editor activo mutable.
    pub fn active_editor_mut(&mut self) -> &mut Editor {
        self.editor_for_pane_mut(self.focused_editor_pane)
    }

    /// Editor asociado a un panel específico.
    pub fn editor_for_pane(&self, pane: EditorPane) -> &Editor {
        let index = self.pane_tab_index(pane);
        &self.open_tabs[index].editor
    }

    /// Editor asociado a un panel específico (mutable).
    pub fn editor_for_pane_mut(&mut self, pane: EditorPane) -> &mut Editor {
        let index = self.pane_tab_index(pane);
        &mut self.open_tabs[index].editor
    }

    /// Ruta de archivo activa, si corresponde a un archivo real.
    pub fn active_file_path(&self) -> Option<&PathBuf> {
        self.file_path_for_pane(self.focused_editor_pane)
    }

    /// Ruta de archivo asociada a un panel.
    pub fn file_path_for_pane(&self, pane: EditorPane) -> Option<&PathBuf> {
        self.open_tabs[self.pane_tab_index(pane)].path.as_ref()
    }

    /// Snapshot de tabs para render sin préstamos largos.
    pub fn tabs_snapshot_for_pane(&self, pane: EditorPane) -> Vec<TabSnapshot> {
        let active_index = self.pane_tab_index(pane);
        self.open_tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let mut title = tab
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| tab.editor.buffer_name().to_string());
                let modified = tab.editor.is_modified();
                if modified {
                    title = format!("*{}", title);
                }
                TabSnapshot {
                    index,
                    title,
                    modified,
                    active: index == active_index,
                }
            })
            .collect()
    }

    /// Snapshot de tabs del panel actualmente enfocado.
    pub fn tabs_snapshot(&self) -> Vec<TabSnapshot> {
        self.tabs_snapshot_for_pane(self.focused_editor_pane)
    }

    /// Cambia pestaña activa en un panel específico.
    pub fn switch_tab_in_pane(&mut self, pane: EditorPane, index: usize) {
        if index >= self.open_tabs.len() {
            return;
        }
        self.set_tab_for_pane(pane, index);
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        self.set_focus(match pane {
            EditorPane::Primary => FocusTarget::EditorPrimary,
            EditorPane::Secondary => FocusTarget::EditorSecondary,
        });
        let title = self
            .open_tabs
            .get(index)
            .and_then(|tab| tab.path.as_ref())
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.active_editor().buffer_name().to_string());
        self.status_text = format!("tab {}", title);
        let _ = self.reveal_active_file_in_explorer();
        self.refresh_search_matches_if_active();
        self.needs_render = true;
    }

    /// Cambia pestaña activa del panel actualmente enfocado.
    pub fn switch_tab(&mut self, index: usize) {
        self.switch_tab_in_pane(self.focused_editor_pane, index);
    }

    /// Cierra la pestaña del panel enfocado si hay más de una.
    fn close_focused_tab_force(&mut self) {
        if self.open_tabs.len() <= 1 {
            self.status_text = "cannot close last tab".to_string();
            self.needs_render = true;
            return;
        }
        let pane = self.focused_editor_pane;
        let target_index = self.pane_tab_index(pane);
        let removed = self.open_tabs.remove(target_index);

        let remap = |idx: usize| {
            if idx > target_index {
                idx - 1
            } else {
                idx
            }
        };

        if self.open_tabs.is_empty() {
            self.open_tabs.push(OpenTab {
                path: None,
                editor: Editor::new(),
            });
            self.active_tab = 0;
            self.secondary_tab = None;
            self.editor_pane_split_enabled = false;
            self.focused_editor_pane = EditorPane::Primary;
        } else {
            self.active_tab = if self.active_tab == target_index {
                target_index.min(self.open_tabs.len().saturating_sub(1))
            } else {
                remap(self.active_tab).min(self.open_tabs.len().saturating_sub(1))
            };

            self.secondary_tab = match self.secondary_tab {
                Some(sec) if sec == target_index => {
                    if self.open_tabs.len() <= 1 {
                        None
                    } else {
                        Some(target_index.min(self.open_tabs.len().saturating_sub(1)))
                    }
                }
                Some(sec) => Some(remap(sec).min(self.open_tabs.len().saturating_sub(1))),
                None => None,
            };
        }

        self.normalize_editor_panes();
        let closed_name = removed
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("untitled");
        self.status_text = format!("closed {}", closed_name);
        self.pending_close_tab_confirm = None;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        let _ = self.reveal_active_file_in_explorer();
        self.refresh_search_matches_if_active();
        self.persist_session_snapshot();
        self.needs_render = true;
    }

    /// Solicita cierre de pestaña activa, con confirmación para cambios sin guardar.
    pub fn request_close_active_tab(&mut self) {
        if self.open_tabs.len() <= 1 {
            self.status_text = "cannot close last tab".to_string();
            self.pending_close_tab_confirm = None;
            self.needs_render = true;
            return;
        }

        let target_index = self.focused_tab_index();
        if self.active_editor().is_modified() {
            if self.pending_close_tab_confirm == Some(target_index) {
                self.close_focused_tab_force();
            } else {
                self.pending_close_tab_confirm = Some(target_index);
                self.pending_exit_confirm = false;
                self.status_text =
                    "unsaved tab: Ctrl+W again to close, or Ctrl+S to save".to_string();
                self.needs_render = true;
            }
            return;
        }

        self.close_focused_tab_force();
    }

    /// Avanza a la siguiente pestaña.
    pub fn cycle_tabs(&mut self) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        let pane = self.focused_editor_pane;
        let current = self.pane_tab_index(pane);
        let next = (current + 1) % self.open_tabs.len();
        self.set_tab_for_pane(pane, next);
        self.refresh_search_matches_if_active();
        self.needs_render = true;
    }

    /// Retrocede a la pestaña anterior.
    pub fn cycle_tabs_previous(&mut self) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        let pane = self.focused_editor_pane;
        let current = self.pane_tab_index(pane);
        let previous = if current == 0 {
            self.open_tabs.len() - 1
        } else {
            current - 1
        };
        self.set_tab_for_pane(pane, previous);
        self.refresh_search_matches_if_active();
        self.needs_render = true;
    }

    /// Crea una nueva pestaña vacía en el editor.
    pub fn open_new_tab(&mut self) {
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        self.open_tabs.push(OpenTab {
            path: None,
            editor: Editor::new(),
        });
        let new_index = self.open_tabs.len().saturating_sub(1);
        let pane = self.focused_editor_pane;
        self.set_tab_for_pane(pane, new_index);
        self.set_focus(match pane {
            EditorPane::Primary => FocusTarget::EditorPrimary,
            EditorPane::Secondary => FocusTarget::EditorSecondary,
        });
        self.status_text = "new tab".to_string();
        self.refresh_search_matches_if_active();
        self.persist_session_snapshot();
        self.needs_render = true;
    }

    /// Abre el workspace actual en una ventana independiente del editor.
    pub fn open_new_window(&mut self) {
        let executable = match std::env::current_exe() {
            Ok(path) => path,
            Err(err) => {
                self.status_text = format!("new window failed: {}", err);
                self.needs_render = true;
                return;
            }
        };

        match Command::new(executable)
            .current_dir(&self.workspace_root)
            .spawn()
        {
            Ok(_) => self.status_text = "new window opened".to_string(),
            Err(err) => self.status_text = format!("new window failed: {}", err),
        }
        self.needs_render = true;
    }

    /// Indica si existe al menos una pestaña con cambios no guardados.
    pub fn has_unsaved_tabs(&self) -> bool {
        self.open_tabs.iter().any(|tab| tab.editor.is_modified())
    }

    /// Solicita cierre de la app, con confirmación si hay cambios pendientes.
    pub fn request_app_exit(&mut self) -> bool {
        if !self.has_unsaved_tabs() {
            return true;
        }

        if self.pending_exit_confirm {
            return true;
        }

        self.pending_exit_confirm = true;
        self.pending_close_tab_confirm = None;
        self.status_text =
            "unsaved tabs: close again (or Ctrl+Q) to force exit, Ctrl+S to save".to_string();
        self.needs_render = true;
        false
    }

    /// Enfoca explícitamente un target.
    pub fn set_focus(&mut self, focus: FocusTarget) {
        let (editor, input, pane, status) = match focus {
            FocusTarget::EditorPrimary => (true, false, EditorPane::Primary, "focus: editor:left"),
            FocusTarget::EditorSecondary => {
                if self.is_editor_split_active() {
                    (true, false, EditorPane::Secondary, "focus: editor:right")
                } else {
                    (true, false, EditorPane::Primary, "focus: editor:left")
                }
            }
            FocusTarget::ChatInput => (false, true, self.focused_editor_pane, "focus: chat"),
        };
        if self.editor_focused != editor
            || self.input_focused != input
            || self.focused_editor_pane != pane
        {
            self.editor_focused = editor;
            self.input_focused = input;
            self.focused_editor_pane = pane;
            self.status_text = status.to_string();
            self.needs_render = true;
        }
    }

    /// Alterna foco entre editor y chat.
    pub fn toggle_focus(&mut self) {
        if self.editor_focused
            && self.focused_editor_pane == EditorPane::Primary
            && self.is_editor_split_active()
        {
            self.set_focus(FocusTarget::EditorSecondary);
        } else if self.editor_focused {
            self.set_focus(FocusTarget::ChatInput);
        } else {
            self.set_focus(FocusTarget::EditorPrimary);
        }
    }

    fn focus_target_at(&self, x: f32, y: f32) -> Option<FocusTarget> {
        if self
            .input_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            return Some(FocusTarget::ChatInput);
        }
        if self
            .editor_secondary_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            return Some(FocusTarget::EditorSecondary);
        }
        if self
            .editor_primary_bounds
            .as_ref()
            .map(|b| b.contains(x, y))
            .unwrap_or(false)
        {
            return Some(FocusTarget::EditorPrimary);
        }
        None
    }

    /// Aplica foco según click de mouse en superficies principales.
    pub fn update_focus_from_click(&mut self, x: f32, y: f32) {
        if let Some(target) = self.focus_target_at(x, y) {
            self.set_focus(target);
        }
    }

    /// Recarga el árbol del explorador desde el workspace.
    pub fn refresh_explorer(&mut self) {
        let previous_selection = self.explorer_selected_path.clone();
        self.explorer_entries.clear();

        // Sin proyecto abierto no hay árbol que mostrar.
        if !self.workspace_is_open() {
            self.explorer_selected_path = None;
            return;
        }

        let root_name = self
            .workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.workspace_root.display().to_string());

        self.explorer_entries.push(ExplorerEntry {
            path: self.workspace_root.clone(),
            name: root_name,
            depth: 0,
            is_dir: true,
        });

        if self.is_dir_expanded(&self.workspace_root) {
            Self::collect_explorer_entries(
                &self.workspace_root,
                &self.workspace_root,
                1,
                8,
                1500,
                &self.expanded_dirs,
                self.explorer_show_noise,
                &mut self.explorer_entries,
            );
        }

        let max_scroll = self.explorer_entries.len().saturating_sub(1);
        if self.explorer_scroll > max_scroll {
            self.explorer_scroll = max_scroll;
        }

        let selection = previous_selection
            .filter(|path| {
                self.explorer_entries
                    .iter()
                    .any(|entry| &entry.path == path)
            })
            .or_else(|| {
                self.active_file_path().cloned().and_then(|path| {
                    self.explorer_entries
                        .iter()
                        .any(|entry| entry.path == path)
                        .then_some(path)
                })
            })
            .or_else(|| {
                self.explorer_entries
                    .first()
                    .map(|entry| entry.path.clone())
            });
        self.explorer_selected_path = selection;
    }

    /// Índice actualmente seleccionado en explorer.
    pub fn explorer_selected_index(&self) -> Option<usize> {
        let selected = self.explorer_selected_path.as_ref()?;
        self.explorer_entries
            .iter()
            .position(|entry| &entry.path == selected)
    }

    /// Selecciona una entrada por índice en el explorer.
    pub fn set_explorer_selection_index(&mut self, index: usize) -> bool {
        let Some(entry) = self.explorer_entries.get(index) else {
            return false;
        };
        self.explorer_selected_path = Some(entry.path.clone());
        self.needs_render = true;
        true
    }

    /// Navega selección del explorer por teclado.
    pub fn move_explorer_selection(&mut self, delta: i32) -> bool {
        if self.explorer_entries.is_empty() || delta == 0 {
            return false;
        }
        let current = self.explorer_selected_index().unwrap_or(0) as i32;
        let max = self.explorer_entries.len().saturating_sub(1) as i32;
        let next = (current + delta).clamp(0, max) as usize;
        if self.set_explorer_selection_index(next) {
            self.status_text = format!("explorer {}", self.explorer_entries[next].name);
            return true;
        }
        false
    }

    /// Expande/colapsa o abre la entrada seleccionada en explorer.
    pub fn activate_explorer_selection(&mut self) -> bool {
        let Some(index) = self.explorer_selected_index() else {
            return false;
        };
        let Some(entry) = self.explorer_entries.get(index).cloned() else {
            return false;
        };
        if entry.is_dir {
            self.toggle_dir_expanded(entry.path);
        } else {
            self.open_file_from_explorer(entry.path);
        }
        true
    }

    /// Expande la carpeta seleccionada o abre el archivo seleccionado.
    pub fn expand_explorer_selection(&mut self) -> bool {
        let Some(index) = self.explorer_selected_index() else {
            return false;
        };
        let Some(entry) = self.explorer_entries.get(index).cloned() else {
            return false;
        };

        if entry.is_dir {
            if !self.is_dir_expanded(&entry.path) {
                self.toggle_dir_expanded(entry.path);
            }
            true
        } else {
            self.open_file_from_explorer(entry.path);
            true
        }
    }

    /// Colapsa la carpeta seleccionada o sube la selección al padre.
    pub fn collapse_explorer_selection(&mut self) -> bool {
        let Some(index) = self.explorer_selected_index() else {
            return false;
        };
        let Some(entry) = self.explorer_entries.get(index).cloned() else {
            return false;
        };

        if entry.is_dir && self.is_dir_expanded(&entry.path) {
            self.toggle_dir_expanded(entry.path);
            return true;
        }

        let mut parent = entry.path.parent();
        while let Some(candidate) = parent {
            if !candidate.starts_with(&self.workspace_root) {
                break;
            }
            if let Some(parent_idx) = self
                .explorer_entries
                .iter()
                .position(|item| item.path == candidate)
            {
                self.set_explorer_selection_index(parent_idx);
                self.status_text = format!("explorer {}", self.explorer_entries[parent_idx].name);
                return true;
            }
            parent = candidate.parent();
        }

        false
    }

    /// Revela archivo activo en explorer expandiendo su ruta.
    pub fn reveal_active_file_in_explorer(&mut self) -> bool {
        let Some(path) = self.active_file_path().cloned() else {
            return false;
        };
        self.reveal_path_in_explorer(&path)
    }

    fn reveal_path_in_explorer(&mut self, path: &Path) -> bool {
        if !path.starts_with(&self.workspace_root) {
            return false;
        }

        let mut current = if path.is_dir() {
            Some(path)
        } else {
            path.parent()
        };
        while let Some(dir) = current {
            if !dir.starts_with(&self.workspace_root) {
                break;
            }
            self.expanded_dirs.insert(dir.to_path_buf());
            if dir == self.workspace_root {
                break;
            }
            current = dir.parent();
        }
        self.refresh_explorer();
        let target = path.to_path_buf();
        self.explorer_selected_path = Some(target.clone());
        if let Some(index) = self
            .explorer_entries
            .iter()
            .position(|entry| entry.path == target)
        {
            if index < self.explorer_scroll {
                self.explorer_scroll = index;
            }
        }
        self.needs_render = true;
        true
    }

    fn explorer_target_dir(&self) -> PathBuf {
        let selected = self
            .explorer_selected_path
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.workspace_root.clone());
        if selected.is_dir() {
            return selected;
        }
        selected
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.workspace_root.clone())
    }

    fn unique_child_path(parent: &Path, stem: &str, extension: Option<&str>) -> PathBuf {
        for idx in 1..=9_999usize {
            let suffix = if idx == 1 {
                String::new()
            } else {
                format!("_{}", idx)
            };
            let file_name = match extension {
                Some(ext) if !ext.is_empty() => format!("{}{}.{}", stem, suffix, ext),
                _ => format!("{}{}", stem, suffix),
            };
            let candidate = parent.join(file_name);
            if !candidate.exists() {
                return candidate;
            }
        }
        parent.join(format!("{}_overflow", stem))
    }

    /// Crea un archivo nuevo en el directorio seleccionado del explorer.
    pub fn explorer_create_new_file(&mut self) {
        let parent = self.explorer_target_dir();
        if !parent.starts_with(&self.workspace_root) {
            self.status_text = "explorer new file blocked: outside workspace".to_string();
            self.needs_render = true;
            return;
        }
        if let Err(err) = fs::create_dir_all(&parent) {
            self.status_text = format!("explorer new file failed: {}", err);
            self.needs_render = true;
            return;
        }
        let path = Self::unique_child_path(&parent, "new_file", Some("txt"));
        match fs::write(&path, "") {
            Ok(_) => {
                self.mark_workspace_mutated();
                self.refresh_explorer();
                let _ = self.reveal_path_in_explorer(&path);
                self.open_file_from_explorer(path.clone());
                self.status_text = format!(
                    "explorer new file: {}",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("new_file.txt")
                );
                self.needs_render = true;
            }
            Err(err) => {
                self.status_text = format!("explorer new file failed: {}", err);
                self.needs_render = true;
            }
        }
    }

    /// Crea una carpeta nueva en el directorio seleccionado del explorer.
    pub fn explorer_create_new_folder(&mut self) {
        let parent = self.explorer_target_dir();
        if !parent.starts_with(&self.workspace_root) {
            self.status_text = "explorer new folder blocked: outside workspace".to_string();
            self.needs_render = true;
            return;
        }
        if let Err(err) = fs::create_dir_all(&parent) {
            self.status_text = format!("explorer new folder failed: {}", err);
            self.needs_render = true;
            return;
        }
        let path = Self::unique_child_path(&parent, "new_folder", None);
        match fs::create_dir(&path) {
            Ok(_) => {
                self.mark_workspace_mutated();
                self.expanded_dirs.insert(path.clone());
                self.refresh_explorer();
                let _ = self.reveal_path_in_explorer(&path);
                self.status_text = format!(
                    "explorer new folder: {}",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("new_folder")
                );
                self.needs_render = true;
            }
            Err(err) => {
                self.status_text = format!("explorer new folder failed: {}", err);
                self.needs_render = true;
            }
        }
    }

    /// Recorre `dir` acumulando entradas visibles del explorador.
    ///
    /// `root` es la raíz del proyecto y delimita lo que puede mostrarse: la
    /// guardia oculta los secretos siempre y los artefactos generados salvo que
    /// `show_noise` lo permita.
    #[allow(clippy::too_many_arguments)]
    fn collect_explorer_entries(
        root: &Path,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        max_entries: usize,
        expanded_dirs: &HashSet<PathBuf>,
        show_noise: bool,
        out: &mut Vec<ExplorerEntry>,
    ) {
        if depth > max_depth || out.len() >= max_entries {
            return;
        }

        if !matches!(workspace_guard::classify(root, dir), Access::Allowed) {
            return;
        }

        let read_dir = match fs::read_dir(dir) {
            Ok(v) => v,
            Err(_) => return,
        };

        let mut dirs: Vec<(String, PathBuf, bool)> = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();

        for entry in read_dir.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            let path = entry.path();

            let visible = match workspace_guard::classify(root, &path) {
                Access::Allowed => true,
                Access::Noise => show_noise,
                Access::Secret | Access::Outside => false,
            };
            if !visible {
                continue;
            }

            if file_type.is_dir() {
                dirs.push((name, path, file_type.is_symlink()));
            } else {
                files.push((name, path));
            }
        }

        dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        for (name, path, is_symlink) in dirs {
            if out.len() >= max_entries {
                return;
            }
            out.push(ExplorerEntry {
                path: path.clone(),
                name,
                depth,
                is_dir: true,
            });

            if !is_symlink && expanded_dirs.contains(&path) {
                Self::collect_explorer_entries(
                    root,
                    &path,
                    depth + 1,
                    max_depth,
                    max_entries,
                    expanded_dirs,
                    show_noise,
                    out,
                );
            }
        }

        for (name, path) in files {
            if out.len() >= max_entries {
                return;
            }
            out.push(ExplorerEntry {
                path,
                name,
                depth,
                is_dir: false,
            });
        }
    }

    /// Verifica si un directorio está expandido en el explorer.
    pub fn is_dir_expanded(&self, path: &Path) -> bool {
        self.expanded_dirs.contains(path)
    }

    /// Alterna estado expandido/colapsado de directorio.
    pub fn toggle_dir_expanded(&mut self, path: PathBuf) {
        if self.expanded_dirs.contains(&path) {
            self.expanded_dirs.remove(&path);
        } else {
            self.expanded_dirs.insert(path);
        }
        self.refresh_explorer();
        self.needs_render = true;
    }

    fn open_file_in_pane(&mut self, path: PathBuf, pane: EditorPane, from_side_action: bool) {
        if !path.is_file() {
            return;
        }

        // La guardia decide antes que el sistema de archivos.
        let access = self.access_to(&path);
        if !access.is_allowed() {
            self.status_text = format!("Acceso denegado ({}): {}", access.reason(), path.display());
            self.needs_render = true;
            return;
        }

        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        let mut existing_idx = self
            .open_tabs
            .iter()
            .position(|tab| tab.path.as_ref().map(|p| p == &path).unwrap_or(false));

        if pane == EditorPane::Secondary && existing_idx == Some(self.active_tab) {
            // Para "open to side", permitir duplicar el archivo activo en tab aparte.
            existing_idx = None;
        }

        let target_index = if let Some(index) = existing_idx {
            index
        } else {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let mut editor =
                        Editor::open_file(path.clone(), &content, &self.language_registry);
                    editor.mark_saved();
                    self.open_tabs.push(OpenTab {
                        path: Some(path.clone()),
                        editor,
                    });
                    self.open_tabs.len().saturating_sub(1)
                }
                Err(err) => {
                    self.status_text = format!("open failed: {}", err);
                    self.needs_render = true;
                    return;
                }
            }
        };

        if pane == EditorPane::Secondary && self.open_tabs.len() > 1 {
            self.editor_pane_split_enabled = true;
        }
        self.set_tab_for_pane(pane, target_index);
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.mouse_selection_anchor = None;
        self.last_editor_click = None;
        self.set_focus(match pane {
            EditorPane::Primary => FocusTarget::EditorPrimary,
            EditorPane::Secondary => FocusTarget::EditorSecondary,
        });
        self.status_text = if from_side_action {
            format!("opened to side {}", path.display())
        } else if existing_idx.is_some() {
            format!("switched {}", path.display())
        } else {
            format!("opened {}", path.display())
        };
        let _ = self.reveal_active_file_in_explorer();
        self.refresh_search_matches_if_active();
        if pane == EditorPane::Secondary {
            self.persist_layout_snapshot();
        }
        self.persist_session_snapshot();
        self.needs_render = true;
    }

    /// Abre un archivo del explorador en el panel enfocado.
    pub fn open_file_from_explorer(&mut self, path: PathBuf) {
        self.open_file_in_pane(path, self.focused_editor_pane, false);
    }

    /// Abre un archivo en panel secundario y activa split si hace falta.
    pub fn open_file_to_side(&mut self, path: PathBuf) {
        self.open_file_in_pane(path, EditorPane::Secondary, true);
    }

    /// Acción "Open to Side": usa selección actual de explorer o archivo activo.
    pub fn open_selected_or_active_to_side(&mut self) {
        let candidate = self
            .explorer_selected_path
            .as_ref()
            .filter(|path| path.is_file())
            .cloned()
            .or_else(|| self.active_file_path().cloned());

        if let Some(path) = candidate {
            self.open_file_to_side(path);
        } else {
            self.status_text = "open to side: no file selected".to_string();
            self.needs_render = true;
        }
    }

    /// Abre picker nativo de archivo y carga el resultado en una pestaña.
    /// Ejecutado en un hilo de sistema separado con un runtime Tokio local
    /// para evitar panics de zbus bajo Wayland.
    pub fn open_file_picker(&mut self) {
        let start_dir = self
            .active_file_path()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| self.workspace_root.clone());

        if let Some(proxy) = self.event_proxy.clone() {
            std::thread::spawn(move || {
                let result: Option<PathBuf> = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()
                    .and_then(|rt| {
                        rt.block_on(async {
                            AsyncFileDialog::new()
                                .set_directory(&start_dir)
                                .pick_file()
                                .await
                                .map(|h: FileHandle| h.path().to_path_buf())
                        })
                    });
                if let Some(path) = result {
                    let _ = proxy.send_event(AppEvent::FileOpened(path));
                }
            });
        }
    }

    /// Abre picker nativo de carpeta y establece el workspace_root en la ruta elegida.
    /// Ejecutado en un hilo de sistema separado con un runtime Tokio local.
    pub fn open_folder_picker(&mut self) {
        // Sin proyecto abierto la raíz está vacía: el selector debe partir de
        // algún sitio con sentido, no del directorio de trabajo del proceso.
        let start_dir = if self.workspace_is_open() {
            self.workspace_root.clone()
        } else {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/"))
        };

        if let Some(proxy) = self.event_proxy.clone() {
            std::thread::spawn(move || {
                let result: Option<PathBuf> = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()
                    .and_then(|rt| {
                        rt.block_on(async {
                            AsyncFileDialog::new()
                                .set_directory(&start_dir)
                                .pick_folder()
                                .await
                                .map(|h: FileHandle| h.path().to_path_buf())
                        })
                    });
                if let Some(path) = result {
                    let _ = proxy.send_event(AppEvent::FolderOpened(path));
                }
            });
        }
    }

    /// Indica si hay un overlay de selección activo.
    pub fn is_overlay_active(&self) -> bool {
        self.overlay_mode.is_some()
    }

    /// Menú superior actualmente desplegado (si existe).
    pub fn top_menu_open(&self) -> Option<TopMenuKind> {
        self.top_menu_open
    }

    /// Índice seleccionado en el menú superior activo.
    pub fn top_menu_selected_index(&self) -> usize {
        self.top_menu_selected_index
    }

    /// Abre un menú superior y posiciona selección en su primer elemento.
    pub fn open_top_menu(&mut self, menu: TopMenuKind) {
        self.top_menu_open = Some(menu);
        self.top_menu_selected_index = 0;
        self.status_text = format!("menu: {}", menu.label());
        self.needs_render = true;
    }

    /// Alterna apertura/cierre del menú superior indicado.
    pub fn toggle_top_menu(&mut self, menu: TopMenuKind) {
        if self.top_menu_open == Some(menu) {
            self.top_menu_open = None;
            self.top_menu_selected_index = 0;
            self.status_text = "menu closed".to_string();
        } else {
            self.open_top_menu(menu);
        }
        self.needs_render = true;
    }

    /// Cierra el menú superior desplegado.
    pub fn close_top_menu(&mut self) {
        if self.top_menu_open.is_some() {
            self.top_menu_open = None;
            self.top_menu_selected_index = 0;
            self.needs_render = true;
        }
    }

    fn cycle_top_menu(&mut self, delta: i32) -> bool {
        let Some(current) = self.top_menu_open else {
            return false;
        };
        let next = if delta >= 0 {
            current.next()
        } else {
            current.previous()
        };
        self.top_menu_open = Some(next);
        self.top_menu_selected_index = 0;
        self.status_text = format!("menu: {}", next.label());
        self.needs_render = true;
        true
    }

    fn move_top_menu_selection(&mut self, delta: i32) -> bool {
        let Some(menu) = self.top_menu_open else {
            return false;
        };
        let entries = top_menu_entries(menu);
        if entries.is_empty() {
            self.top_menu_selected_index = 0;
            return false;
        }
        let len = entries.len() as i32;
        let current = self.top_menu_selected_index as i32;
        let next = (current + delta).rem_euclid(len) as usize;
        self.top_menu_selected_index = next;
        self.status_text = format!("menu item: {}", entries[next].label);
        self.needs_render = true;
        true
    }

    fn execute_top_menu_selected(&mut self) -> bool {
        let Some(menu) = self.top_menu_open else {
            return false;
        };
        let entries = top_menu_entries(menu);
        if entries.is_empty() {
            self.close_top_menu();
            return true;
        }
        let index = self
            .top_menu_selected_index
            .min(entries.len().saturating_sub(1));
        let action = entries[index].action;
        self.close_top_menu();
        self.execute_command_palette_action(action);
        true
    }

    /// Maneja navegación de menús superiores por teclado.
    pub fn handle_top_menu_key(&mut self, key: &Key, shift: bool, alt: bool) -> bool {
        if alt {
            if let Key::Character(ch) = key {
                let lower = ch.to_lowercase();
                if let Some(menu) = TopMenuKind::from_mnemonic(lower.as_str()) {
                    self.open_top_menu(menu);
                    return true;
                }
            }
        }

        let Some(menu) = self.top_menu_open else {
            return false;
        };

        match key {
            Key::Named(NamedKey::Escape) => {
                self.close_top_menu();
                true
            }
            Key::Named(NamedKey::ArrowLeft) => self.cycle_top_menu(-1),
            Key::Named(NamedKey::ArrowRight) => self.cycle_top_menu(1),
            Key::Named(NamedKey::ArrowUp) => self.move_top_menu_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_top_menu_selection(1),
            Key::Named(NamedKey::Tab) => {
                if shift {
                    self.cycle_top_menu(-1)
                } else {
                    self.cycle_top_menu(1)
                }
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                self.execute_top_menu_selected()
            }
            Key::Character(ch) => {
                let needle = ch.to_lowercase();
                let entries = top_menu_entries(menu);
                if let Some((idx, _)) = entries
                    .iter()
                    .enumerate()
                    .find(|(_, entry)| entry.label.to_lowercase().starts_with(&needle))
                {
                    self.top_menu_selected_index = idx;
                    self.status_text = format!("menu item: {}", entries[idx].label);
                    self.needs_render = true;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Abre overlay de quick open con indexación local del workspace.
    /// Empieza una conversación limpia.
    ///
    /// Vacía el hilo, nada más. La maqueta enseña además una lista de sesiones
    /// anteriores, pero eso no existe todavía: Llore no guarda conversaciones,
    /// así que un chat nuevo no archiva el viejo, lo descarta. Cuando haya
    /// persistencia de sesiones, aquí es donde se archivará antes de vaciar.
    pub fn new_chat(&mut self) {
        if self.messages.is_empty() {
            self.status_text = "chat: ya estaba vacío".to_string();
        } else {
            self.messages.clear();
            self.expanded_thoughts.clear();
            self.status_text = "chat: conversación nueva".to_string();
        }
        self.needs_render = true;
    }

    pub fn begin_quick_open(&mut self) {
        self.overlay_mode = Some(OverlayMode::QuickOpen);
        self.overlay_query.clear();
        self.overlay_selected = 0;
        if self.quick_open_cache_stale || self.quick_open_candidates.is_empty() {
            self.quick_open_candidates = self.collect_workspace_files();
            self.quick_open_cache_stale = false;
        }
        self.rebuild_quick_open_items();
        self.status_text = format!(
            "quick open: {} files indexed",
            self.quick_open_candidates.len()
        );
        self.needs_render = true;
    }

    /// Abre command palette.
    pub fn begin_command_palette(&mut self) {
        self.overlay_mode = Some(OverlayMode::CommandPalette);
        self.overlay_query.clear();
        self.overlay_selected = 0;
        self.rebuild_command_palette_items();
        self.status_text = "command palette".to_string();
        self.needs_render = true;
    }

    /// Abre overlay de salto a línea/columna del archivo activo.
    pub fn begin_go_to_line_overlay(&mut self) {
        self.overlay_mode = Some(OverlayMode::GoToLine);
        self.overlay_query.clear();
        self.overlay_selected = 0;
        self.rebuild_go_to_line_overlay_items();
        self.status_text = "go to line".to_string();
        self.needs_render = true;
    }

    /// Abre overlay de símbolos del archivo activo (outline filtrable).
    pub fn begin_symbol_overlay(&mut self) {
        self.overlay_mode = Some(OverlayMode::Symbols);
        self.overlay_query.clear();
        self.overlay_selected = 0;
        self.rebuild_symbol_overlay_items();
        self.status_text = "go to symbol".to_string();
        self.needs_render = true;
    }

    /// Abre overlay de símbolos globales del workspace.
    pub fn begin_workspace_symbol_overlay(&mut self) {
        self.overlay_mode = Some(OverlayMode::WorkspaceSymbols);
        self.overlay_query.clear();
        self.overlay_selected = 0;
        if self.quick_open_cache_stale || self.quick_open_candidates.is_empty() {
            self.quick_open_candidates = self.collect_workspace_files();
            self.quick_open_cache_stale = false;
        }
        if self.workspace_symbol_cache_stale || self.workspace_symbol_candidates.is_empty() {
            self.workspace_symbol_candidates = self.collect_workspace_symbol_candidates();
            self.workspace_symbol_cache_stale = false;
        }
        self.rebuild_workspace_symbol_overlay_items();
        self.status_text = format!(
            "workspace symbols: {} indexed",
            self.workspace_symbol_candidates.len()
        );
        self.needs_render = true;
    }

    /// Abre overlay de búsqueda textual global del workspace.
    pub fn begin_workspace_text_search_overlay(&mut self) {
        self.set_sidebar_panel(SidebarPanel::Search);
        self.overlay_mode = Some(OverlayMode::WorkspaceTextSearch);
        self.overlay_query.clear();
        self.overlay_selected = 0;
        if self.quick_open_cache_stale || self.quick_open_candidates.is_empty() {
            self.quick_open_candidates = self.collect_workspace_files();
            self.quick_open_cache_stale = false;
        }
        self.rebuild_workspace_text_search_overlay_items();
        self.status_text = format!(
            "workspace search: {} files indexed",
            self.quick_open_candidates.len()
        );
        self.needs_render = true;
    }

    /// Abre overlay de problemas operativos (estado real de sesión).
    pub fn begin_problems_overlay(&mut self) {
        self.set_sidebar_panel(SidebarPanel::Problems);
        self.overlay_mode = Some(OverlayMode::Problems);
        self.overlay_query.clear();
        self.overlay_selected = self.active_problem_index.unwrap_or(0);
        self.rebuild_problems_overlay_items();
        self.status_text = "problems list".to_string();
        self.needs_render = true;
    }

    /// Abre overlay de problemas aplicando filtro inicial de contexto.
    pub fn begin_problems_overlay_with_query(&mut self, query: &str) {
        self.begin_problems_overlay();
        self.overlay_query = query.trim().to_string();
        self.rebuild_problems_overlay_items();
        self.status_text = format!("problems filter: {}", query.trim());
        self.needs_render = true;
    }

    /// Tema visual activo.
    pub fn ui_theme(&self) -> UiTheme {
        self.ui_theme
    }

    /// Paleta del tema activo para render dinámico.
    pub fn theme_palette(&self) -> &'static ThemePalette {
        crate::theme::palette(self.ui_theme)
    }

    /// Aplica tema visual y persiste layout.
    pub fn set_ui_theme(&mut self, theme: UiTheme) {
        if self.ui_theme == theme {
            return;
        }
        self.ui_theme = theme;
        self.status_text = format!("theme: {}", theme.label());
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    pub fn cycle_ui_theme_next(&mut self) {
        self.set_ui_theme(self.ui_theme.next());
    }

    pub fn cycle_ui_theme_previous(&mut self) {
        self.set_ui_theme(self.ui_theme.previous());
    }

    /// Densidad visual activa de la UI.
    pub fn ui_density(&self) -> UiDensity {
        self.ui_density
    }

    /// Escala base de densidad para espaciados y alturas de filas.
    pub fn ui_density_scale(&self) -> f32 {
        self.ui_density.scale()
    }

    /// Aplica densidad visual y persiste layout.
    pub fn set_ui_density(&mut self, density: UiDensity) {
        if self.ui_density == density {
            return;
        }
        self.ui_density = density;
        self.status_text = format!("density: {}", density.label());
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    pub fn cycle_ui_density_next(&mut self) {
        self.set_ui_density(self.ui_density.next());
    }

    /// Aplica preset de apariencia (tema + densidad + font scale) en una sola operación.
    pub fn apply_ui_appearance_preset(&mut self, preset: UiAppearancePreset) {
        let (theme, density, font_scale): (UiTheme, UiDensity, f32) = match preset {
            UiAppearancePreset::Dev => (UiTheme::GraphiteDark, UiDensity::Compact, 0.95_f32),
            UiAppearancePreset::Focus => (UiTheme::QuironDark, UiDensity::Normal, 1.10_f32),
            UiAppearancePreset::Reading => (UiTheme::CopperLight, UiDensity::Comfortable, 1.20_f32),
        };
        self.ui_theme = theme;
        self.ui_density = density;
        self.editor_font_scale = font_scale.clamp(MIN_EDITOR_FONT_SCALE, MAX_EDITOR_FONT_SCALE);
        self.status_text = format!("appearance preset: {}", preset.label());
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    /// Restaura apariencia completa a valores por defecto.
    pub fn reset_ui_appearance(&mut self) {
        self.ui_theme = UiTheme::ModernistLight;
        self.ui_density = UiDensity::Normal;
        self.editor_font_scale = DEFAULT_EDITOR_FONT_SCALE;
        self.editor_horizontal_padding = DEFAULT_EDITOR_HORIZONTAL_PADDING;
        self.explorer_indent_step = DEFAULT_EXPLORER_INDENT_STEP;
        self.status_text = "appearance reset".to_string();
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    /// Escala de fuente activa del editor (1.0 == 100%).
    pub fn editor_font_scale(&self) -> f32 {
        self.editor_font_scale
    }

    /// Tamaño de fuente de código derivado de la escala activa.
    pub fn editor_code_font_size(&self) -> f32 {
        (EDITOR_CODE_FONT_SIZE * self.editor_font_scale).clamp(10.0, 26.0)
    }

    /// Altura de línea efectiva del editor según escala activa.
    pub fn editor_line_height(&self) -> f32 {
        (EDITOR_LINE_HEIGHT * self.editor_font_scale).clamp(14.0, 34.0)
    }

    /// Ancho aproximado de carácter efectivo según escala activa.
    pub fn editor_char_width(&self) -> f32 {
        (EDITOR_CHAR_WIDTH * self.editor_font_scale).clamp(6.0, 14.5)
    }

    /// Ajusta escala de fuente del editor y persiste layout.
    pub fn set_editor_font_scale(&mut self, scale: f32) {
        let clamped = scale.clamp(MIN_EDITOR_FONT_SCALE, MAX_EDITOR_FONT_SCALE);
        if (self.editor_font_scale - clamped).abs() < f32::EPSILON {
            return;
        }
        self.editor_font_scale = clamped;
        self.status_text = format!("editor font: {:.0}%", clamped * 100.0);
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    /// Agranda la interfaz entera. A diferencia del ajuste de fuente del editor,
    /// esto afecta a las letras y a los espaciados de todos los paneles.
    pub fn increase_ui_scale(&mut self) {
        self.ui_scale.increase();
        self.after_ui_scale_change();
    }

    pub fn decrease_ui_scale(&mut self) {
        self.ui_scale.decrease();
        self.after_ui_scale_change();
    }

    /// Devuelve la interfaz a la escala natural del monitor.
    pub fn reset_ui_scale(&mut self) {
        self.ui_scale.reset();
        self.after_ui_scale_change();
    }

    fn after_ui_scale_change(&mut self) {
        self.status_text = format!("interfaz al {:.0}%", self.ui_scale.user() * 100.0);
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    pub fn increase_editor_font_scale(&mut self) {
        self.set_editor_font_scale(self.editor_font_scale + EDITOR_FONT_SCALE_STEP);
    }

    pub fn decrease_editor_font_scale(&mut self) {
        self.set_editor_font_scale(self.editor_font_scale - EDITOR_FONT_SCALE_STEP);
    }

    pub fn reset_editor_font_scale(&mut self) {
        self.set_editor_font_scale(DEFAULT_EDITOR_FONT_SCALE);
    }

    /// Padding horizontal actual del editor (entre gutter y texto).
    pub fn editor_horizontal_padding(&self) -> f32 {
        self.editor_horizontal_padding
    }

    /// Ajusta padding horizontal del editor y persiste layout.
    pub fn set_editor_horizontal_padding(&mut self, value: f32) {
        let clamped = value.clamp(MIN_EDITOR_HORIZONTAL_PADDING, MAX_EDITOR_HORIZONTAL_PADDING);
        if (self.editor_horizontal_padding - clamped).abs() < f32::EPSILON {
            return;
        }
        self.editor_horizontal_padding = clamped;
        self.status_text = format!("editor left padding: {:.0}px", clamped);
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    pub fn increase_editor_horizontal_padding(&mut self) {
        self.set_editor_horizontal_padding(
            self.editor_horizontal_padding + EDITOR_HORIZONTAL_PADDING_STEP,
        );
    }

    pub fn decrease_editor_horizontal_padding(&mut self) {
        self.set_editor_horizontal_padding(
            self.editor_horizontal_padding - EDITOR_HORIZONTAL_PADDING_STEP,
        );
    }

    pub fn reset_editor_horizontal_padding(&mut self) {
        self.set_editor_horizontal_padding(DEFAULT_EDITOR_HORIZONTAL_PADDING);
    }

    /// Sangría actual por nivel del árbol explorer.
    pub fn explorer_indent_step(&self) -> f32 {
        self.explorer_indent_step
    }

    /// Ajusta sangría horizontal del explorer y persiste layout.
    pub fn set_explorer_indent_step(&mut self, value: f32) {
        let clamped = value.clamp(MIN_EXPLORER_INDENT_STEP, MAX_EXPLORER_INDENT_STEP);
        if (self.explorer_indent_step - clamped).abs() < f32::EPSILON {
            return;
        }
        self.explorer_indent_step = clamped;
        self.status_text = format!("explorer indent: {:.0}px", clamped);
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    pub fn increase_explorer_indent_step(&mut self) {
        self.set_explorer_indent_step(self.explorer_indent_step + EXPLORER_INDENT_STEP_DELTA);
    }

    pub fn decrease_explorer_indent_step(&mut self) {
        self.set_explorer_indent_step(self.explorer_indent_step - EXPLORER_INDENT_STEP_DELTA);
    }

    pub fn reset_explorer_indent_step(&mut self) {
        self.set_explorer_indent_step(DEFAULT_EXPLORER_INDENT_STEP);
    }

    /// Cierra overlay activo.
    pub fn close_overlay(&mut self) {
        if self.overlay_mode.is_some() {
            self.reset_overlay_state();
            self.status_text = "overlay closed".to_string();
            self.needs_render = true;
        }
    }

    fn reset_overlay_state(&mut self) {
        self.overlay_mode = None;
        self.overlay_query.clear();
        self.overlay_items.clear();
        self.overlay_selected = 0;
    }

    /// Cambia panel activo del sidebar.
    pub fn set_sidebar_panel(&mut self, panel: SidebarPanel) {
        if self.sidebar_panel == panel {
            return;
        }
        self.sidebar_panel = panel;
        self.status_text = match panel {
            SidebarPanel::Explorer => "sidebar: explorer".to_string(),
            SidebarPanel::Search => "sidebar: search".to_string(),
            SidebarPanel::Git => "sidebar: git".to_string(),
            SidebarPanel::Problems => "sidebar: problems".to_string(),
            SidebarPanel::Outline => "sidebar: outline".to_string(),
            SidebarPanel::Appearance => "sidebar: appearance".to_string(),
            SidebarPanel::Security => "sidebar: passwords".to_string(),
        };
        if panel == SidebarPanel::Git {
            self.last_git_status_poll = Instant::now() - GIT_STATUS_POLL_INTERVAL;
            self.refresh_git_sidebar_status();
        }
        self.persist_layout_snapshot();
        self.needs_render = true;
    }

    fn parse_git_branch_header(line: &str) -> (String, usize, usize) {
        let branch = line
            .strip_prefix("## ")
            .unwrap_or(line)
            .split_once("...")
            .map(|(left, _)| left.trim().to_string())
            .unwrap_or_else(|| line.strip_prefix("## ").unwrap_or(line).trim().to_string());

        let mut ahead = 0usize;
        let mut behind = 0usize;
        if let (Some(start), Some(end)) = (line.find('['), line.rfind(']')) {
            if start < end {
                let section = &line[start + 1..end];
                for token in section.split(',') {
                    let token = token.trim();
                    if let Some(value) = token.strip_prefix("ahead ") {
                        ahead = value.trim().parse::<usize>().unwrap_or(0);
                    } else if let Some(value) = token.strip_prefix("behind ") {
                        behind = value.trim().parse::<usize>().unwrap_or(0);
                    }
                }
            }
        }
        (branch, ahead, behind)
    }

    fn refresh_git_sidebar_status(&mut self) {
        let output = Command::new("git")
            .arg("status")
            .arg("--porcelain=1")
            .arg("--branch")
            .current_dir(&self.workspace_root)
            .output();
        self.last_git_status_poll = Instant::now();

        let Ok(output) = output else {
            self.git_sidebar_status = None;
            self.git_sidebar_error = Some("git command unavailable".to_string());
            return;
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if stderr.to_lowercase().contains("not a git repository") {
                "workspace is not a git repository".to_string()
            } else {
                format!("git status failed ({})", output.status)
            };
            self.git_sidebar_status = None;
            self.git_sidebar_error = Some(msg);
            return;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut branch = "-".to_string();
        let mut ahead = 0usize;
        let mut behind = 0usize;
        let mut modified = 0usize;
        let mut added = 0usize;
        let mut deleted = 0usize;
        let mut untracked = 0usize;
        let mut conflicted = 0usize;

        for line in stdout.lines() {
            if line.starts_with("## ") {
                let parsed = Self::parse_git_branch_header(line);
                branch = parsed.0;
                ahead = parsed.1;
                behind = parsed.2;
                continue;
            }

            if line.starts_with("?? ") {
                untracked += 1;
                continue;
            }

            let mut chars = line.chars();
            let x = chars.next().unwrap_or(' ');
            let y = chars.next().unwrap_or(' ');

            if matches!(x, 'A' | 'C' | 'R') || matches!(y, 'A' | 'C' | 'R') {
                added += 1;
            }
            if x == 'D' || y == 'D' {
                deleted += 1;
            }
            if matches!(x, 'U') || matches!(y, 'U') {
                conflicted += 1;
            }
            if matches!(x, 'M' | 'R' | 'C' | 'U') || matches!(y, 'M' | 'R' | 'C' | 'U') {
                modified += 1;
            }
        }

        let total = modified + added + deleted + untracked + conflicted;
        self.git_sidebar_status = Some(GitSidebarStatus {
            branch,
            ahead,
            behind,
            modified,
            added,
            deleted,
            untracked,
            conflicted,
            total,
        });
        self.git_sidebar_error = None;
    }

    fn poll_git_sidebar_status(&mut self) -> bool {
        if self.sidebar_panel != SidebarPanel::Git {
            return false;
        }
        let now = Instant::now();
        if now.duration_since(self.last_git_status_poll) < GIT_STATUS_POLL_INTERVAL {
            return false;
        }
        let before = self.git_sidebar_status.clone();
        let before_err = self.git_sidebar_error.clone();
        self.refresh_git_sidebar_status();
        let changed = before != self.git_sidebar_status || before_err != self.git_sidebar_error;
        if changed {
            self.needs_render = true;
        }
        changed
    }

    fn poll_quiron_health(&mut self) -> bool {
        let mut changed = false;

        if self
            .quiron_health_task
            .as_ref()
            .map(|task| task.is_finished())
            .unwrap_or(false)
        {
            let task = self
                .quiron_health_task
                .take()
                .expect("health task must exist when finished");
            match self.runtime.block_on(async move { task.await }) {
                Ok(snapshot) => {
                    changed = true;
                    self.quiron_last_health_ok =
                        Some(snapshot.as_ref().map(|h| h.is_ok()).unwrap_or(false));
                    self.quiron_index_health = snapshot;
                }
                Err(err) => {
                    tracing::warn!("quiron health task join error: {}", err);
                    changed = true;
                    self.quiron_last_health_ok = Some(false);
                    self.quiron_index_health = None;
                }
            }
            self.needs_render = true;
        }

        if self.quiron_health_task.is_some() {
            return changed;
        }

        let now = Instant::now();
        if now.duration_since(self.quiron_last_health_poll) < QUIRON_HEALTH_POLL_INTERVAL {
            return changed;
        }
        self.start_quiron_health_check();
        changed
    }

    /// Snapshot de problemas operacionales para panel sidebar.
    pub fn sidebar_problems_snapshot(&self) -> Vec<SidebarProblem> {
        let mut out = Vec::new();

        let unsaved_count = self
            .open_tabs
            .iter()
            .filter(|tab| tab.editor.is_modified())
            .count();
        if unsaved_count > 0 {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Warning,
                title: "Unsaved tabs".to_string(),
                detail: format!("{} tab(s) with local changes", unsaved_count),
            });
        }

        if self.pending_exit_confirm {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Warning,
                title: "Exit confirm pending".to_string(),
                detail: "close requested with unsaved tabs".to_string(),
            });
        }

        if let Some(err) = self.telemetry_persisted_last_error.as_deref() {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Warning,
                title: "Telemetry persisted".to_string(),
                detail: Self::truncate_for_report(err, 96),
            });
        }

        if let Some(err) = self.git_sidebar_error.as_deref() {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Warning,
                title: "Git status".to_string(),
                detail: Self::truncate_for_report(err, 96),
            });
        }

        if self.search_active && !self.search_query.is_empty() && self.search_matches.is_empty() {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Info,
                title: "Find no matches".to_string(),
                detail: format!(
                    "query '{}' has no matches",
                    Self::truncate_for_report(&self.search_query, 40)
                ),
            });
        }

        if self.replace_all_confirm_pending {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Info,
                title: "Replace preview pending".to_string(),
                detail: "Ctrl+Shift+R apply or Esc cancel".to_string(),
            });
        }

        if self.loading {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Info,
                title: "Request running".to_string(),
                detail: "Quiron is processing a response".to_string(),
            });
        }

        if let Some(path) = self.active_file_path() {
            if let Some(language_name) = self
                .language_registry
                .detect_language(path)
                .map(|lang| lang.name().to_string())
            {
                let source = self.active_editor().text();
                if let Some(diag) = syntax_diagnostic(Some(&language_name), &source) {
                    out.push(SidebarProblem {
                        severity: SidebarProblemSeverity::Error,
                        title: "Syntax error".to_string(),
                        detail: format!(
                            "{} L{}:{} {}",
                            language_name,
                            diag.line + 1,
                            diag.column + 1,
                            Self::truncate_for_report(&diag.message, 64)
                        ),
                    });
                }
            }
        }

        let status_lower = self.status_text.to_lowercase();
        if status_lower.contains("error") || status_lower.contains("failed") {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Error,
                title: "Latest status".to_string(),
                detail: Self::truncate_for_report(&self.status_text, 96),
            });
        }

        if out.is_empty() {
            out.push(SidebarProblem {
                severity: SidebarProblemSeverity::Info,
                title: "No issues".to_string(),
                detail: "All clear in current session".to_string(),
            });
        } else {
            out.sort_by(|a, b| {
                Self::sidebar_problem_context_rank(a)
                    .cmp(&Self::sidebar_problem_context_rank(b))
                    .then_with(|| {
                        Self::sidebar_problem_severity_rank(a.severity)
                            .cmp(&Self::sidebar_problem_severity_rank(b.severity))
                    })
                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            });
        }
        out
    }

    /// Índice del problema activo (si hubo navegación/selección previa).
    pub fn active_problem_index(&self) -> Option<usize> {
        self.active_problem_index
    }

    fn sidebar_problem_severity_rank(severity: SidebarProblemSeverity) -> usize {
        match severity {
            SidebarProblemSeverity::Error => 0,
            SidebarProblemSeverity::Warning => 1,
            SidebarProblemSeverity::Info => 2,
        }
    }

    /// Contexto operativo del problema para agrupación y filtros.
    pub fn sidebar_problem_context_label(problem: &SidebarProblem) -> &'static str {
        match problem.title.as_str() {
            "Unsaved tabs" | "Exit confirm pending" | "Syntax error" => "editor",
            "Find no matches" | "Replace preview pending" => "search",
            "Git status" => "git",
            "Telemetry persisted" => "telemetry",
            "Request running" | "Latest status" => "runtime",
            "No issues" => "session",
            _ => "session",
        }
    }

    fn sidebar_problem_context_rank(problem: &SidebarProblem) -> usize {
        match Self::sidebar_problem_context_label(problem) {
            "editor" => 0,
            "search" => 1,
            "git" => 2,
            "telemetry" => 3,
            "runtime" => 4,
            _ => 5,
        }
    }

    fn sidebar_problem_severity_label(severity: SidebarProblemSeverity) -> &'static str {
        match severity {
            SidebarProblemSeverity::Error => "error",
            SidebarProblemSeverity::Warning => "warn",
            SidebarProblemSeverity::Info => "info",
        }
    }

    fn apply_problem_navigation(&mut self, problem: &SidebarProblem) {
        match problem.title.as_str() {
            "Unsaved tabs" => {
                if let Some((index, _)) = self
                    .open_tabs
                    .iter()
                    .enumerate()
                    .find(|(_, tab)| tab.editor.is_modified())
                {
                    self.set_tab_for_pane(EditorPane::Primary, index);
                    self.set_focus(FocusTarget::EditorPrimary);
                    let _ = self.reveal_active_file_in_explorer();
                    self.refresh_search_matches_if_active();
                } else {
                    self.set_sidebar_panel(SidebarPanel::Problems);
                }
            }
            "Telemetry persisted" => {
                if !self.telemetry_panel_enabled {
                    self.toggle_telemetry_panel();
                }
                self.refresh_telemetry_persisted_cache();
                self.set_sidebar_panel(SidebarPanel::Problems);
            }
            "Git status" => {
                self.set_sidebar_panel(SidebarPanel::Git);
            }
            "Find no matches" => {
                self.set_sidebar_panel(SidebarPanel::Search);
                self.begin_search_mode();
            }
            "Replace preview pending" => {
                self.set_focus(match self.focused_editor_pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
            }
            "Request running" => {
                self.set_focus(FocusTarget::ChatInput);
            }
            "Syntax error" => {
                self.set_focus(match self.focused_editor_pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
            }
            _ => {
                self.set_sidebar_panel(SidebarPanel::Problems);
            }
        }
    }

    fn rebuild_problems_overlay_items(&mut self) {
        let problems = self.sidebar_problems_snapshot();
        let query = self.overlay_query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_string())
            .collect();

        let mut scored: Vec<(i32, usize, OverlayItem)> = Vec::new();
        for (index, problem) in problems.iter().enumerate() {
            let sev = Self::sidebar_problem_severity_label(problem.severity);
            let ctx = Self::sidebar_problem_context_label(problem);
            let haystack = format!(
                "{} {} {} {}",
                problem.title.to_lowercase(),
                problem.detail.to_lowercase(),
                sev,
                ctx
            );

            let mut score = 0i32;
            let mut matched = true;
            if tokens.is_empty() {
                score = 10_000 - index as i32;
            } else {
                for token in &tokens {
                    if let Some(pos) = haystack.find(token) {
                        score += 900 - pos.min(200) as i32;
                    } else if let Some(fuzzy) =
                        Self::fuzzy_subsequence_score(&problem.title.to_lowercase(), token)
                    {
                        score += fuzzy / 2;
                    } else {
                        matched = false;
                        break;
                    }
                }
            }

            if !matched {
                continue;
            }

            scored.push((
                score,
                index,
                OverlayItem {
                    title: problem.title.clone(),
                    detail: format!("[{}][{}] {}", ctx, sev, problem.detail),
                    action: OverlayAction::ProblemSelect {
                        title: problem.title.clone(),
                        index,
                    },
                },
            ));
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        self.overlay_items = scored
            .into_iter()
            .take(OVERLAY_RESULTS_MAX)
            .map(|(_, _, item)| item)
            .collect();
        self.overlay_selected = self
            .overlay_selected
            .min(self.overlay_items.len().saturating_sub(1));
        self.needs_render = true;
    }

    fn activate_problem_index(&mut self, index: usize) -> bool {
        let problems = self.sidebar_problems_snapshot();
        if index >= problems.len() {
            return false;
        }
        let len = problems.len();
        self.active_problem_index = Some(index);
        let selected = problems[index].clone();
        self.apply_problem_navigation(&selected);
        self.status_text = format!("problem {}/{}: {}", index + 1, len, selected.title);
        self.needs_render = true;
        true
    }

    /// Navega y aplica el siguiente/anterior problema operativo.
    pub fn navigate_problem(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let problems = self.sidebar_problems_snapshot();
        if problems.is_empty() {
            return false;
        }
        let len = problems.len();
        let next = match self.active_problem_index {
            Some(current) => (current as i32 + delta).rem_euclid(len as i32) as usize,
            None => {
                if delta > 0 {
                    0
                } else {
                    len.saturating_sub(1)
                }
            }
        };
        self.activate_problem_index(next)
    }

    fn parse_outline_symbol_name(line: &str, prefix: &str, stop_chars: &[char]) -> Option<String> {
        let after = line.trim_start().strip_prefix(prefix)?.trim_start();
        let mut out = String::new();
        for ch in after.chars() {
            if stop_chars.contains(&ch) || ch.is_whitespace() {
                break;
            }
            out.push(ch);
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn outline_symbols_from_text(text: &str) -> Vec<SidebarOutlineItem> {
        let mut out = Vec::new();
        for (line_idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let indent = line.len().saturating_sub(trimmed.len());
            let column = indent;

            if trimmed.starts_with("#") {
                let level = trimmed
                    .chars()
                    .take_while(|c| *c == '#')
                    .count()
                    .clamp(1, 6);
                let label = trimmed[level..].trim();
                if !label.is_empty() {
                    out.push(SidebarOutlineItem {
                        line: line_idx,
                        column,
                        kind: format!("H{}", level),
                        label: label.to_string(),
                    });
                    continue;
                }
            }

            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "pub fn ", &['(', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "fn".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "fn ", &['(', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "fn".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) =
                Self::parse_outline_symbol_name(trimmed, "pub struct ", &['{', '<', '('])
            {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "struct".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) =
                Self::parse_outline_symbol_name(trimmed, "struct ", &['{', '<', '('])
            {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "struct".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "pub enum ", &['{', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "enum".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "enum ", &['{', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "enum".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "pub trait ", &['{', '<'])
            {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "trait".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "trait ", &['{', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "trait".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "impl ", &[' ', '<', '{'])
            {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "impl".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "mod ", &[';', '{']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "mod".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "class ", &['{', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "class".to_string(),
                    label: name,
                });
                continue;
            }
            if let Some(name) = Self::parse_outline_symbol_name(trimmed, "function ", &['(', '<']) {
                out.push(SidebarOutlineItem {
                    line: line_idx,
                    column,
                    kind: "fn".to_string(),
                    label: name,
                });
            }
        }
        out
    }

    /// Snapshot de símbolos del editor activo/focal para panel Outline.
    pub fn sidebar_outline_snapshot(&self, pane: EditorPane) -> Vec<SidebarOutlineItem> {
        let text = self.editor_for_pane(pane).text();
        Self::outline_symbols_from_text(&text)
    }

    fn invalidate_workspace_cache(&mut self) {
        self.quick_open_cache_stale = true;
        self.workspace_symbol_cache_stale = true;
    }

    fn mark_workspace_mutated(&mut self) {
        self.invalidate_workspace_cache();
        self.workspace_signature = self.compute_workspace_signature();
        self.last_workspace_poll = Instant::now();
    }

    fn poll_workspace_changes(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_workspace_poll) < WORKSPACE_POLL_INTERVAL {
            return false;
        }
        self.last_workspace_poll = now;

        let signature = self.compute_workspace_signature();
        if signature == self.workspace_signature {
            return false;
        }

        self.workspace_signature = signature;
        self.invalidate_workspace_cache();
        self.refresh_explorer();
        if self.sidebar_panel == SidebarPanel::Git {
            self.last_git_status_poll = Instant::now() - GIT_STATUS_POLL_INTERVAL;
            self.refresh_git_sidebar_status();
        }
        if self.overlay_mode == Some(OverlayMode::QuickOpen) {
            self.quick_open_candidates = self.collect_workspace_files();
            self.quick_open_cache_stale = false;
            self.rebuild_quick_open_items();
        } else if self.overlay_mode == Some(OverlayMode::WorkspaceSymbols) {
            self.quick_open_candidates = self.collect_workspace_files();
            self.quick_open_cache_stale = false;
            self.workspace_symbol_candidates = self.collect_workspace_symbol_candidates();
            self.workspace_symbol_cache_stale = false;
            self.rebuild_workspace_symbol_overlay_items();
        } else if self.overlay_mode == Some(OverlayMode::WorkspaceTextSearch) {
            self.quick_open_candidates = self.collect_workspace_files();
            self.quick_open_cache_stale = false;
            self.rebuild_workspace_text_search_overlay_items();
        }
        self.status_text = "workspace changed: explorer updated".to_string();
        self.needs_render = true;
        true
    }

    fn reset_telemetry_prefetch_state(&mut self) {
        self.telemetry_persisted_prefetch_task = None;
        self.telemetry_persisted_prefetch_session = None;
        self.telemetry_persisted_prefetch_last_at = Instant::now();
    }

    fn poll_telemetry_persisted_prefetch(&mut self) -> bool {
        let mut changed = false;

        if self
            .telemetry_persisted_prefetch_task
            .as_ref()
            .map(|task| task.is_finished())
            .unwrap_or(false)
        {
            let task = self
                .telemetry_persisted_prefetch_task
                .take()
                .expect("prefetch task must exist when finished");
            let expected_session = self.telemetry_persisted_prefetch_session.take();
            let finished = self.runtime.block_on(async move { task.await });
            match finished {
                Ok(Ok(page)) => {
                    if expected_session.as_deref() == Some(page.session_id.as_str()) {
                        let merged = if let Some(current) = self.telemetry_persisted_cache.take() {
                            if current.session_id == page.session_id {
                                Self::merge_persisted_cache_pages(Some(current), page)
                            } else {
                                page
                            }
                        } else {
                            page
                        };
                        self.telemetry_persisted_cache = Some(merged);
                        self.telemetry_persisted_last_error = None;
                        self.needs_render = true;
                        changed = true;
                    }
                }
                Ok(Err(err)) => {
                    self.telemetry_persisted_last_error = Some(err);
                }
                Err(err) => {
                    self.telemetry_persisted_last_error =
                        Some(format!("prefetch join error: {}", err));
                }
            }
        }

        if self.telemetry_persisted_prefetch_task.is_some() || !self.telemetry_panel_enabled {
            return changed;
        }

        let now = Instant::now();
        if now.duration_since(self.telemetry_persisted_prefetch_last_at)
            < TELEMETRY_PERSISTED_PREFETCH_INTERVAL
        {
            return changed;
        }
        self.telemetry_persisted_prefetch_last_at = now;

        let quiron = self.quiron.clone();
        let local = self.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.session_telemetry_snapshot()
        });
        let (cp_offset, an_offset) = match self.telemetry_persisted_cache.as_ref() {
            Some(cache) if cache.session_id == local.session_id => {
                if !cache.has_more_checkpoints && !cache.has_more_anomalies {
                    return changed;
                }
                (cache.checkpoints.len(), cache.anomalies.len())
            }
            _ => (0, 0),
        };

        let quiron = self.quiron.clone();
        let session_id = local.session_id.clone();
        let session_for_req = session_id.clone();
        let task = self.runtime.spawn(async move {
            let q = quiron.lock().await;
            q.get_persisted_session_telemetry_page(
                Some(&session_for_req),
                RUNTIME_TELEMETRY_FETCH_LIMIT,
                cp_offset,
                an_offset,
            )
            .await
            .map_err(|e| e.to_string())
        });
        self.telemetry_persisted_prefetch_session = Some(session_id);
        self.telemetry_persisted_prefetch_task = Some(task);

        changed
    }

    fn compute_workspace_signature(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let mut scanned = 0usize;
        Self::hash_workspace_recursive(
            &self.workspace_root,
            &self.workspace_root,
            0,
            WORKSPACE_SIGNATURE_MAX_DEPTH,
            WORKSPACE_SIGNATURE_MAX_ENTRIES,
            &mut scanned,
            &mut hasher,
        );
        hasher.finish()
    }

    fn hash_workspace_recursive(
        root: &Path,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        max_entries: usize,
        scanned: &mut usize,
        hasher: &mut DefaultHasher,
    ) {
        if depth > max_depth || *scanned >= max_entries {
            return;
        }

        if !matches!(workspace_guard::classify(root, dir), Access::Allowed) {
            return;
        }

        let read_dir = match fs::read_dir(dir) {
            Ok(v) => v,
            Err(_) => return,
        };

        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for entry in read_dir.flatten() {
            if *scanned >= max_entries {
                break;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            let path = entry.path();
            if !matches!(workspace_guard::classify(root, &path), Access::Allowed) {
                continue;
            }
            if file_type.is_dir() {
                if Self::is_ignored_workspace_dir(&name) {
                    continue;
                }
                dirs.push((name, path));
            } else if file_type.is_file() {
                files.push((name, path));
            }
        }

        dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        for (_, file_path) in files {
            if *scanned >= max_entries {
                return;
            }
            *scanned += 1;
            let rel = file_path
                .strip_prefix(root)
                .unwrap_or(&file_path)
                .to_string_lossy();
            rel.hash(hasher);
            if let Ok(metadata) = fs::metadata(&file_path) {
                metadata.len().hash(hasher);
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        duration.as_secs().hash(hasher);
                        duration.subsec_nanos().hash(hasher);
                    }
                }
            }
        }

        for (name, dir_path) in dirs {
            if *scanned >= max_entries {
                return;
            }
            name.hash(hasher);
            *scanned += 1;
            Self::hash_workspace_recursive(
                root,
                &dir_path,
                depth + 1,
                max_depth,
                max_entries,
                scanned,
                hasher,
            );
        }
    }

    fn overlay_title(&self) -> &'static str {
        match self.overlay_mode {
            Some(OverlayMode::QuickOpen) => "Quick Open",
            Some(OverlayMode::CommandPalette) => "Command Palette",
            Some(OverlayMode::GoToLine) => "Go to Line",
            Some(OverlayMode::Symbols) => "Go to Symbol",
            Some(OverlayMode::WorkspaceSymbols) => "Go to Workspace Symbol",
            Some(OverlayMode::WorkspaceTextSearch) => "Find in Workspace",
            Some(OverlayMode::Problems) => "Problems",
            None => "",
        }
    }

    fn compact_line_preview(text: &str, max_chars: usize) -> String {
        let normalized = text.trim().replace('\t', " ");
        let mut preview: String = normalized.chars().take(max_chars).collect();
        if normalized.chars().count() > max_chars {
            preview.push_str("...");
        }
        preview
    }

    fn parse_go_to_line_input(input: &str) -> Option<(usize, usize)> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let (line_part, column_part) = if let Some((line, col)) = trimmed.split_once(':') {
            (line.trim(), Some(col.trim()))
        } else if let Some((line, col)) = trimmed.split_once(',') {
            (line.trim(), Some(col.trim()))
        } else {
            (trimmed, None)
        };

        let line_number = line_part.parse::<usize>().ok()?;
        if line_number == 0 {
            return None;
        }
        let column_number = column_part
            .and_then(|raw| {
                if raw.is_empty() {
                    None
                } else {
                    raw.parse::<usize>().ok()
                }
            })
            .unwrap_or(1);
        if column_number == 0 {
            return None;
        }
        Some((line_number - 1, column_number - 1))
    }

    fn collect_workspace_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if !self.workspace_is_open() {
            return files;
        }
        Self::collect_workspace_files_recursive(
            &self.workspace_root,
            &self.workspace_root,
            0,
            QUICK_OPEN_MAX_DEPTH,
            QUICK_OPEN_MAX_FILES,
            &mut files,
        );

        files.sort_by(|a, b| {
            self.workspace_relative_label(a)
                .cmp(&self.workspace_relative_label(b))
        });
        files
    }

    fn collect_workspace_symbol_candidates(&self) -> Vec<WorkspaceSymbolCandidate> {
        let mut out = Vec::new();
        for path in self
            .quick_open_candidates
            .iter()
            .take(WORKSPACE_SYMBOL_MAX_FILES)
        {
            if out.len() >= WORKSPACE_SYMBOL_MAX_ENTRIES {
                break;
            }

            let Ok(metadata) = fs::metadata(path) else {
                continue;
            };
            if metadata.len() > WORKSPACE_SYMBOL_MAX_FILE_BYTES {
                continue;
            }

            let Some(content) = self.read_project_file(path) else {
                continue;
            };

            let symbols = Self::outline_symbols_from_text(&content);
            for symbol in symbols {
                if out.len() >= WORKSPACE_SYMBOL_MAX_ENTRIES {
                    break;
                }
                out.push(WorkspaceSymbolCandidate {
                    path: path.clone(),
                    line: symbol.line,
                    column: symbol.column,
                    kind: symbol.kind,
                    label: symbol.label,
                });
            }
        }
        out
    }

    /// Acumula los archivos indexables del proyecto para búsqueda y quick open.
    ///
    /// Solo recoge lo que la guardia marca como permitido: ni secretos, ni
    /// artefactos generados, ni nada fuera de `root`.
    fn collect_workspace_files_recursive(
        root: &Path,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        max_files: usize,
        out: &mut Vec<PathBuf>,
    ) {
        if depth > max_depth || out.len() >= max_files {
            return;
        }

        if !matches!(workspace_guard::classify(root, dir), Access::Allowed) {
            return;
        }

        let read_dir = match fs::read_dir(dir) {
            Ok(v) => v,
            Err(_) => return,
        };

        let mut dirs: Vec<(String, PathBuf, bool)> = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();

        for entry in read_dir.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            let path = entry.path();
            if !matches!(workspace_guard::classify(root, &path), Access::Allowed) {
                continue;
            }
            if file_type.is_dir() {
                if Self::is_ignored_workspace_dir(&name) {
                    continue;
                }
                dirs.push((name, path, file_type.is_symlink()));
            } else if file_type.is_file() {
                files.push((name, path));
            }
        }

        dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        for (_, path, is_symlink) in dirs {
            if out.len() >= max_files {
                return;
            }
            if is_symlink {
                continue;
            }
            Self::collect_workspace_files_recursive(root, &path, depth + 1, max_depth, max_files, out);
        }

        for (_, path) in files {
            if out.len() >= max_files {
                return;
            }
            out.push(path);
        }
    }

    fn is_ignored_workspace_dir(name: &str) -> bool {
        matches!(
            name,
            ".git" | "target" | "node_modules" | ".idea" | ".vscode" | ".next" | ".cache"
        )
    }

    fn workspace_relative_label(&self, path: &Path) -> String {
        path.strip_prefix(&self.workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn fuzzy_subsequence_score(candidate: &str, query: &str) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }

        let mut search_from = 0usize;
        let mut score = 0i32;
        let mut prev_idx: Option<usize> = None;

        for query_ch in query.chars() {
            let mut found = None;
            for (offset, candidate_ch) in candidate[search_from..].char_indices() {
                if candidate_ch == query_ch {
                    found = Some(search_from + offset);
                    break;
                }
            }
            let idx = found?;

            score += 16;
            if idx == 0 {
                score += 24;
            }
            if let Some(prev) = prev_idx {
                if idx == prev + 1 {
                    score += 28;
                } else {
                    score -= (idx.saturating_sub(prev + 1)).min(8) as i32;
                }
            } else {
                score += (50i32 - idx.min(50) as i32).max(0);
            }

            prev_idx = Some(idx);
            search_from = idx.saturating_add(1);
        }

        Some(score)
    }

    fn quick_open_score(
        relative_lower: &str,
        file_name_lower: &str,
        tokens: &[String],
    ) -> Option<i32> {
        if tokens.is_empty() {
            return Some(0);
        }

        let mut score = 0i32;
        for token in tokens {
            if token.is_empty() {
                continue;
            }

            if let Some(pos) = relative_lower.find(token) {
                score += 2200 - pos.min(400) as i32;
                if file_name_lower.starts_with(token) {
                    score += 620;
                } else if file_name_lower.contains(token) {
                    score += 360;
                }
                score += token.len() as i32 * 18;
                continue;
            }

            if tokens.len() == 1 {
                if let Some(subseq) = Self::fuzzy_subsequence_score(file_name_lower, token)
                    .or_else(|| Self::fuzzy_subsequence_score(relative_lower, token))
                {
                    score += subseq;
                    continue;
                }
            }

            return None;
        }

        Some(score)
    }

    fn rebuild_quick_open_items(&mut self) {
        let query = self.overlay_query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_string())
            .collect();

        let mut scored: Vec<(i32, OverlayItem)> = Vec::new();
        for path in &self.quick_open_candidates {
            let relative = self.workspace_relative_label(path);
            let relative_lower = relative.to_lowercase();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| relative.clone());
            let file_name_lower = file_name.to_lowercase();
            let Some(score) = Self::quick_open_score(&relative_lower, &file_name_lower, &tokens)
            else {
                continue;
            };
            scored.push((
                score,
                OverlayItem {
                    title: file_name,
                    detail: relative,
                    action: OverlayAction::OpenFile(path.clone()),
                },
            ));
        }

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.detail.to_lowercase().cmp(&b.1.detail.to_lowercase()))
        });

        self.overlay_items = scored
            .into_iter()
            .take(OVERLAY_RESULTS_MAX)
            .map(|(_, item)| item)
            .collect();
        self.overlay_selected = self
            .overlay_selected
            .min(self.overlay_items.len().saturating_sub(1));
        self.needs_render = true;
    }

    fn rebuild_command_palette_items(&mut self) {
        let query = self.overlay_query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_string())
            .collect();

        let mut scored: Vec<(i32, OverlayItem)> = Vec::new();
        for descriptor in COMMAND_DESCRIPTORS {
            let haystack = format!(
                "{} {} {}",
                descriptor.label.to_lowercase(),
                descriptor.detail.to_lowercase(),
                descriptor.keywords.to_lowercase()
            );

            let mut score = 0i32;
            let mut matched = true;
            for token in &tokens {
                if let Some(pos) = haystack.find(token) {
                    score += 900 - pos.min(200) as i32;
                } else if let Some(fuzzy) = Self::fuzzy_subsequence_score(&haystack, token) {
                    score += fuzzy / 2;
                } else {
                    matched = false;
                    break;
                }
            }
            if !matched {
                continue;
            }

            scored.push((
                score,
                OverlayItem {
                    title: descriptor.label.to_string(),
                    detail: descriptor.detail.to_string(),
                    action: OverlayAction::Command(descriptor.action),
                },
            ));
        }

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.title.to_lowercase().cmp(&b.1.title.to_lowercase()))
        });

        self.overlay_items = scored
            .into_iter()
            .take(OVERLAY_RESULTS_MAX)
            .map(|(_, item)| item)
            .collect();
        self.overlay_selected = self
            .overlay_selected
            .min(self.overlay_items.len().saturating_sub(1));
        self.needs_render = true;
    }

    fn rebuild_go_to_line_overlay_items(&mut self) {
        let pane = self.focused_editor_pane;
        let editor = self.editor_for_pane(pane);
        let text = editor.text();
        let lines: Vec<&str> = text.split('\n').collect();
        let total_lines = lines.len().max(1);
        let cursor = editor.cursor();

        let requested = if self.overlay_query.trim().is_empty() {
            Some((cursor.line, cursor.column))
        } else {
            Self::parse_go_to_line_input(&self.overlay_query)
        };

        let Some((line, column)) = requested else {
            self.overlay_items.clear();
            self.overlay_selected = 0;
            self.needs_render = true;
            return;
        };

        let clamped_line = line.min(total_lines.saturating_sub(1));
        let line_text = lines.get(clamped_line).copied().unwrap_or("");
        let clamped_col = column.min(line_text.chars().count());

        self.overlay_items = vec![OverlayItem {
            title: format!("Line {}, Column {}", clamped_line + 1, clamped_col + 1),
            detail: format!("jump in active editor ({:?})", pane),
            action: OverlayAction::JumpToLine {
                pane,
                line: clamped_line,
                column: clamped_col,
            },
        }];
        self.overlay_selected = 0;
        self.needs_render = true;
    }

    fn rebuild_symbol_overlay_items(&mut self) {
        let pane = self.focused_editor_pane;
        let symbols = self.sidebar_outline_snapshot(pane);
        let query = self.overlay_query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_string())
            .collect();

        let mut scored: Vec<(i32, usize, OverlayItem)> = Vec::new();
        for symbol in symbols {
            let label_lower = symbol.label.to_lowercase();
            let kind_lower = symbol.kind.to_lowercase();
            let line_label = (symbol.line + 1).to_string();
            let haystack = format!("{} {} {}", label_lower, kind_lower, line_label);

            let mut score = 0i32;
            let mut matched = true;
            if tokens.is_empty() {
                score = 10_000i32.saturating_sub(symbol.line as i32);
            } else {
                for token in &tokens {
                    if let Some(pos) = haystack.find(token) {
                        score += 1000 - pos.min(240) as i32;
                    } else if let Some(fuzzy) = Self::fuzzy_subsequence_score(&label_lower, token) {
                        score += fuzzy / 2;
                    } else {
                        matched = false;
                        break;
                    }
                }
                if matched {
                    score -= (symbol.line.min(5000) as i32) / 4;
                }
            }
            if !matched {
                continue;
            }

            scored.push((
                score,
                symbol.line,
                OverlayItem {
                    title: symbol.label.clone(),
                    detail: format!("{}  •  L{}", symbol.kind, symbol.line + 1),
                    action: OverlayAction::JumpToSymbol {
                        pane,
                        line: symbol.line,
                        column: symbol.column,
                    },
                },
            ));
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        self.overlay_items = scored
            .into_iter()
            .take(OVERLAY_RESULTS_MAX)
            .map(|(_, _, item)| item)
            .collect();
        self.overlay_selected = self
            .overlay_selected
            .min(self.overlay_items.len().saturating_sub(1));
        self.needs_render = true;
    }

    fn rebuild_workspace_symbol_overlay_items(&mut self) {
        let query = self.overlay_query.trim().to_lowercase();
        let tokens: Vec<String> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_string())
            .collect();

        let mut scored: Vec<(i32, String, usize, OverlayItem)> = Vec::new();
        for symbol in &self.workspace_symbol_candidates {
            let relative = self.workspace_relative_label(&symbol.path);
            let relative_lower = relative.to_lowercase();
            let label_lower = symbol.label.to_lowercase();
            let kind_lower = symbol.kind.to_lowercase();
            let line_label = (symbol.line + 1).to_string();
            let haystack = format!(
                "{} {} {} {}",
                label_lower, kind_lower, relative_lower, line_label
            );

            let mut score = 0i32;
            let mut matched = true;
            if tokens.is_empty() {
                score = 12_000i32.saturating_sub(symbol.line as i32);
            } else {
                for token in &tokens {
                    if let Some(pos) = haystack.find(token) {
                        score += 1100 - pos.min(300) as i32;
                    } else if let Some(fuzzy) = Self::fuzzy_subsequence_score(&label_lower, token) {
                        score += fuzzy / 2;
                    } else if let Some(fuzzy) =
                        Self::fuzzy_subsequence_score(&relative_lower, token)
                    {
                        score += fuzzy / 3;
                    } else {
                        matched = false;
                        break;
                    }
                }
                if matched {
                    score -= (symbol.line.min(5000) as i32) / 6;
                }
            }
            if !matched {
                continue;
            }

            scored.push((
                score,
                relative.clone(),
                symbol.line,
                OverlayItem {
                    title: symbol.label.clone(),
                    detail: format!("{}  •  {}  •  L{}", symbol.kind, relative, symbol.line + 1),
                    action: OverlayAction::OpenFileAtLocation {
                        path: symbol.path.clone(),
                        line: symbol.line,
                        column: symbol.column,
                    },
                },
            ));
        }

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
                .then_with(|| a.2.cmp(&b.2))
        });

        self.overlay_items = scored
            .into_iter()
            .take(OVERLAY_RESULTS_MAX)
            .map(|(_, _, _, item)| item)
            .collect();
        self.overlay_selected = self
            .overlay_selected
            .min(self.overlay_items.len().saturating_sub(1));
        self.needs_render = true;
    }

    fn rebuild_workspace_text_search_overlay_items(&mut self) {
        let query = self.overlay_query.trim();
        if query.is_empty() {
            self.overlay_items.clear();
            self.overlay_selected = 0;
            self.needs_render = true;
            return;
        }

        let query_lower = query.to_lowercase();
        let tokens: Vec<String> = query_lower
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_string())
            .collect();
        if tokens.is_empty() {
            self.overlay_items.clear();
            self.overlay_selected = 0;
            self.needs_render = true;
            return;
        }

        let mut scored: Vec<(i32, String, usize, OverlayItem)> = Vec::new();
        for path in self
            .quick_open_candidates
            .iter()
            .take(WORKSPACE_TEXT_SEARCH_MAX_FILES)
        {
            let Ok(metadata) = fs::metadata(path) else {
                continue;
            };
            if metadata.len() > WORKSPACE_TEXT_SEARCH_MAX_FILE_BYTES {
                continue;
            }

            let Some(content) = self.read_project_file(path) else {
                continue;
            };
            if content.is_empty() {
                continue;
            }

            let relative = self.workspace_relative_label(path);
            let relative_lower = relative.to_lowercase();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| relative.clone());
            let file_name_lower = file_name.to_lowercase();

            for (line_idx, line) in content.lines().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let line_lower = line.to_lowercase();
                let mut score = 0i32;
                let mut first_match_col_bytes: Option<usize> = None;
                let mut matched = true;
                for token in &tokens {
                    if let Some(pos) = line_lower.find(token) {
                        if first_match_col_bytes.is_none() {
                            first_match_col_bytes = Some(pos);
                        }
                        score += 1400 - pos.min(260) as i32;
                        score += token.len() as i32 * 8;
                    } else if let Some(pos) = file_name_lower.find(token) {
                        score += 560 - pos.min(180) as i32;
                    } else if let Some(pos) = relative_lower.find(token) {
                        score += 320 - pos.min(220) as i32;
                    } else {
                        matched = false;
                        break;
                    }
                }
                if !matched {
                    continue;
                }

                let column = first_match_col_bytes
                    .map(|pos| line[..pos].chars().count())
                    .unwrap_or(0);
                let preview = Self::compact_line_preview(line, WORKSPACE_TEXT_PREVIEW_MAX_CHARS);
                score += 220 - line_idx.min(220) as i32;

                scored.push((
                    score,
                    relative.clone(),
                    line_idx,
                    OverlayItem {
                        title: file_name.clone(),
                        detail: format!(
                            "{}  •  L{}:{}  •  {}",
                            relative,
                            line_idx + 1,
                            column + 1,
                            preview
                        ),
                        action: OverlayAction::OpenFileAtLocation {
                            path: path.clone(),
                            line: line_idx,
                            column,
                        },
                    },
                ));
            }
        }

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
                .then_with(|| a.2.cmp(&b.2))
        });
        if scored.len() > WORKSPACE_TEXT_SEARCH_MAX_RESULTS {
            scored.truncate(WORKSPACE_TEXT_SEARCH_MAX_RESULTS);
        }

        self.overlay_items = scored
            .into_iter()
            .take(OVERLAY_RESULTS_MAX)
            .map(|(_, _, _, item)| item)
            .collect();
        self.overlay_selected = self
            .overlay_selected
            .min(self.overlay_items.len().saturating_sub(1));
        self.needs_render = true;
    }

    fn rebuild_overlay_items_for_active_mode(&mut self) {
        match self.overlay_mode {
            Some(OverlayMode::QuickOpen) => self.rebuild_quick_open_items(),
            Some(OverlayMode::CommandPalette) => self.rebuild_command_palette_items(),
            Some(OverlayMode::GoToLine) => self.rebuild_go_to_line_overlay_items(),
            Some(OverlayMode::Symbols) => self.rebuild_symbol_overlay_items(),
            Some(OverlayMode::WorkspaceSymbols) => self.rebuild_workspace_symbol_overlay_items(),
            Some(OverlayMode::WorkspaceTextSearch) => {
                self.rebuild_workspace_text_search_overlay_items()
            }
            Some(OverlayMode::Problems) => self.rebuild_problems_overlay_items(),
            None => {}
        }
    }

    fn move_overlay_selection(&mut self, delta: i32) {
        if self.overlay_items.is_empty() {
            return;
        }

        let len = self.overlay_items.len() as i32;
        let next = (self.overlay_selected as i32 + delta).rem_euclid(len);
        self.overlay_selected = next as usize;
        self.status_text = format!(
            "{} ({}/{})",
            self.overlay_title(),
            self.overlay_selected + 1,
            self.overlay_items.len()
        );
        self.needs_render = true;
    }

    fn move_overlay_selection_page(&mut self, delta_pages: i32) {
        if self.overlay_items.is_empty() || delta_pages == 0 {
            return;
        }
        let step = (OVERLAY_RESULTS_MAX as i32 / 2).max(1);
        self.move_overlay_selection(step * delta_pages);
    }

    fn apply_runtime_token_budget(&mut self, token_budget: u32) {
        let quiron = self.quiron.clone();
        let metrics = self.runtime.block_on(async move {
            let mut q = quiron.lock().await;
            q.set_runtime_token_budget(token_budget);
            q.delegation_metrics()
        });
        self.status_text = format!(
            "runtime token budget={} | {}",
            metrics.token_budget,
            Self::format_delegation_metrics_tag(&metrics)
        );
        self.needs_render = true;
    }

    fn apply_runtime_parallel_cap(&mut self, max_parallel_subtasks: usize) {
        let quiron = self.quiron.clone();
        let metrics = self.runtime.block_on(async move {
            let mut q = quiron.lock().await;
            q.set_runtime_parallel_cap(max_parallel_subtasks);
            q.delegation_metrics()
        });
        self.status_text = format!(
            "runtime parallel cap={} | {}",
            metrics.parallel_subtasks_cap,
            Self::format_delegation_metrics_tag(&metrics)
        );
        self.needs_render = true;
    }

    fn apply_runtime_threshold_scales(&mut self, worker_scale: f32, primary_scale: f32) {
        let quiron = self.quiron.clone();
        let metrics = self.runtime.block_on(async move {
            let mut q = quiron.lock().await;
            q.set_runtime_threshold_scales(worker_scale, primary_scale);
            q.delegation_metrics()
        });
        self.status_text = format!(
            "runtime thresholds w={:.2} p={:.2} | {}",
            metrics.worker_threshold_scale,
            metrics.primary_threshold_scale,
            Self::format_delegation_metrics_tag(&metrics)
        );
        self.needs_render = true;
    }

    /// Si el panel visual de telemetría está activo.
    pub fn is_telemetry_panel_enabled(&self) -> bool {
        self.telemetry_panel_enabled
    }

    /// Filtro activo del timeline de telemetría visual.
    pub fn telemetry_timeline_filter(&self) -> TelemetryTimelineFilter {
        self.telemetry_timeline_filter
    }

    /// Scroll actual del timeline de telemetría.
    pub fn telemetry_timeline_scroll(&self) -> usize {
        self.telemetry_timeline_scroll
    }

    /// Alterna visibilidad del panel de telemetría en chat.
    pub fn toggle_telemetry_panel(&mut self) {
        self.telemetry_panel_enabled = !self.telemetry_panel_enabled;
        if self.telemetry_panel_enabled {
            self.telemetry_persisted_prefetch_last_at =
                Instant::now() - TELEMETRY_PERSISTED_PREFETCH_INTERVAL;
        } else {
            self.reset_telemetry_prefetch_state();
        }
        self.status_text = if self.telemetry_panel_enabled {
            "telemetry panel enabled".to_string()
        } else {
            "telemetry panel hidden".to_string()
        };
        self.needs_render = true;
    }

    /// Rota filtro de timeline: all -> checkpoints -> anomalies -> all.
    pub fn cycle_telemetry_timeline_filter(&mut self) {
        self.telemetry_timeline_filter = self.telemetry_timeline_filter.next();
        self.telemetry_timeline_scroll = 0;
        let filter = match self.telemetry_timeline_filter {
            TelemetryTimelineFilter::All => "all",
            TelemetryTimelineFilter::Checkpoints => "checkpoints",
            TelemetryTimelineFilter::Anomalies => "anomalies",
        };
        self.status_text = format!("telemetry timeline filter={}", filter);
        self.needs_render = true;
    }

    /// Ajusta scroll del timeline de telemetría.
    pub fn scroll_telemetry_timeline_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        let next = if delta_lines > 0 {
            self.telemetry_timeline_scroll
                .saturating_add(delta_lines as usize)
        } else {
            self.telemetry_timeline_scroll
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.telemetry_timeline_scroll = next;
        self.needs_render = true;
    }

    /// Lee telemetría persistida de la sesión activa y actualiza cache local.
    fn fetch_session_telemetry_with_persisted(
        &mut self,
    ) -> (
        SessionTelemetrySnapshot,
        Result<SessionTelemetryResponse, String>,
    ) {
        let quiron = self.quiron.clone();
        let (local, persisted_result) = self.runtime.block_on(async move {
            let q = quiron.lock().await;
            let local = q.session_telemetry_snapshot();
            let persisted = q
                .get_persisted_session_telemetry_page(
                    Some(&local.session_id),
                    RUNTIME_TELEMETRY_FETCH_LIMIT,
                    0,
                    0,
                )
                .await;
            (local, persisted)
        });

        match persisted_result {
            Ok(persisted) => {
                let merged = if let Some(current) = self.telemetry_persisted_cache.take() {
                    if current.session_id == persisted.session_id {
                        Self::merge_persisted_cache_pages(Some(current), persisted.clone())
                    } else {
                        persisted.clone()
                    }
                } else {
                    persisted.clone()
                };
                self.telemetry_persisted_cache = Some(merged);
                self.telemetry_persisted_last_error = None;
                (local, Ok(persisted))
            }
            Err(err) => {
                let err_msg = err.to_string();
                self.telemetry_persisted_last_error = Some(err_msg.clone());
                (local, Err(err_msg))
            }
        }
    }

    /// Sincroniza explícitamente la cache de telemetría persistida para timeline.
    fn refresh_telemetry_persisted_cache(&mut self) {
        self.reset_telemetry_prefetch_state();
        let (local, persisted_result) = self.fetch_session_telemetry_with_persisted();
        let sid = Self::short_session_id(&local.session_id);
        self.status_text = match persisted_result {
            Ok(persisted) => format!(
                "telemetry sync ok session={} p(cp/an)={}/{}",
                sid,
                persisted.checkpoints.len(),
                persisted.anomalies.len()
            ),
            Err(err) => format!("telemetry sync error session={} ({})", sid, err),
        };
        self.needs_render = true;
    }

    fn merge_persisted_cache_pages(
        current: Option<SessionTelemetryResponse>,
        next: SessionTelemetryResponse,
    ) -> SessionTelemetryResponse {
        let mut merged = current.unwrap_or_else(|| SessionTelemetryResponse {
            schema_version: next.schema_version,
            session_id: next.session_id.clone(),
            checkpoints_total: 0,
            anomalies_total: 0,
            checkpoints_offset: 0,
            anomalies_offset: 0,
            has_more_checkpoints: false,
            has_more_anomalies: false,
            checkpoints: Vec::new(),
            anomalies: Vec::new(),
        });

        for cp in next.checkpoints {
            if !merged
                .checkpoints
                .iter()
                .any(|v| v.segment_seq == cp.segment_seq && v.step_end == cp.step_end)
            {
                merged.checkpoints.push(cp);
            }
        }
        merged.checkpoints.sort_by(|a, b| {
            a.segment_seq
                .cmp(&b.segment_seq)
                .then_with(|| a.step_end.cmp(&b.step_end))
        });

        for anomaly in next.anomalies {
            if !merged.anomalies.iter().any(|v| {
                v.segment_seq == anomaly.segment_seq
                    && v.step == anomaly.step
                    && v.kind.eq_ignore_ascii_case(&anomaly.kind)
            }) {
                merged.anomalies.push(anomaly);
            }
        }
        merged.anomalies.sort_by(|a, b| {
            a.step
                .cmp(&b.step)
                .then_with(|| a.segment_seq.cmp(&b.segment_seq))
                .then_with(|| a.kind.cmp(&b.kind))
        });

        merged.checkpoints_total = next.checkpoints_total.max(merged.checkpoints.len());
        merged.anomalies_total = next.anomalies_total.max(merged.anomalies.len());
        merged.checkpoints_offset = 0;
        merged.anomalies_offset = 0;
        merged.has_more_checkpoints = merged.checkpoints.len() < merged.checkpoints_total;
        merged.has_more_anomalies = merged.anomalies.len() < merged.anomalies_total;
        merged
    }

    fn load_more_telemetry_persisted_cache(&mut self) {
        self.reset_telemetry_prefetch_state();
        let quiron = self.quiron.clone();
        let local = self.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.session_telemetry_snapshot()
        });
        let sid = Self::short_session_id(&local.session_id);

        let (cp_offset, an_offset) = if let Some(cache) = self.telemetry_persisted_cache.as_ref() {
            if cache.session_id != local.session_id {
                (0, 0)
            } else if !cache.has_more_checkpoints && !cache.has_more_anomalies {
                self.status_text = format!("telemetry load-more: no more pages session={}", sid);
                self.needs_render = true;
                return;
            } else {
                (cache.checkpoints.len(), cache.anomalies.len())
            }
        } else {
            (0, 0)
        };

        let quiron = self.quiron.clone();
        let page_result = self.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.get_persisted_session_telemetry_page(
                Some(&local.session_id),
                RUNTIME_TELEMETRY_FETCH_LIMIT,
                cp_offset,
                an_offset,
            )
            .await
        });

        match page_result {
            Ok(page) => {
                let merged =
                    Self::merge_persisted_cache_pages(self.telemetry_persisted_cache.take(), page);
                self.telemetry_persisted_last_error = None;
                self.status_text = format!(
                    "telemetry load-more session={} loaded(cp/an)={}/{} total(cp/an)={}/{}",
                    sid,
                    merged.checkpoints.len(),
                    merged.anomalies.len(),
                    merged.checkpoints_total,
                    merged.anomalies_total
                );
                self.telemetry_persisted_cache = Some(merged);
            }
            Err(err) => {
                let err_msg = err.to_string();
                self.telemetry_persisted_last_error = Some(err_msg.clone());
                self.status_text =
                    format!("telemetry load-more error session={} ({})", sid, err_msg);
            }
        }
        self.needs_render = true;
    }

    /// Snapshot local resumido para render de panel de telemetría.
    pub fn session_telemetry_panel_info(&self) -> SessionTelemetryPanelInfo {
        let quiron = self.quiron.clone();
        let local = self.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.session_telemetry_snapshot()
        });

        let last_sample = local.recent_samples.last();
        let last_checkpoint = local.checkpoints.last();
        let last_anomaly = local.anomalies.last();

        let model_tokens_used_total = last_sample
            .map(|s| s.model_tokens_used_total)
            .or_else(|| last_checkpoint.map(|cp| cp.model_tokens_used_total))
            .unwrap_or(0);
        let token_budget = last_sample
            .map(|s| s.token_budget)
            .or_else(|| last_checkpoint.map(|cp| cp.token_budget))
            .unwrap_or(0);
        let token_budget_remaining = last_sample
            .map(|s| s.token_budget_remaining)
            .or_else(|| last_checkpoint.map(|cp| cp.token_budget_remaining))
            .unwrap_or(0);
        let fallback_rate = last_sample
            .map(|s| s.llm_fallback_rate)
            .or_else(|| last_checkpoint.map(|cp| cp.llm_fallback_rate))
            .unwrap_or(0.0);
        let persisted = self
            .telemetry_persisted_cache
            .as_ref()
            .filter(|cache| cache.session_id == local.session_id);
        let persisted_checkpoints = persisted.map(|v| v.checkpoints.len()).unwrap_or(0);
        let persisted_anomalies = persisted.map(|v| v.anomalies.len()).unwrap_or(0);
        let persisted_checkpoints_total = persisted.map(|v| v.checkpoints_total).unwrap_or(0);
        let persisted_anomalies_total = persisted.map(|v| v.anomalies_total).unwrap_or(0);
        let persisted_sync = if persisted.is_some() {
            if let Some(err) = self.telemetry_persisted_last_error.as_deref() {
                format!("stale({})", Self::truncate_for_report(err, 36))
            } else {
                "ok".to_string()
            }
        } else if let Some(err) = self.telemetry_persisted_last_error.as_deref() {
            format!("error({})", Self::truncate_for_report(err, 36))
        } else {
            "not_synced".to_string()
        };

        let mut timeline: Vec<SessionTelemetryTimelineEntry> = Vec::with_capacity(
            local
                .checkpoints
                .len()
                .saturating_add(local.anomalies.len())
                .saturating_add(persisted_checkpoints)
                .saturating_add(persisted_anomalies),
        );
        let mut seen_entries = HashSet::new();
        for cp in &local.checkpoints {
            let flags = if cp.anomaly_flags.is_empty() {
                "none".to_string()
            } else {
                cp.anomaly_flags.join(",")
            };
            let key = format!("cp:{}:{}", cp.segment_seq, cp.step_end);
            seen_entries.insert(key);
            timeline.push(SessionTelemetryTimelineEntry {
                is_anomaly: false,
                source: SessionTelemetryTimelineSource::Local,
                segment_seq: cp.segment_seq,
                step: cp.step_end,
                label: format!(
                    "cp seg={} steps={}..{} tok={}/{} fb={:.2} flags={}",
                    cp.segment_seq,
                    cp.step_start,
                    cp.step_end,
                    cp.token_budget_remaining,
                    cp.token_budget,
                    cp.llm_fallback_rate,
                    Self::truncate_for_report(&flags, 36)
                ),
            });
        }
        for anomaly in &local.anomalies {
            let key = format!(
                "an:{}:{}:{}",
                anomaly.segment_seq,
                anomaly.step,
                format!("{:?}", anomaly.kind).to_lowercase()
            );
            seen_entries.insert(key);
            timeline.push(SessionTelemetryTimelineEntry {
                is_anomaly: true,
                source: SessionTelemetryTimelineSource::Local,
                segment_seq: anomaly.segment_seq,
                step: anomaly.step,
                label: format!(
                    "an seg={} step={} kind={} iter={} {}",
                    anomaly.segment_seq,
                    anomaly.step,
                    format!("{:?}", anomaly.kind).to_lowercase(),
                    anomaly.request_iteration,
                    Self::truncate_for_report(&anomaly.detail, 44)
                ),
            });
        }
        if let Some(persisted) = persisted {
            for cp in &persisted.checkpoints {
                let key = format!("cp:{}:{}", cp.segment_seq, cp.step_end);
                if !seen_entries.insert(key) {
                    continue;
                }
                let flags = if cp.anomaly_flags.is_empty() {
                    "none".to_string()
                } else {
                    cp.anomaly_flags.join(",")
                };
                timeline.push(SessionTelemetryTimelineEntry {
                    is_anomaly: false,
                    source: SessionTelemetryTimelineSource::Persisted,
                    segment_seq: cp.segment_seq,
                    step: cp.step_end,
                    label: format!(
                        "cp seg={} steps={}..{} tok={}/{} fb={:.2} flags={}",
                        cp.segment_seq,
                        cp.step_start,
                        cp.step_end,
                        cp.token_budget_remaining,
                        cp.token_budget,
                        cp.llm_fallback_rate,
                        Self::truncate_for_report(&flags, 36)
                    ),
                });
            }
            for anomaly in &persisted.anomalies {
                let key = format!(
                    "an:{}:{}:{}",
                    anomaly.segment_seq,
                    anomaly.step,
                    anomaly.kind.to_lowercase()
                );
                if !seen_entries.insert(key) {
                    continue;
                }
                timeline.push(SessionTelemetryTimelineEntry {
                    is_anomaly: true,
                    source: SessionTelemetryTimelineSource::Persisted,
                    segment_seq: anomaly.segment_seq,
                    step: anomaly.step,
                    label: format!(
                        "an seg={} step={} kind={} iter={} {}",
                        anomaly.segment_seq,
                        anomaly.step,
                        anomaly.kind,
                        anomaly.request_iteration,
                        Self::truncate_for_report(&anomaly.detail, 44)
                    ),
                });
            }
        }
        timeline.sort_by(|a, b| {
            b.step
                .cmp(&a.step)
                .then_with(|| b.is_anomaly.cmp(&a.is_anomaly))
                .then_with(|| a.source.cmp(&b.source))
                .then_with(|| b.segment_seq.cmp(&a.segment_seq))
        });

        SessionTelemetryPanelInfo {
            session_id: local.session_id,
            parent_session_id: local.parent_session_id,
            segment_seq: local.segment_seq,
            checkpoints: local.checkpoints.len(),
            anomalies: local.anomalies.len(),
            samples: local.recent_samples.len(),
            model_tokens_used_total,
            token_budget,
            token_budget_remaining,
            fallback_rate,
            last_checkpoint_step_start: last_checkpoint.map(|cp| cp.step_start),
            last_checkpoint_step_end: last_checkpoint.map(|cp| cp.step_end),
            last_checkpoint_flags: last_checkpoint
                .map(|cp| cp.anomaly_flags.clone())
                .unwrap_or_default(),
            last_anomaly_kind: last_anomaly.map(|an| format!("{:?}", an.kind).to_lowercase()),
            last_anomaly_step: last_anomaly.map(|an| an.step),
            persisted_checkpoints,
            persisted_anomalies,
            persisted_checkpoints_total,
            persisted_anomalies_total,
            persisted_sync,
            timeline,
        }
    }

    fn show_session_telemetry_status(&mut self) {
        let (local, persisted_result) = self.fetch_session_telemetry_with_persisted();
        let sid = Self::short_session_id(&local.session_id);
        let status = match persisted_result {
            Ok(persisted) => format!(
                "telemetry session={} seg={} local(cp/an/smp)={}/{}/{} persisted(cp/an)={}/{}",
                sid,
                local.segment_seq,
                local.checkpoints.len(),
                local.anomalies.len(),
                local.recent_samples.len(),
                persisted.checkpoints.len(),
                persisted.anomalies.len()
            ),
            Err(err) => format!(
                "telemetry session={} seg={} local(cp/an/smp)={}/{}/{} persisted=error({})",
                sid,
                local.segment_seq,
                local.checkpoints.len(),
                local.anomalies.len(),
                local.recent_samples.len(),
                err
            ),
        };

        self.status_text = status;
        self.needs_render = true;
    }

    fn show_session_telemetry_report(&mut self) {
        let (local, persisted_result) = self.fetch_session_telemetry_with_persisted();

        let sid = Self::short_session_id(&local.session_id);
        let parent_sid = local
            .parent_session_id
            .as_deref()
            .map(Self::short_session_id)
            .unwrap_or_else(|| "-".to_string());

        let mut report_lines = vec![
            format!("Session telemetry report [{}]", sid),
            format!(
                "local seg={} checkpoints={} anomalies={} samples={} parent={}",
                local.segment_seq,
                local.checkpoints.len(),
                local.anomalies.len(),
                local.recent_samples.len(),
                parent_sid
            ),
        ];

        let status = match persisted_result {
            Ok(persisted) => {
                report_lines.push(format!(
                    "persisted checkpoints={} anomalies={}",
                    persisted.checkpoints.len(),
                    persisted.anomalies.len()
                ));
                report_lines.push(format!(
                    "last checkpoint: {}",
                    Self::format_checkpoint_report(
                        persisted.checkpoints.last(),
                        local.checkpoints.last()
                    )
                ));
                report_lines.push(format!(
                    "last anomaly: {}",
                    Self::format_anomaly_report(persisted.anomalies.last(), local.anomalies.last())
                ));
                format!(
                    "telemetry report posted cp={} an={}",
                    persisted.checkpoints.len(),
                    persisted.anomalies.len()
                )
            }
            Err(err) => {
                report_lines.push(format!("persisted telemetry unavailable: {}", err));
                report_lines.push(format!(
                    "last checkpoint (local): {}",
                    Self::format_checkpoint_report(None, local.checkpoints.last())
                ));
                report_lines.push(format!(
                    "last anomaly (local): {}",
                    Self::format_anomaly_report(None, local.anomalies.last())
                ));
                "telemetry report posted (persisted unavailable)".to_string()
            }
        };

        self.messages.push(ChatMessage {
            is_user: false,
            content: report_lines.join("\n"),
            meta: Some("telemetry_report".to_string()),
            citations: vec![],
        });
        self.status_text = status;
        self.needs_render = true;
    }

    fn execute_command_palette_action(&mut self, action: CommandPaletteAction) {
        self.close_top_menu();
        match action {
            CommandPaletteAction::NewTab => self.open_new_tab(),
            CommandPaletteAction::NewWindow => self.open_new_window(),
            CommandPaletteAction::OpenFilePicker => self.open_file_picker(),
            CommandPaletteAction::OpenFolderPicker => self.open_folder_picker(),
            CommandPaletteAction::NewChat => self.new_chat(),
            CommandPaletteAction::QuickOpen => self.begin_quick_open(),
            CommandPaletteAction::GoToLine => self.begin_go_to_line_overlay(),
            CommandPaletteAction::FindInWorkspace => self.begin_workspace_text_search_overlay(),
            CommandPaletteAction::GoToSymbol => self.begin_symbol_overlay(),
            CommandPaletteAction::GoToWorkspaceSymbol => self.begin_workspace_symbol_overlay(),
            CommandPaletteAction::ShowProblems => self.begin_problems_overlay(),
            CommandPaletteAction::ShowProblemsEditor => {
                self.begin_problems_overlay_with_query("editor")
            }
            CommandPaletteAction::ShowProblemsSearch => {
                self.begin_problems_overlay_with_query("search")
            }
            CommandPaletteAction::ShowProblemsGit => self.begin_problems_overlay_with_query("git"),
            CommandPaletteAction::ShowProblemsTelemetry => {
                self.begin_problems_overlay_with_query("telemetry")
            }
            CommandPaletteAction::ShowProblemsRuntime => {
                self.begin_problems_overlay_with_query("runtime")
            }
            CommandPaletteAction::ShowAppearancePanel => {
                self.set_sidebar_panel(SidebarPanel::Appearance)
            }
            CommandPaletteAction::ShowExplorerPanel => {
                self.set_sidebar_panel(SidebarPanel::Explorer)
            }
            CommandPaletteAction::ShowSearchPanel => self.set_sidebar_panel(SidebarPanel::Search),
            CommandPaletteAction::ShowGitPanel => self.set_sidebar_panel(SidebarPanel::Git),
            CommandPaletteAction::ShowOutlinePanel => self.set_sidebar_panel(SidebarPanel::Outline),
            CommandPaletteAction::ShowSecurityPanel => {
                self.set_sidebar_panel(SidebarPanel::Security)
            }
            CommandPaletteAction::NextProblem => {
                let _ = self.navigate_problem(1);
            }
            CommandPaletteAction::PreviousProblem => {
                let _ = self.navigate_problem(-1);
            }
            CommandPaletteAction::OpenToSide => self.open_selected_or_active_to_side(),
            CommandPaletteAction::ResetLayout => self.reset_layout(),
            CommandPaletteAction::ToggleEditorSplit => self.toggle_editor_pane_split(),
            CommandPaletteAction::MoveExplorerLeft => self.set_explorer_dock(PanelDock::Left),
            CommandPaletteAction::MoveExplorerRight => self.set_explorer_dock(PanelDock::Right),
            CommandPaletteAction::MoveChatLeft => self.set_chat_dock(PanelDock::Left),
            CommandPaletteAction::MoveChatRight => self.set_chat_dock(PanelDock::Right),
            CommandPaletteAction::CycleAiModel => self.cycle_ai_model(),
            CommandPaletteAction::ToggleTelemetryPanel => self.toggle_telemetry_panel(),
            CommandPaletteAction::CycleTelemetryTimelineFilter => {
                self.cycle_telemetry_timeline_filter()
            }
            CommandPaletteAction::RefreshTelemetryPersistedCache => {
                self.refresh_telemetry_persisted_cache()
            }
            CommandPaletteAction::LoadMoreTelemetryPersistedCache => {
                self.load_more_telemetry_persisted_cache()
            }
            CommandPaletteAction::Save => self.save_active_file(),
            CommandPaletteAction::SaveAs => self.save_active_file_as(),
            CommandPaletteAction::CloseTab => self.request_close_active_tab(),
            CommandPaletteAction::Find => self.begin_search_mode(),
            CommandPaletteAction::Replace => self.begin_replace_mode(),
            CommandPaletteAction::AddSelectionToChat => {
                self.add_selection_to_chat_input();
            }
            CommandPaletteAction::ExplorerNewFile => self.explorer_create_new_file(),
            CommandPaletteAction::ExplorerNewFolder => self.explorer_create_new_folder(),
            CommandPaletteAction::RefreshExplorer => {
                self.refresh_explorer();
                self.invalidate_workspace_cache();
                self.status_text = "explorer refreshed".to_string();
                self.needs_render = true;
            }
            CommandPaletteAction::RevealActiveFile => {
                if self.reveal_active_file_in_explorer() {
                    self.status_text = "explorer revealed active file".to_string();
                    self.needs_render = true;
                }
            }
            CommandPaletteAction::FocusEditor => self.set_focus(FocusTarget::EditorPrimary),
            CommandPaletteAction::FocusChat => self.set_focus(FocusTarget::ChatInput),
            CommandPaletteAction::NextTab => self.cycle_tabs(),
            CommandPaletteAction::PreviousTab => self.cycle_tabs_previous(),
            CommandPaletteAction::SetThemeQuironDark => self.set_ui_theme(UiTheme::QuironDark),
            CommandPaletteAction::SetThemeGraphiteDark => self.set_ui_theme(UiTheme::GraphiteDark),
            CommandPaletteAction::SetThemeCopperLight => self.set_ui_theme(UiTheme::CopperLight),
            CommandPaletteAction::SetThemeModernistLight => {
                self.set_ui_theme(UiTheme::ModernistLight)
            }
            CommandPaletteAction::CycleThemeNext => self.cycle_ui_theme_next(),
            CommandPaletteAction::CycleThemePrevious => self.cycle_ui_theme_previous(),
            CommandPaletteAction::SetDensityCompact => self.set_ui_density(UiDensity::Compact),
            CommandPaletteAction::SetDensityNormal => self.set_ui_density(UiDensity::Normal),
            CommandPaletteAction::SetDensityComfortable => {
                self.set_ui_density(UiDensity::Comfortable)
            }
            CommandPaletteAction::CycleDensityNext => self.cycle_ui_density_next(),
            CommandPaletteAction::ApplyAppearancePresetDev => {
                self.apply_ui_appearance_preset(UiAppearancePreset::Dev)
            }
            CommandPaletteAction::ApplyAppearancePresetFocus => {
                self.apply_ui_appearance_preset(UiAppearancePreset::Focus)
            }
            CommandPaletteAction::ApplyAppearancePresetReading => {
                self.apply_ui_appearance_preset(UiAppearancePreset::Reading)
            }
            CommandPaletteAction::ResetAppearance => self.reset_ui_appearance(),
            CommandPaletteAction::IncreaseEditorFontScale => self.increase_editor_font_scale(),
            CommandPaletteAction::DecreaseEditorFontScale => self.decrease_editor_font_scale(),
            CommandPaletteAction::ResetEditorFontScale => self.reset_editor_font_scale(),
            CommandPaletteAction::IncreaseUiScale => self.increase_ui_scale(),
            CommandPaletteAction::DecreaseUiScale => self.decrease_ui_scale(),
            CommandPaletteAction::ResetUiScale => self.reset_ui_scale(),
            CommandPaletteAction::IncreaseEditorHorizontalPadding => {
                self.increase_editor_horizontal_padding()
            }
            CommandPaletteAction::DecreaseEditorHorizontalPadding => {
                self.decrease_editor_horizontal_padding()
            }
            CommandPaletteAction::ResetEditorHorizontalPadding => {
                self.reset_editor_horizontal_padding()
            }
            CommandPaletteAction::IncreaseExplorerIndent => self.increase_explorer_indent_step(),
            CommandPaletteAction::DecreaseExplorerIndent => self.decrease_explorer_indent_step(),
            CommandPaletteAction::ResetExplorerIndent => self.reset_explorer_indent_step(),
            CommandPaletteAction::SetRuntimeBudgetTight => {
                self.apply_runtime_token_budget(RUNTIME_TOKEN_BUDGET_TIGHT);
            }
            CommandPaletteAction::SetRuntimeBudgetBalanced => {
                self.apply_runtime_token_budget(RUNTIME_TOKEN_BUDGET_BALANCED);
            }
            CommandPaletteAction::SetRuntimeBudgetDeep => {
                self.apply_runtime_token_budget(RUNTIME_TOKEN_BUDGET_DEEP);
            }
            CommandPaletteAction::SetRuntimeThresholdBalanced => {
                self.apply_runtime_threshold_scales(
                    RUNTIME_WORKER_SCALE_BALANCED,
                    RUNTIME_PRIMARY_SCALE_BALANCED,
                );
            }
            CommandPaletteAction::SetRuntimeThresholdWorkerBias => {
                self.apply_runtime_threshold_scales(
                    RUNTIME_WORKER_SCALE_WORKER_BIAS,
                    RUNTIME_PRIMARY_SCALE_WORKER_BIAS,
                );
            }
            CommandPaletteAction::SetRuntimeThresholdPrimaryBias => {
                self.apply_runtime_threshold_scales(
                    RUNTIME_WORKER_SCALE_PRIMARY_BIAS,
                    RUNTIME_PRIMARY_SCALE_PRIMARY_BIAS,
                );
            }
            CommandPaletteAction::SetRuntimeParallelConservative => {
                self.apply_runtime_parallel_cap(RUNTIME_PARALLEL_CAP_CONSERVATIVE);
            }
            CommandPaletteAction::SetRuntimeParallelBalanced => {
                self.apply_runtime_parallel_cap(RUNTIME_PARALLEL_CAP_BALANCED);
            }
            CommandPaletteAction::SetRuntimeParallelWide => {
                self.apply_runtime_parallel_cap(RUNTIME_PARALLEL_CAP_WIDE);
            }
            CommandPaletteAction::ShowSessionTelemetryStatus => {
                self.show_session_telemetry_status();
            }
            CommandPaletteAction::ShowSessionTelemetryReport => {
                self.show_session_telemetry_report();
            }
            CommandPaletteAction::ReconnectQuironSecureLocal => {
                match self.reconnect_quiron_with_mode("http://localhost:8766", true) {
                    Ok(()) => {
                        self.status_text = format!(
                            "secure reconnect local applied | {} | {} | health=checking",
                            self.quiron_connection_endpoint_label(),
                            self.quiron_connection_mode_label(),
                        );
                    }
                    Err(err) => {
                        self.status_text = format!("secure reconnect local failed: {}", err);
                    }
                }
                self.needs_render = true;
            }
            CommandPaletteAction::ReconnectQuironSecureEnv => {
                let target = Self::env_non_empty(QUIRON_BRAIN_URL_ENV)
                    .unwrap_or_else(|| "http://localhost:8766".to_string());
                match self.reconnect_quiron_with_mode(&target, true) {
                    Ok(()) => {
                        self.status_text = format!(
                            "secure reconnect env applied | {} | {} | health=checking",
                            self.quiron_connection_endpoint_label(),
                            self.quiron_connection_mode_label(),
                        );
                    }
                    Err(err) => {
                        self.status_text = format!("secure reconnect env failed: {}", err);
                    }
                }
                self.needs_render = true;
            }
            CommandPaletteAction::ShowQuironConnectionStatus => {
                self.post_connection_status_message("command-palette");
            }
        }
    }

    fn execute_overlay_selection(&mut self) {
        let Some(item) = self.overlay_items.get(self.overlay_selected).cloned() else {
            self.status_text = "overlay: no results".to_string();
            self.needs_render = true;
            return;
        };

        match item.action {
            OverlayAction::OpenFile(path) => {
                self.reset_overlay_state();
                self.open_file_from_explorer(path);
            }
            OverlayAction::OpenFileAtLocation { path, line, column } => {
                let pane = self.focused_editor_pane;
                self.reset_overlay_state();
                self.open_file_in_pane(path, pane, false);
                self.editor_for_pane_mut(pane)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("symbol L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
            OverlayAction::JumpToSymbol { pane, line, column } => {
                self.reset_overlay_state();
                self.set_focus(match pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                self.editor_for_pane_mut(pane)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("symbol L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
            OverlayAction::JumpToLine { pane, line, column } => {
                self.reset_overlay_state();
                self.set_focus(match pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                self.editor_for_pane_mut(pane)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("line L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
            OverlayAction::ProblemSelect { title, index } => {
                self.reset_overlay_state();
                if !self.activate_problem_index(index) {
                    self.status_text = format!("problem unavailable: {}", title);
                    self.needs_render = true;
                }
            }
            OverlayAction::Command(action) => {
                self.reset_overlay_state();
                self.execute_command_palette_action(action);
            }
        }
    }

    fn execute_overlay_selection_to_side(&mut self) {
        let Some(item) = self.overlay_items.get(self.overlay_selected).cloned() else {
            self.status_text = "overlay: no results".to_string();
            self.needs_render = true;
            return;
        };

        match item.action {
            OverlayAction::OpenFile(path) => {
                self.reset_overlay_state();
                self.open_file_to_side(path);
            }
            OverlayAction::OpenFileAtLocation { path, line, column } => {
                self.reset_overlay_state();
                self.open_file_in_pane(path, EditorPane::Secondary, true);
                self.editor_for_pane_mut(EditorPane::Secondary)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("symbol side L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
            OverlayAction::JumpToSymbol { pane, line, column } => {
                self.reset_overlay_state();
                self.set_focus(match pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                self.editor_for_pane_mut(pane)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("symbol L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
            OverlayAction::JumpToLine { pane, line, column } => {
                self.reset_overlay_state();
                self.set_focus(match pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                self.editor_for_pane_mut(pane)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("line L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
            OverlayAction::ProblemSelect { title, index } => {
                self.reset_overlay_state();
                if !self.activate_problem_index(index) {
                    self.status_text = format!("problem unavailable: {}", title);
                    self.needs_render = true;
                }
            }
            OverlayAction::Command(action) => {
                self.reset_overlay_state();
                self.execute_command_palette_action(action);
            }
        }
    }

    /// Maneja teclado mientras un overlay de selección está activo.
    pub fn handle_overlay_key(&mut self, key: &Key, shift: bool, ctrl: bool, alt: bool) -> bool {
        if self.overlay_mode.is_none() {
            return false;
        }

        match key {
            Key::Named(NamedKey::Escape) => self.close_overlay(),
            Key::Named(NamedKey::Enter) => {
                if alt
                    && matches!(
                        self.overlay_mode,
                        Some(
                            OverlayMode::QuickOpen
                                | OverlayMode::WorkspaceSymbols
                                | OverlayMode::WorkspaceTextSearch
                        )
                    )
                {
                    self.execute_overlay_selection_to_side();
                } else {
                    self.execute_overlay_selection();
                }
            }
            Key::Named(NamedKey::ArrowDown) => self.move_overlay_selection(1),
            Key::Named(NamedKey::ArrowUp) => self.move_overlay_selection(-1),
            Key::Named(NamedKey::PageDown) => self.move_overlay_selection_page(1),
            Key::Named(NamedKey::PageUp) => self.move_overlay_selection_page(-1),
            Key::Named(NamedKey::Home) => {
                self.overlay_selected = 0;
                self.needs_render = true;
            }
            Key::Named(NamedKey::End) => {
                self.overlay_selected = self.overlay_items.len().saturating_sub(1);
                self.needs_render = true;
            }
            Key::Named(NamedKey::Tab) => {
                if shift {
                    self.move_overlay_selection(-1);
                } else {
                    self.move_overlay_selection(1);
                }
            }
            Key::Named(NamedKey::Backspace) => {
                self.overlay_query.pop();
                self.rebuild_overlay_items_for_active_mode();
            }
            Key::Named(NamedKey::Space) => {
                self.overlay_query.push(' ');
                self.rebuild_overlay_items_for_active_mode();
            }
            Key::Character(ch) => {
                let lower = ch.to_lowercase();
                if ctrl && lower == "n" {
                    self.move_overlay_selection(1);
                    return true;
                }
                if ctrl && lower == "p" {
                    self.move_overlay_selection(-1);
                    return true;
                }
                if ctrl && lower == "l" {
                    self.overlay_query.clear();
                    self.rebuild_overlay_items_for_active_mode();
                    return true;
                }
                if ctrl {
                    return true;
                }

                self.overlay_query.push_str(ch);
                self.rebuild_overlay_items_for_active_mode();
            }
            _ => {
                self.needs_render = true;
            }
        }

        true
    }

    /// Ajusta scroll del explorador por delta de líneas (positivo baja, negativo sube).
    pub fn scroll_explorer_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }

        let next = if delta_lines > 0 {
            self.explorer_scroll.saturating_add(delta_lines as usize)
        } else {
            self.explorer_scroll
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.explorer_scroll = next.min(self.explorer_entries.len().saturating_sub(1));
        self.needs_render = true;
    }

    /// Ajusta scroll del panel Search del sidebar.
    pub fn scroll_sidebar_search_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        let next = if delta_lines > 0 {
            self.sidebar_search_scroll
                .saturating_add(delta_lines as usize)
        } else {
            self.sidebar_search_scroll
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.sidebar_search_scroll = next;
        self.needs_render = true;
    }

    /// Ajusta scroll del panel Problems del sidebar.
    pub fn scroll_sidebar_problems_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        let next = if delta_lines > 0 {
            self.sidebar_problems_scroll
                .saturating_add(delta_lines as usize)
        } else {
            self.sidebar_problems_scroll
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.sidebar_problems_scroll = next;
        self.needs_render = true;
    }

    /// Ajusta scroll del panel Outline del sidebar.
    pub fn scroll_sidebar_outline_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        let next = if delta_lines > 0 {
            self.sidebar_outline_scroll
                .saturating_add(delta_lines as usize)
        } else {
            self.sidebar_outline_scroll
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.sidebar_outline_scroll = next;
        self.needs_render = true;
    }

    /// Ajusta scroll contextual según panel activo del sidebar.
    pub fn scroll_sidebar_lines(&mut self, delta_lines: i32) {
        match self.sidebar_panel {
            SidebarPanel::Explorer => self.scroll_explorer_lines(delta_lines),
            SidebarPanel::Search => self.scroll_sidebar_search_lines(delta_lines),
            SidebarPanel::Git => {}
            SidebarPanel::Problems => self.scroll_sidebar_problems_lines(delta_lines),
            SidebarPanel::Outline => self.scroll_sidebar_outline_lines(delta_lines),
            SidebarPanel::Appearance => {}
            SidebarPanel::Security => {}
        }
    }

    /// Ajusta scroll del editor activo por delta de líneas.
    pub fn scroll_editor_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        self.active_editor_mut().scroll_lines(delta_lines);
        self.needs_render = true;
    }

    /// Ajusta scroll de un panel de editor específico por delta de líneas.
    pub fn scroll_editor_lines_in_pane(&mut self, pane: EditorPane, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        self.editor_for_pane_mut(pane).scroll_lines(delta_lines);
        self.needs_render = true;
    }

    /// Ajusta scroll del panel de resultados de búsqueda.
    pub fn scroll_search_results_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 || self.search_matches.is_empty() {
            return;
        }
        let visible_rows = SEARCH_RESULTS_MAX_ROWS.min(self.search_matches.len());
        let max_scroll = self.search_matches.len().saturating_sub(visible_rows);
        let next = if delta_lines > 0 {
            self.search_results_scroll
                .saturating_add(delta_lines as usize)
        } else {
            self.search_results_scroll
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.search_results_scroll = next.min(max_scroll);
        self.needs_render = true;
    }

    /// Convierte un punto de pantalla a posición línea/columna del editor.
    fn editor_position_from_point(&self, x: f32, y: f32) -> Option<(EditorPane, usize, usize)> {
        let Some(pane) = self.editor_pane_at_point(x, y) else {
            return None;
        };
        let Some(editor_bounds) = self.editor_pane_bounds(pane) else {
            return None;
        };

        let line_height = self.editor_line_height();
        let char_width = self.editor_char_width();
        let body_top = editor_bounds.y + EDITOR_TAB_BAR_HEIGHT + EDITOR_BODY_TOP_PADDING;
        let body_height =
            (editor_bounds.height - EDITOR_TAB_BAR_HEIGHT - EDITOR_BODY_BOTTOM_PADDING)
                .max(line_height);
        let body_bottom = body_top + body_height;
        if y < body_top || y > body_bottom {
            return None;
        }

        let editor = self.editor_for_pane(pane);
        let text = editor.text();
        let lines: Vec<&str> = text.split('\n').collect();
        let max_line = lines.len().saturating_sub(1);

        let row = ((y - body_top) / line_height).floor().max(0.0) as usize;
        let target_line = editor.scroll_top().saturating_add(row).min(max_line);

        let relative_x = (x - (editor_bounds.x + EDITOR_GUTTER_WIDTH)).max(0.0);
        let visible_col = (relative_x / char_width).floor().max(0.0) as usize;
        let target_col = editor.scroll_left().saturating_add(visible_col).min(
            lines
                .get(target_line)
                .map(|l| l.chars().count())
                .unwrap_or(0),
        );
        Some((pane, target_line, target_col))
    }

    /// Posiciona cursor del editor según coordenadas de click.
    pub fn place_cursor_from_point(&mut self, x: f32, y: f32) -> bool {
        let Some((pane, target_line, target_col)) = self.editor_position_from_point(x, y) else {
            return false;
        };
        self.set_focus(match pane {
            EditorPane::Primary => FocusTarget::EditorPrimary,
            EditorPane::Secondary => FocusTarget::EditorSecondary,
        });
        self.editor_for_pane_mut(pane)
            .set_cursor_line_column(target_line, target_col);
        self.status_text = format!("cursor Ln {}, Col {}", target_line + 1, target_col + 1);
        self.needs_render = true;
        true
    }

    /// Maneja click en body de editor (cursor, doble click palabra, triple click línea).
    pub fn handle_editor_mouse_press(&mut self, x: f32, y: f32) -> bool {
        let Some((pane, line, col)) = self.editor_position_from_point(x, y) else {
            return false;
        };

        self.set_focus(match pane {
            EditorPane::Primary => FocusTarget::EditorPrimary,
            EditorPane::Secondary => FocusTarget::EditorSecondary,
        });

        let now = Instant::now();
        let tab_index = self.pane_tab_index(pane);
        let click_count = self
            .last_editor_click
            .map(|(ts, tab, prev_line, prev_col, prev_count)| {
                let col_diff = prev_col.max(col) - prev_col.min(col);
                let is_repeated_click = tab == tab_index
                    && prev_line == line
                    && col_diff <= 1
                    && now.duration_since(ts) <= DOUBLE_CLICK_THRESHOLD;
                if is_repeated_click {
                    prev_count.saturating_add(1).min(3)
                } else {
                    1
                }
            })
            .unwrap_or(1);

        self.last_editor_click = Some((now, tab_index, line, col, click_count));

        if click_count >= 3 {
            self.mouse_selection_anchor = None;
            self.editor_for_pane_mut(pane).select_line_at(line);
            let selected_len = self.editor_for_pane(pane).selection().len();
            self.status_text = format!("selected line ({} chars)", selected_len);
        } else if click_count == 2 {
            self.mouse_selection_anchor = None;
            self.editor_for_pane_mut(pane).select_word_at(line, col);
            let selected_len = self.editor_for_pane(pane).selection().len();
            self.status_text = if selected_len > 0 {
                format!("selected {} chars", selected_len)
            } else {
                format!("cursor Ln {}, Col {}", line + 1, col + 1)
            };
        } else {
            self.editor_for_pane_mut(pane)
                .set_cursor_line_column(line, col);
            self.mouse_selection_anchor = Some((pane, line, col));
            self.status_text = format!("cursor Ln {}, Col {}", line + 1, col + 1);
        }

        self.needs_render = true;
        true
    }

    /// Actualiza selección por drag en el editor correspondiente al anchor.
    pub fn update_mouse_selection_from_point(&mut self, x: f32, y: f32) -> bool {
        let Some((anchor_pane, anchor_line, anchor_col)) = self.mouse_selection_anchor else {
            return false;
        };
        let Some((pane, line, col)) = self.editor_position_from_point(x, y) else {
            return false;
        };
        if pane != anchor_pane {
            return false;
        }

        self.editor_for_pane_mut(anchor_pane)
            .set_selection_line_columns(anchor_line, anchor_col, line, col);
        let selected_len = self.editor_for_pane(anchor_pane).selection().len();
        self.status_text = format!("selection {} chars", selected_len);
        self.needs_render = true;
        true
    }

    /// Finaliza ciclo de drag de selección por mouse.
    pub fn end_mouse_selection(&mut self) {
        self.mouse_selection_anchor = None;
    }

    fn write_clipboard_text(&mut self, text: &str) -> Result<(), String> {
        self.clipboard_fallback = text.to_string();
        let mut clipboard = Clipboard::new().map_err(|err| err.to_string())?;
        clipboard
            .set_text(text.to_string())
            .map_err(|err| err.to_string())
    }

    fn read_clipboard_text(&mut self) -> Option<String> {
        if let Ok(mut clipboard) = Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                self.clipboard_fallback = text.clone();
                return Some(text);
            }
        }

        if self.clipboard_fallback.is_empty() {
            None
        } else {
            Some(self.clipboard_fallback.clone())
        }
    }

    fn selected_text_owned(&self) -> Option<String> {
        let text = self.active_editor().text();
        let selection = self.active_editor().selection().clone();
        if selection.is_empty() {
            return None;
        }
        Some(selection.selected_text(&text).to_string())
    }

    fn truncate_selection_for_chat(text: &str, max_chars: usize) -> (String, bool) {
        let mut out = String::new();
        let mut truncated = false;
        for (idx, ch) in text.chars().enumerate() {
            if idx >= max_chars {
                truncated = true;
                break;
            }
            out.push(ch);
        }
        (out, truncated)
    }

    /// Inserta la selección activa del editor en el input del chat como contexto.
    pub fn add_selection_to_chat_input(&mut self) -> bool {
        let text = self.active_editor().text();
        let selection = self.active_editor().selection().clone();
        if selection.is_empty() {
            self.status_text = "chat context: select text first".to_string();
            self.needs_render = true;
            return false;
        }

        let selected = selection.selected_text(&text).to_string();
        if selected.is_empty() {
            self.status_text = "chat context: selection is empty".to_string();
            self.needs_render = true;
            return false;
        }

        let (start_cursor, end_cursor) = if selection.anchor.offset <= selection.head.offset {
            (&selection.anchor, &selection.head)
        } else {
            (&selection.head, &selection.anchor)
        };
        let file_label = self
            .active_file_path()
            .map(|path| self.workspace_relative_label(path))
            .unwrap_or_else(|| "untitled".to_string());
        let lines_label = if start_cursor.line == end_cursor.line {
            format!("{}", start_cursor.line + 1)
        } else {
            format!("{}-{}", start_cursor.line + 1, end_cursor.line + 1)
        };
        let lang_hint = self
            .active_file_path()
            .and_then(|path| path.extension())
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        let (snippet, was_truncated) =
            Self::truncate_selection_for_chat(&selected, CHAT_SELECTION_MAX_CHARS);
        let mut block = format!("@context {}:{}\n", file_label, lines_label);
        if lang_hint.is_empty() {
            block.push_str("```\n");
        } else {
            block.push_str(&format!("```{}\n", lang_hint));
        }
        block.push_str(&snippet);
        if !snippet.ends_with('\n') {
            block.push('\n');
        }
        block.push_str("```\n");
        if was_truncated {
            block.push_str(&format!(
                "[selection truncated to {} chars]\n",
                CHAT_SELECTION_MAX_CHARS
            ));
        }

        if !self.input_text.is_empty() && !self.input_text.ends_with('\n') {
            self.input_text.push_str("\n\n");
        } else if !self.input_text.is_empty() {
            self.input_text.push('\n');
        }
        self.input_text.push_str(&block);
        self.set_focus(FocusTarget::ChatInput);
        self.status_text = format!(
            "chat context added {} chars {}:{}",
            snippet.chars().count(),
            file_label,
            lines_label
        );
        self.needs_render = true;
        true
    }

    fn capture_selection_scope(&self) -> Option<SearchScope> {
        let selection = self.active_editor().selection();
        if selection.is_empty() {
            return None;
        }
        Some(SearchScope {
            start_offset: selection.start_offset(),
            end_offset: selection.end_offset(),
        })
    }

    fn match_is_in_scope(&self, start_offset: usize, end_offset: usize) -> bool {
        if !(self.replace_active && self.replace_in_selection_only) {
            return true;
        }
        let Some(scope) = self.replace_selection_scope else {
            return false;
        };
        start_offset >= scope.start_offset && end_offset <= scope.end_offset
    }

    fn apply_replace_scope_delta(&mut self, delta_bytes: isize) {
        if !(self.replace_active && self.replace_in_selection_only) || delta_bytes == 0 {
            return;
        }
        let Some(scope) = self.replace_selection_scope.as_mut() else {
            return;
        };
        let next_end = scope.end_offset as isize + delta_bytes;
        scope.end_offset = next_end.max(scope.start_offset as isize) as usize;
    }

    fn toggle_replace_in_selection_scope(&mut self) {
        if !self.replace_active {
            self.status_text = "replace mode is off (Ctrl+H)".to_string();
            self.needs_render = true;
            return;
        }

        if self.replace_in_selection_only {
            self.replace_in_selection_only = false;
            self.rebuild_search_matches();
            return;
        }

        if let Some(scope) = self.capture_selection_scope() {
            self.replace_selection_scope = Some(scope);
        }

        if self.replace_selection_scope.is_none() {
            self.status_text = "replace scope needs an editor selection".to_string();
            self.needs_render = true;
            return;
        }

        self.replace_in_selection_only = true;
        self.rebuild_search_matches();
    }

    fn clear_replace_all_preview(&mut self) {
        self.replace_all_confirm_pending = false;
    }

    fn active_search_input_mut(&mut self) -> &mut String {
        match self.search_input_focus {
            SearchInputFocus::Find => &mut self.search_query,
            SearchInputFocus::Replace => &mut self.replace_query,
        }
    }

    fn toggle_search_input_focus(&mut self) {
        self.search_input_focus = match self.search_input_focus {
            SearchInputFocus::Find => SearchInputFocus::Replace,
            SearchInputFocus::Replace => SearchInputFocus::Find,
        };
        self.needs_render = true;
    }

    fn refresh_search_matches_if_active(&mut self) {
        if self.search_active {
            self.rebuild_search_matches();
        }
    }

    fn clamp_search_results_scroll(&mut self) {
        if self.search_matches.is_empty() {
            self.search_results_scroll = 0;
            return;
        }
        let visible_rows = SEARCH_RESULTS_MAX_ROWS.min(self.search_matches.len());
        let max_scroll = self.search_matches.len().saturating_sub(visible_rows);
        self.search_results_scroll = self.search_results_scroll.min(max_scroll);
    }

    fn ensure_active_search_visible(&mut self) {
        if self.search_matches.is_empty() {
            self.search_results_scroll = 0;
            return;
        }
        let visible_rows = SEARCH_RESULTS_MAX_ROWS
            .min(self.search_matches.len())
            .max(1);
        let max_scroll = self.search_matches.len().saturating_sub(visible_rows);
        let active_idx = self
            .active_search_match
            .unwrap_or(0)
            .min(self.search_matches.len().saturating_sub(1));

        if active_idx < self.search_results_scroll {
            self.search_results_scroll = active_idx;
        } else {
            let visible_end = self.search_results_scroll.saturating_add(visible_rows);
            if active_idx >= visible_end {
                self.search_results_scroll =
                    active_idx.saturating_add(1).saturating_sub(visible_rows);
            }
        }
        self.search_results_scroll = self.search_results_scroll.min(max_scroll);
    }

    fn line_start_offsets(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        for (idx, ch) in text.char_indices() {
            if ch == '\n' && idx < text.len() {
                starts.push(idx.saturating_add(1));
            }
        }
        starts
    }

    fn line_col_from_offset(text: &str, line_starts: &[usize], offset: usize) -> (usize, usize) {
        let clamped = offset.min(text.len());
        let line = match line_starts.binary_search(&clamped) {
            Ok(idx) => idx,
            Err(insert_idx) => insert_idx.saturating_sub(1),
        };
        let line_start = line_starts.get(line).copied().unwrap_or(0);
        let col = text[line_start..clamped].chars().count();
        (line, col)
    }

    fn build_search_pattern(&self) -> String {
        let base_pattern = if self.search_regex_mode {
            self.search_query.clone()
        } else {
            regex::escape(&self.search_query)
        };
        if self.search_whole_word {
            format!(r"\b(?:{})\b", base_pattern)
        } else {
            base_pattern
        }
    }

    fn build_search_regex(&self) -> Result<Regex, regex::Error> {
        RegexBuilder::new(&self.build_search_pattern())
            .case_insensitive(!self.search_match_case)
            .build()
    }

    fn replacement_for_match_in_text(
        &self,
        source_text: &str,
        matched: &SearchMatch,
        compiled_regex: Option<&Regex>,
    ) -> Result<String, String> {
        if matched.start_offset > matched.end_offset || matched.end_offset > source_text.len() {
            return Err("replace match out of bounds".to_string());
        }

        if let Some(regex) = compiled_regex {
            let captures = regex
                .captures_at(source_text, matched.start_offset)
                .ok_or_else(|| "replace capture mismatch".to_string())?;
            let full = captures
                .get(0)
                .ok_or_else(|| "replace capture mismatch".to_string())?;
            if full.start() != matched.start_offset || full.end() != matched.end_offset {
                return Err("replace capture mismatch".to_string());
            }
            let mut expanded = String::new();
            captures.expand(self.replace_query.as_str(), &mut expanded);
            return Ok(expanded);
        }

        Ok(self.replace_query.clone())
    }

    fn search_flags_label(&self) -> String {
        let case = if self.search_match_case {
            "case"
        } else {
            "nocase"
        };
        let word = if self.search_whole_word {
            "word"
        } else {
            "any"
        };
        let regex_mode = if self.search_regex_mode {
            "regex"
        } else {
            "literal"
        };
        let scope = if self.replace_active && self.replace_in_selection_only {
            "sel"
        } else {
            "all"
        };
        format!("{},{},{},{}", case, word, regex_mode, scope)
    }

    fn apply_active_search_match(&mut self) {
        self.ensure_active_search_visible();
        let Some(idx) = self.active_search_match else {
            return;
        };
        let Some(matched) = self.search_matches.get(idx).cloned() else {
            return;
        };
        self.active_editor_mut().set_selection_line_columns(
            matched.start_line,
            matched.start_col,
            matched.end_line,
            matched.end_col,
        );
        self.active_editor_mut()
            .set_scroll_top(matched.start_line.saturating_sub(2));
        self.status_text = format!(
            "find: '{}' ({}/{}) [{}]",
            self.search_query,
            idx + 1,
            self.search_matches.len(),
            self.search_flags_label()
        );
    }

    fn rebuild_search_matches(&mut self) {
        self.clear_replace_all_preview();
        self.search_matches.clear();
        self.active_search_match = None;

        if self.search_query.is_empty() {
            self.search_results_scroll = 0;
            self.status_text = "find: type query".to_string();
            self.needs_render = true;
            return;
        }

        let text = self.active_editor().text();
        let regex = match self.build_search_regex() {
            Ok(re) => re,
            Err(err) => {
                self.search_results_scroll = 0;
                self.status_text = format!("find pattern error: {}", err);
                self.needs_render = true;
                return;
            }
        };

        let line_starts = Self::line_start_offsets(&text);
        for found in regex.find_iter(&text) {
            let start_offset = found.start();
            let end_offset = found.end();
            if !self.match_is_in_scope(start_offset, end_offset) {
                continue;
            }
            let (start_line, start_col) =
                Self::line_col_from_offset(&text, &line_starts, start_offset);
            let (end_line, end_col) = Self::line_col_from_offset(&text, &line_starts, end_offset);
            self.search_matches.push(SearchMatch {
                start_line,
                start_col,
                end_line,
                end_col,
                start_offset,
                end_offset,
            });
        }
        self.clamp_search_results_scroll();

        if self.search_matches.is_empty() {
            self.status_text = format!(
                "find: '{}' (0/0) [{}]",
                self.search_query,
                self.search_flags_label()
            );
            self.needs_render = true;
            return;
        }

        self.active_search_match = Some(0);
        self.apply_active_search_match();
        self.needs_render = true;
    }

    /// Activa modo búsqueda incremental.
    pub fn begin_search_mode(&mut self) {
        self.search_active = true;
        self.replace_active = false;
        self.replace_in_selection_only = false;
        self.replace_selection_scope = None;
        self.search_input_focus = SearchInputFocus::Find;
        self.rebuild_search_matches();
        self.needs_render = true;
    }

    /// Activa modo búsqueda+reemplazo.
    pub fn begin_replace_mode(&mut self) {
        let selection_scope = self.capture_selection_scope();
        if self.search_query.is_empty() {
            if let Some(selection) = self.selected_text_owned() {
                if !selection.contains('\n') && !selection.is_empty() {
                    self.search_query = selection;
                }
            }
        }
        self.search_active = true;
        self.replace_active = true;
        self.search_input_focus = if self.search_query.is_empty() {
            SearchInputFocus::Find
        } else {
            SearchInputFocus::Replace
        };
        self.replace_in_selection_only = selection_scope.is_some();
        self.replace_selection_scope = selection_scope;
        self.rebuild_search_matches();
        self.needs_render = true;
    }

    /// Cierra modo búsqueda incremental.
    pub fn close_search_mode(&mut self) {
        self.clear_replace_all_preview();
        self.search_active = false;
        self.replace_active = false;
        self.replace_in_selection_only = false;
        self.replace_selection_scope = None;
        self.search_results_scroll = 0;
        self.clear_search_results_bounds();
        self.replace_query.clear();
        self.search_input_focus = SearchInputFocus::Find;
        self.search_matches.clear();
        self.active_search_match = None;
        self.status_text = "search closed".to_string();
        self.needs_render = true;
    }

    /// Navega al siguiente match de búsqueda.
    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            self.status_text = "find: no matches".to_string();
            self.needs_render = true;
            return;
        }
        let next = self
            .active_search_match
            .map(|idx| (idx + 1) % self.search_matches.len())
            .unwrap_or(0);
        self.active_search_match = Some(next);
        self.apply_active_search_match();
        self.needs_render = true;
    }

    /// Navega al match previo de búsqueda.
    pub fn search_previous(&mut self) {
        if self.search_matches.is_empty() {
            self.status_text = "find: no matches".to_string();
            self.needs_render = true;
            return;
        }
        let prev = self
            .active_search_match
            .map(|idx| {
                if idx == 0 {
                    self.search_matches.len() - 1
                } else {
                    idx - 1
                }
            })
            .unwrap_or(0);
        self.active_search_match = Some(prev);
        self.apply_active_search_match();
        self.needs_render = true;
    }

    /// Reemplaza el match activo y salta al siguiente.
    pub fn replace_current_match(&mut self) {
        self.clear_replace_all_preview();
        let Some(idx) = self.active_search_match else {
            self.status_text = "replace: no active match".to_string();
            self.needs_render = true;
            return;
        };
        let Some(matched) = self.search_matches.get(idx).cloned() else {
            self.status_text = "replace: no active match".to_string();
            self.needs_render = true;
            return;
        };

        let source_text = self.active_editor().text();
        let compiled_regex = if self.search_regex_mode {
            match self.build_search_regex() {
                Ok(regex) => Some(regex),
                Err(err) => {
                    self.status_text = format!("replace pattern error: {}", err);
                    self.needs_render = true;
                    return;
                }
            }
        } else {
            None
        };
        let replacement = match self.replacement_for_match_in_text(
            &source_text,
            &matched,
            compiled_regex.as_ref(),
        ) {
            Ok(value) => value,
            Err(err) => {
                self.status_text = format!("replace failed: {}", err);
                self.needs_render = true;
                return;
            }
        };
        let removed_bytes = matched.end_offset.saturating_sub(matched.start_offset);
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.active_editor_mut().set_selection_line_columns(
            matched.start_line,
            matched.start_col,
            matched.end_line,
            matched.end_col,
        );
        if removed_bytes > 0 {
            self.active_editor_mut().delete_forward();
        }
        if !replacement.is_empty() {
            self.active_editor_mut().insert(&replacement);
        }
        let delta_bytes = replacement.len() as isize - removed_bytes as isize;
        self.apply_replace_scope_delta(delta_bytes);

        let cursor_offset = self.active_editor().cursor().offset;
        self.rebuild_search_matches();
        if !self.search_matches.is_empty() {
            let next_idx = self
                .search_matches
                .iter()
                .position(|m| m.start_offset >= cursor_offset)
                .unwrap_or(0);
            self.active_search_match = Some(next_idx);
            self.apply_active_search_match();
        } else {
            self.status_text = "replace: done (no matches)".to_string();
        }
        self.needs_render = true;
    }

    /// Reemplaza todas las coincidencias.
    pub fn replace_all_matches(&mut self) {
        self.clear_replace_all_preview();
        if self.search_matches.is_empty() {
            self.status_text = "replace all: no matches".to_string();
            self.needs_render = true;
            return;
        }

        let matches = self.search_matches.clone();
        let total = matches.len();
        let source_text = self.active_editor().text();
        let compiled_regex = if self.search_regex_mode {
            match self.build_search_regex() {
                Ok(regex) => Some(regex),
                Err(err) => {
                    self.status_text = format!("replace pattern error: {}", err);
                    self.needs_render = true;
                    return;
                }
            }
        } else {
            None
        };
        let mut replacements: Vec<String> = Vec::with_capacity(matches.len());
        for matched in &matches {
            match self.replacement_for_match_in_text(&source_text, matched, compiled_regex.as_ref())
            {
                Ok(value) => replacements.push(value),
                Err(err) => {
                    self.status_text = format!("replace failed: {}", err);
                    self.needs_render = true;
                    return;
                }
            }
        }
        let mut total_delta_bytes: isize = 0;
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;

        for (matched, replacement) in matches
            .into_iter()
            .rev()
            .zip(replacements.into_iter().rev())
        {
            let removed_bytes = matched.end_offset.saturating_sub(matched.start_offset);
            self.active_editor_mut().set_selection_line_columns(
                matched.start_line,
                matched.start_col,
                matched.end_line,
                matched.end_col,
            );
            if removed_bytes > 0 {
                self.active_editor_mut().delete_forward();
            }
            if !replacement.is_empty() {
                self.active_editor_mut().insert(&replacement);
            }
            total_delta_bytes += replacement.len() as isize - removed_bytes as isize;
        }

        self.apply_replace_scope_delta(total_delta_bytes);
        self.rebuild_search_matches();
        self.status_text = format!("replace all: {}", total);
        self.needs_render = true;
    }

    /// Maneja teclas cuando búsqueda incremental está activa.
    pub fn handle_search_key(&mut self, key: &Key, shift: bool, ctrl: bool) -> bool {
        if !self.search_active {
            return false;
        }

        if ctrl {
            if let Key::Character(ch) = key {
                let lower = ch.to_lowercase();
                if lower == "f" {
                    self.close_search_mode();
                    return true;
                }
                if lower == "h" {
                    self.replace_active = !self.replace_active;
                    self.search_input_focus = if self.replace_active {
                        SearchInputFocus::Replace
                    } else {
                        self.clear_replace_all_preview();
                        self.replace_in_selection_only = false;
                        SearchInputFocus::Find
                    };
                    self.status_text = if self.replace_active {
                        "replace mode on".to_string()
                    } else {
                        "replace mode off".to_string()
                    };
                    self.rebuild_search_matches();
                    return true;
                }
                if lower == "m" {
                    self.search_match_case = !self.search_match_case;
                    self.rebuild_search_matches();
                    return true;
                }
                if lower == "w" {
                    self.search_whole_word = !self.search_whole_word;
                    self.rebuild_search_matches();
                    return true;
                }
                if lower == "g" {
                    self.search_regex_mode = !self.search_regex_mode;
                    self.rebuild_search_matches();
                    return true;
                }
                if lower == "l" {
                    self.toggle_replace_in_selection_scope();
                    return true;
                }
                if lower == "r" {
                    if self.replace_active {
                        if shift {
                            if self.replace_all_confirm_pending {
                                self.replace_all_matches();
                            } else if self.search_matches.is_empty() {
                                self.status_text = "replace all: no matches".to_string();
                                self.needs_render = true;
                            } else {
                                self.replace_all_confirm_pending = true;
                                self.status_text = format!(
                                    "replace preview: {} matches | Ctrl+Shift+R apply | Esc cancel",
                                    self.search_matches.len()
                                );
                                self.needs_render = true;
                            }
                        } else {
                            self.replace_current_match();
                        }
                    } else {
                        self.status_text = "replace mode is off (Ctrl+H)".to_string();
                        self.needs_render = true;
                    }
                    return true;
                }
                if lower == "v" {
                    if let Some(text) = self.read_clipboard_text() {
                        let input = self.active_search_input_mut();
                        input.push_str(&text);
                        if self.search_input_focus == SearchInputFocus::Find {
                            self.rebuild_search_matches();
                        } else {
                            self.clear_replace_all_preview();
                            self.status_text = format!(
                                "replace text: {} chars",
                                self.replace_query.chars().count()
                            );
                            self.needs_render = true;
                        }
                        return true;
                    }
                    return false;
                }
            }
            return false;
        }

        match key {
            Key::Named(NamedKey::Escape) => {
                if self.replace_all_confirm_pending {
                    self.clear_replace_all_preview();
                    self.status_text = "replace preview canceled".to_string();
                    self.needs_render = true;
                } else {
                    self.close_search_mode();
                }
                true
            }
            Key::Named(NamedKey::Tab) if self.replace_active => {
                self.clear_replace_all_preview();
                self.toggle_search_input_focus();
                true
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Tab) => {
                if shift {
                    self.search_previous();
                } else {
                    self.search_next();
                }
                true
            }
            Key::Named(NamedKey::Backspace) => {
                let input = self.active_search_input_mut();
                input.pop();
                if self.search_input_focus == SearchInputFocus::Find {
                    self.rebuild_search_matches();
                } else {
                    self.clear_replace_all_preview();
                    self.status_text =
                        format!("replace text: {} chars", self.replace_query.chars().count());
                    self.needs_render = true;
                }
                true
            }
            Key::Named(NamedKey::Space) => {
                let input = self.active_search_input_mut();
                input.push(' ');
                if self.search_input_focus == SearchInputFocus::Find {
                    self.rebuild_search_matches();
                } else {
                    self.clear_replace_all_preview();
                    self.status_text =
                        format!("replace text: {} chars", self.replace_query.chars().count());
                    self.needs_render = true;
                }
                true
            }
            Key::Character(c) => {
                let input = self.active_search_input_mut();
                input.push_str(c);
                if self.search_input_focus == SearchInputFocus::Find {
                    self.rebuild_search_matches();
                } else {
                    self.clear_replace_all_preview();
                    self.status_text =
                        format!("replace text: {} chars", self.replace_query.chars().count());
                    self.needs_render = true;
                }
                true
            }
            _ => false,
        }
    }

    /// Copia la selección activa del editor al clipboard.
    pub fn copy_selection_to_clipboard(&mut self) -> bool {
        let Some(selected) = self.selected_text_owned() else {
            self.status_text = "no selection to copy".to_string();
            self.needs_render = true;
            return false;
        };
        if selected.is_empty() {
            self.status_text = "no selection to copy".to_string();
            self.needs_render = true;
            return false;
        }

        match self.write_clipboard_text(&selected) {
            Ok(_) => {
                self.status_text = format!("copied {} chars", selected.chars().count());
                self.needs_render = true;
                true
            }
            Err(err) => {
                self.status_text = format!(
                    "clipboard unavailable (cached {} chars): {}",
                    selected.chars().count(),
                    err
                );
                self.needs_render = true;
                true
            }
        }
    }

    /// Corta selección activa del editor al clipboard.
    pub fn cut_selection_to_clipboard(&mut self) -> bool {
        let Some(selected) = self.selected_text_owned() else {
            self.status_text = "no selection to cut".to_string();
            self.needs_render = true;
            return false;
        };
        if selected.is_empty() {
            self.status_text = "no selection to cut".to_string();
            self.needs_render = true;
            return false;
        }

        let _ = self.write_clipboard_text(&selected);
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.active_editor_mut().delete_forward();
        self.refresh_search_matches_if_active();
        self.status_text = format!("cut {} chars", selected.chars().count());
        self.needs_render = true;
        true
    }

    /// Pega texto de clipboard en editor activo.
    pub fn paste_into_editor(&mut self) -> bool {
        let Some(text) = self.read_clipboard_text() else {
            self.status_text = "clipboard empty".to_string();
            self.needs_render = true;
            return false;
        };
        if text.is_empty() {
            self.status_text = "clipboard empty".to_string();
            self.needs_render = true;
            return false;
        }

        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.active_editor_mut().insert(&text);
        self.refresh_search_matches_if_active();
        self.status_text = format!("pasted {} chars", text.chars().count());
        self.needs_render = true;
        true
    }

    /// Pega texto de clipboard en el input del chat.
    pub fn paste_into_chat_input(&mut self) -> bool {
        let Some(text) = self.read_clipboard_text() else {
            self.status_text = "clipboard empty".to_string();
            self.needs_render = true;
            return false;
        };
        if text.is_empty() {
            self.status_text = "clipboard empty".to_string();
            self.needs_render = true;
            return false;
        }

        self.input_text.push_str(&text);
        self.status_text = format!("chat pasted {} chars", text.chars().count());
        self.needs_render = true;
        true
    }

    /// Selecciona todo el buffer activo.
    pub fn select_all_in_editor(&mut self) {
        self.active_editor_mut().select_all();
        let selected_len = self.active_editor().selection().len();
        self.status_text = format!("selected {} chars", selected_len);
        self.needs_render = true;
    }

    /// Undo en editor activo.
    pub fn undo_in_editor(&mut self) -> bool {
        if self.active_editor_mut().undo() {
            self.refresh_search_matches_if_active();
            self.status_text = "undo".to_string();
            self.needs_render = true;
            true
        } else {
            self.status_text = "nothing to undo".to_string();
            self.needs_render = true;
            false
        }
    }

    /// Redo en editor activo.
    pub fn redo_in_editor(&mut self) -> bool {
        if self.active_editor_mut().redo() {
            self.refresh_search_matches_if_active();
            self.status_text = "redo".to_string();
            self.needs_render = true;
            true
        } else {
            self.status_text = "nothing to redo".to_string();
            self.needs_render = true;
            false
        }
    }

    /// Mueve cursor al inicio de la palabra previa.
    pub fn move_word_left_in_editor(&mut self) {
        self.active_editor_mut().move_word_left();
        let cursor = *self.active_editor().cursor();
        self.status_text = format!("cursor Ln {}, Col {}", cursor.line + 1, cursor.column + 1);
        self.needs_render = true;
    }

    /// Mueve cursor al inicio de la siguiente palabra.
    pub fn move_word_right_in_editor(&mut self) {
        self.active_editor_mut().move_word_right();
        let cursor = *self.active_editor().cursor();
        self.status_text = format!("cursor Ln {}, Col {}", cursor.line + 1, cursor.column + 1);
        self.needs_render = true;
    }

    /// Extiende selección a palabra previa.
    pub fn extend_selection_word_left_in_editor(&mut self) {
        self.active_editor_mut().extend_selection_word_left();
        let selected_len = self.active_editor().selection().len();
        self.status_text = format!("selection {} chars", selected_len);
        self.needs_render = true;
    }

    /// Extiende selección a palabra siguiente.
    pub fn extend_selection_word_right_in_editor(&mut self) {
        self.active_editor_mut().extend_selection_word_right();
        let selected_len = self.active_editor().selection().len();
        self.status_text = format!("selection {} chars", selected_len);
        self.needs_render = true;
    }

    /// Borra palabra previa al cursor.
    pub fn delete_word_backward_in_editor(&mut self) {
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.active_editor_mut().delete_word_backward();
        self.refresh_search_matches_if_active();
        self.status_text = "delete word backward".to_string();
        self.needs_render = true;
    }

    /// Borra palabra siguiente al cursor.
    pub fn delete_word_forward_in_editor(&mut self) {
        self.pending_close_tab_confirm = None;
        self.pending_exit_confirm = false;
        self.active_editor_mut().delete_word_forward();
        self.refresh_search_matches_if_active();
        self.status_text = "delete word forward".to_string();
        self.needs_render = true;
    }

    /// Guarda el archivo activo en disco.
    pub fn save_active_file(&mut self) {
        let focused_index = self.focused_tab_index();
        let path = if let Some(path) = self.active_file_path().cloned() {
            path
        } else {
            // Save-as implícito para buffers sin ruta: untitled-N.md en workspace.
            let mut candidate = None;
            for i in 1..=9999u32 {
                let p = self.workspace_root.join(format!("untitled-{}.md", i));
                if !p.exists() {
                    candidate = Some(p);
                    break;
                }
            }
            let Some(new_path) = candidate else {
                self.status_text = "save failed: could not allocate untitled file".to_string();
                self.needs_render = true;
                return;
            };
            if let Some(tab) = self.open_tabs.get_mut(focused_index) {
                tab.path = Some(new_path.clone());
            }
            new_path
        };

        let content = self.active_editor().text();
        match fs::write(&path, content) {
            Ok(_) => {
                self.active_editor_mut().mark_saved();
                self.status_text = format!("saved {}", path.display());
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.mark_workspace_mutated();
                self.refresh_explorer();
                self.persist_session_snapshot();
                self.needs_render = true;
            }
            Err(err) => {
                self.status_text = format!("save failed: {}", err);
                self.needs_render = true;
            }
        }
    }

    /// Guarda pestaña activa con picker nativo (Save As).
    pub fn save_active_file_as(&mut self) {
        let focused_index = self.focused_tab_index();
        let default_name = self
            .active_file_path()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "untitled.md".to_string());

        let start_dir2 = self
            .active_file_path()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| self.workspace_root.clone());
        let default_name2 = default_name.clone();
        let result: Option<std::path::PathBuf> = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?
                .block_on(async {
                    AsyncFileDialog::new()
                        .set_directory(&start_dir2)
                        .set_file_name(&default_name2)
                        .save_file()
                        .await
                        .map(|h: FileHandle| h.path().to_path_buf())
                })
        })
        .join()
        .ok()
        .flatten();

        let Some(path) = result else {
            self.status_text = "save as canceled".to_string();
            self.needs_render = true;
            return;
        };

        let content = self.active_editor().text();
        match fs::write(&path, content) {
            Ok(_) => {
                if let Some(tab) = self.open_tabs.get_mut(focused_index) {
                    tab.path = Some(path.clone());
                    tab.editor.mark_saved();
                }
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.status_text = format!("saved as {}", path.display());
                self.mark_workspace_mutated();
                self.refresh_explorer();
                self.persist_session_snapshot();
                self.needs_render = true;
            }
            Err(err) => {
                self.status_text = format!("save as failed: {}", err);
                self.needs_render = true;
            }
        }
    }

    fn tab_hitbox_at(&self, x: f32, y: f32) -> Option<TabHitbox> {
        self.tab_hitboxes
            .iter()
            .rev()
            .find(|hit| hit.bounds.contains(x, y))
            .copied()
    }

    fn remap_tab_index_after_move(index: usize, from: usize, to: usize) -> usize {
        if index == from {
            return to;
        }
        if from < to && index > from && index <= to {
            return index - 1;
        }
        if from > to && index >= to && index < from {
            return index + 1;
        }
        index
    }

    fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.open_tabs.len() || to >= self.open_tabs.len() {
            return false;
        }

        let tab = self.open_tabs.remove(from);
        self.open_tabs.insert(to, tab);

        self.active_tab = Self::remap_tab_index_after_move(self.active_tab, from, to);
        self.secondary_tab = self
            .secondary_tab
            .map(|idx| Self::remap_tab_index_after_move(idx, from, to));
        self.pending_close_tab_confirm = self
            .pending_close_tab_confirm
            .map(|idx| Self::remap_tab_index_after_move(idx, from, to));
        self.normalize_editor_panes();
        self.persist_session_snapshot();
        self.needs_render = true;
        true
    }

    pub fn begin_tab_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(hit) = self.tab_hitbox_at(x, y) else {
            return false;
        };
        self.switch_tab_in_pane(hit.pane, hit.index);
        self.tab_drag_state = Some(TabDragState {
            pane: hit.pane,
            index: hit.index,
            press_x: x,
            press_y: y,
            moved: false,
        });
        true
    }

    pub fn is_tab_drag_active(&self) -> bool {
        self.tab_drag_state.is_some()
    }

    pub fn update_tab_drag_from_point(&mut self, x: f32, y: f32) -> bool {
        let Some(mut drag) = self.tab_drag_state else {
            return false;
        };

        if !drag.moved && (x - drag.press_x).abs() < 4.0 && (y - drag.press_y).abs() < 4.0 {
            return false;
        }
        drag.moved = true;

        let target = self
            .tab_hitbox_at(x, y)
            .filter(|hit| hit.pane == drag.pane)
            .map(|hit| hit.index);

        if let Some(target_index) = target {
            if target_index != drag.index && self.move_tab(drag.index, target_index) {
                drag.index = target_index;
                self.status_text = format!("tab moved to {}", target_index + 1);
                self.tab_drag_state = Some(drag);
                return true;
            }
        }

        self.tab_drag_state = Some(drag);
        false
    }

    pub fn end_tab_drag(&mut self) {
        self.tab_drag_state = None;
    }

    fn action_at(&self, x: f32, y: f32) -> Option<ClickTargetAction> {
        self.click_targets
            .iter()
            .rev()
            .find(|target| target.bounds.contains(x, y))
            .map(|target| target.action.clone())
    }

    /// Maneja keypress cuando el foco está en el input de chat.
    pub fn handle_chat_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::Backspace) => {
                self.input_text.pop();
                true
            }
            Key::Named(NamedKey::Enter) => {
                let msg = self.input_text.clone();
                self.input_text.clear();
                self.send_message(&msg);
                true
            }
            Key::Named(NamedKey::Escape) => {
                self.set_focus(match self.focused_editor_pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                true
            }
            Key::Named(NamedKey::Space) => {
                self.input_text.push(' ');
                true
            }
            Key::Character(c) => {
                self.input_text.push_str(c);
                true
            }
            _ => false,
        }
    }

    /// Maneja keypress cuando el foco está en el editor.
    pub fn handle_editor_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::ArrowLeft) => {
                self.active_editor_mut().move_left();
                true
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.active_editor_mut().move_right();
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.active_editor_mut().move_up();
                true
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.active_editor_mut().move_down();
                true
            }
            Key::Named(NamedKey::Home) => {
                self.active_editor_mut().move_to_line_start();
                true
            }
            Key::Named(NamedKey::End) => {
                self.active_editor_mut().move_to_line_end();
                true
            }
            Key::Named(NamedKey::Backspace) => {
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.active_editor_mut().backspace();
                self.refresh_search_matches_if_active();
                self.notify_quiron_edit("backspace");
                true
            }
            Key::Named(NamedKey::Delete) => {
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.active_editor_mut().delete_forward();
                self.refresh_search_matches_if_active();
                self.notify_quiron_edit("delete");
                true
            }
            Key::Named(NamedKey::Enter) => {
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.active_editor_mut().insert_newline();
                self.refresh_search_matches_if_active();
                self.notify_quiron_edit("newline");
                true
            }
            Key::Named(NamedKey::Space) => {
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.active_editor_mut().insert(" ");
                self.refresh_search_matches_if_active();
                self.notify_quiron_edit("space");
                true
            }
            Key::Named(NamedKey::Escape) => {
                self.set_focus(FocusTarget::ChatInput);
                true
            }
            Key::Character(c) => {
                self.pending_close_tab_confirm = None;
                self.pending_exit_confirm = false;
                self.active_editor_mut().insert(c);
                self.refresh_search_matches_if_active();
                let desc = format!("insert '{}'", c);
                self.notify_quiron_edit(&desc);
                true
            }
            _ => false,
        }
    }

    /// Notifica una edición de código a Quirón de forma asíncrona.
    fn notify_quiron_edit(&self, desc: &str) {
        if let Some(path) = self.active_editor().file_path() {
            let path_str = path.to_string_lossy().into_owned();
            let desc_str = desc.to_string();
            let quiron = self.quiron.clone();

            // Tarea de fondo fire-and-forget
            self.runtime.spawn(async move {
                let q = quiron.lock().await;
                let _ = q.observe_code_edit(&path_str, &desc_str).await;
            });
        }
    }

    /// Maneja keypress en editor considerando modificadores (Shift para selección).
    pub fn handle_editor_key_with_modifiers(&mut self, key: &Key, shift: bool) -> bool {
        if shift {
            match key {
                Key::Named(NamedKey::ArrowLeft) => {
                    self.active_editor_mut().extend_selection_left();
                    true
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.active_editor_mut().extend_selection_right();
                    true
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.active_editor_mut().extend_selection_up();
                    true
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.active_editor_mut().extend_selection_down();
                    true
                }
                Key::Named(NamedKey::Home) => {
                    self.active_editor_mut().extend_selection_to_line_start();
                    true
                }
                Key::Named(NamedKey::End) => {
                    self.active_editor_mut().extend_selection_to_line_end();
                    true
                }
                _ => self.handle_editor_key(key),
            }
        } else {
            self.handle_editor_key(key)
        }
    }

    /// Ejecuta la acción asociada a un click.
    pub fn handle_click(&mut self, x: f32, y: f32) {
        let Some(action) = self.action_at(x, y) else {
            if self.is_overlay_active() {
                self.close_overlay();
                return;
            }
            if self.top_menu_open.is_some() {
                self.close_top_menu();
                return;
            }
            let _ = self.place_cursor_from_point(x, y);
            return;
        };

        if self.top_menu_open.is_some()
            && !matches!(
                action,
                ClickTargetAction::TopMenuToggle(_) | ClickTargetAction::TopMenuExecute(_)
            )
        {
            self.close_top_menu();
        }

        match action {
            ClickTargetAction::Citation(citation) => {
                let event_id = citation.event_id.trim().to_string();
                if event_id.is_empty() || event_id == "?" {
                    self.status_text = "citation without event_id".to_string();
                    self.messages.push(ChatMessage {
                        is_user: false,
                        content: format!(
                            "📎 Citation sin event_id\n{}\nRelevancia: {}",
                            citation.snippet, citation.relevance
                        ),
                        meta: Some("citation_invalid".to_string()),
                        citations: vec![citation],
                    });
                    self.needs_render = true;
                    return;
                }

                self.status_text = format!("loading citation [{}]...", event_id);

                let quiron = self.quiron.clone();
                let event_id_for_lookup = event_id.clone();
                let lookup = self.runtime.block_on(async move {
                    let q = quiron.lock().await;
                    q.get_event(&event_id_for_lookup).await
                });

                match lookup {
                    Ok(Some(event)) => {
                        let short_id = event.id.chars().take(12).collect::<String>();
                        self.status_text = format!("citation [{}] resolved", short_id);
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!(
                                "📎 Evidence Event\nid: {}\nkind: {}\ndescription: {}\n\nsnippet: {}\nrelevancia: {}",
                                event.id,
                                event.kind,
                                event.description,
                                citation.snippet,
                                citation.relevance
                            ),
                            meta: Some("citation_resolved".to_string()),
                            citations: vec![citation],
                        });
                    }
                    Ok(None) => {
                        self.status_text = "citation event not found".to_string();
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!(
                                "📎 Citation event no encontrado: {}\nsnippet: {}\nrelevancia: {}",
                                event_id, citation.snippet, citation.relevance
                            ),
                            meta: Some("citation_not_found".to_string()),
                            citations: vec![citation],
                        });
                    }
                    Err(err) => {
                        self.status_text = "citation lookup failed".to_string();
                        self.messages.push(ChatMessage {
                            is_user: false,
                            content: format!(
                                "📎 Error consultando citation {}\nerror: {}\nsnippet: {}\nrelevancia: {}",
                                event_id, err, citation.snippet, citation.relevance
                            ),
                            meta: Some("citation_lookup_error".to_string()),
                            citations: vec![citation],
                        });
                    }
                }

                self.needs_render = true;
            }
            ClickTargetAction::ExplorerFile(path) => {
                self.explorer_selected_path = Some(path.clone());
                self.open_file_from_explorer(path);
            }
            ClickTargetAction::ExplorerDir(path) => {
                self.explorer_selected_path = Some(path.clone());
                self.toggle_dir_expanded(path);
            }
            ClickTargetAction::BreadcrumbSegment {
                pane,
                path,
                is_file,
            } => {
                self.set_sidebar_panel(SidebarPanel::Explorer);
                self.set_focus(match pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                if is_file {
                    self.open_file_in_pane(path, pane, false);
                } else if self.reveal_path_in_explorer(&path) {
                    self.status_text = format!("breadcrumb {}", path.display());
                    self.needs_render = true;
                }
            }
            ClickTargetAction::TabSelect { pane, index } => {
                self.switch_tab_in_pane(pane, index);
            }
            ClickTargetAction::SearchMatchSelect(index) => {
                if !self.search_active {
                    return;
                }
                if index >= self.search_matches.len() {
                    return;
                }
                self.active_search_match = Some(index);
                self.apply_active_search_match();
                self.set_focus(match self.focused_editor_pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                self.needs_render = true;
            }
            ClickTargetAction::SidebarProblemSelect(index) => {
                self.set_sidebar_panel(SidebarPanel::Problems);
                let _ = self.activate_problem_index(index);
            }
            ClickTargetAction::OverlayItemSelect(index) => {
                if !self.is_overlay_active() {
                    return;
                }
                if index >= self.overlay_items.len() {
                    return;
                }
                self.overlay_selected = index;
                self.execute_overlay_selection();
            }
            ClickTargetAction::ActivityFocusExplorer => {
                self.set_sidebar_panel(SidebarPanel::Explorer);
                self.set_focus(FocusTarget::EditorPrimary);
            }
            ClickTargetAction::NewChat => self.new_chat(),
            ClickTargetAction::ToggleThought(indice) => {
                if !self.expanded_thoughts.remove(&indice) {
                    self.expanded_thoughts.insert(indice);
                }
                self.needs_render = true;
            }
            ClickTargetAction::ActivityQuickOpen => {
                if self.overlay_mode == Some(OverlayMode::QuickOpen) {
                    self.close_overlay();
                } else {
                    self.begin_quick_open();
                }
            }
            ClickTargetAction::WelcomeOpenFolder => {
                self.open_folder_picker();
            }
            ClickTargetAction::WelcomeOpenRecent(path) => {
                if path.is_dir() {
                    self.open_workspace(path);
                } else {
                    // La carpeta desapareció desde la última sesión.
                    self.recent_projects.retain(|entry| entry != &path);
                    self.status_text = format!("no existe: {}", path.display());
                    self.needs_render = true;
                }
            }
            ClickTargetAction::ActivityCommandPalette => {
                if self.overlay_mode == Some(OverlayMode::CommandPalette) {
                    self.close_overlay();
                } else {
                    self.begin_command_palette();
                }
            }
            ClickTargetAction::ActivityToggleTelemetry => {
                self.toggle_telemetry_panel();
            }
            ClickTargetAction::ActivityCycleTheme => {
                self.cycle_ui_theme_next();
            }
            ClickTargetAction::ActivityCycleDensity => {
                self.cycle_ui_density_next();
            }
            ClickTargetAction::ActivitySetTheme(theme) => {
                self.set_ui_theme(theme);
            }
            ClickTargetAction::ActivitySetDensity(density) => {
                self.set_ui_density(density);
            }
            ClickTargetAction::ActivityApplyAppearancePreset(preset) => {
                self.apply_ui_appearance_preset(preset);
            }
            ClickTargetAction::ActivityIncreaseEditorFontScale => {
                self.increase_editor_font_scale();
            }
            ClickTargetAction::ActivityDecreaseEditorFontScale => {
                self.decrease_editor_font_scale();
            }
            ClickTargetAction::ActivityResetEditorFontScale => {
                self.reset_editor_font_scale();
            }
            ClickTargetAction::ActivityResetAppearance => {
                self.reset_ui_appearance();
            }
            ClickTargetAction::ActivityIncreaseEditorHorizontalPadding => {
                self.increase_editor_horizontal_padding();
            }
            ClickTargetAction::ActivityDecreaseEditorHorizontalPadding => {
                self.decrease_editor_horizontal_padding();
            }
            ClickTargetAction::ActivityResetEditorHorizontalPadding => {
                self.reset_editor_horizontal_padding();
            }
            ClickTargetAction::ActivityIncreaseExplorerIndent => {
                self.increase_explorer_indent_step();
            }
            ClickTargetAction::ActivityDecreaseExplorerIndent => {
                self.decrease_explorer_indent_step();
            }
            ClickTargetAction::ActivityResetExplorerIndent => {
                self.reset_explorer_indent_step();
            }
            ClickTargetAction::ActivityExplorerNewFile => {
                self.explorer_create_new_file();
            }
            ClickTargetAction::ActivityExplorerNewFolder => {
                self.explorer_create_new_folder();
            }
            ClickTargetAction::ActivityRefreshExplorer => {
                self.refresh_explorer();
                self.invalidate_workspace_cache();
                self.status_text = "explorer refreshed".to_string();
                self.needs_render = true;
            }
            ClickTargetAction::ActivityRevealActiveFile => {
                if self.reveal_active_file_in_explorer() {
                    self.status_text = "explorer revealed active file".to_string();
                    self.needs_render = true;
                }
            }
            ClickTargetAction::ActivityAddSelectionToChat => {
                self.add_selection_to_chat_input();
            }
            ClickTargetAction::ActivityCycleAiModel => {
                self.cycle_ai_model();
            }
            ClickTargetAction::TopMenuToggle(menu) => {
                self.toggle_top_menu(menu);
            }
            ClickTargetAction::TopMenuExecute(command) => {
                self.close_top_menu();
                self.execute_command_palette_action(command);
            }
            ClickTargetAction::ActivityFocusChat => {
                self.set_focus(FocusTarget::ChatInput);
            }
            ClickTargetAction::ActivitySidebarSearch => {
                self.set_sidebar_panel(SidebarPanel::Search);
            }
            ClickTargetAction::ActivitySidebarGit => {
                self.set_sidebar_panel(SidebarPanel::Git);
            }
            ClickTargetAction::ActivitySidebarProblems => {
                self.set_sidebar_panel(SidebarPanel::Problems);
            }
            ClickTargetAction::ActivitySidebarOutline => {
                self.set_sidebar_panel(SidebarPanel::Outline);
            }
            ClickTargetAction::ActivitySidebarAppearance => {
                self.set_sidebar_panel(SidebarPanel::Appearance);
            }
            ClickTargetAction::ActivitySidebarSecurity => {
                self.set_sidebar_panel(SidebarPanel::Security);
            }
            ClickTargetAction::OutlineSelect { pane, line, column } => {
                self.set_sidebar_panel(SidebarPanel::Outline);
                self.set_focus(match pane {
                    EditorPane::Primary => FocusTarget::EditorPrimary,
                    EditorPane::Secondary => FocusTarget::EditorSecondary,
                });
                self.editor_for_pane_mut(pane)
                    .set_cursor_line_column(line, column);
                self.status_text = format!("outline L{}:{}", line + 1, column + 1);
                self.needs_render = true;
            }
        }
    }

    fn format_delegation_metrics_tag(metrics: &DelegationMetrics) -> String {
        let w_ewma = metrics
            .worker_tokens_ewma
            .map(|v| format!("{:.0}", v))
            .unwrap_or_else(|| "-".to_string());
        let p_ewma = metrics
            .primary_tokens_ewma
            .map(|v| format!("{:.0}", v))
            .unwrap_or_else(|| "-".to_string());
        format!(
            "budget={}/{} rem={} fb={:.2} par={}/{} w_ewma={} p_ewma={} min_w={} min_p={} scale_w={:.2} scale_p={:.2}",
            metrics.model_tokens_used,
            metrics.token_budget,
            metrics.token_budget_remaining,
            metrics.llm_fallback_rate,
            metrics.parallel_subtasks_current,
            metrics.parallel_subtasks_cap,
            w_ewma,
            p_ewma,
            metrics.worker_min_tokens_threshold,
            metrics.primary_min_tokens_threshold,
            metrics.worker_threshold_scale,
            metrics.primary_threshold_scale
        )
    }

    fn short_session_id(session_id: &str) -> String {
        if session_id.len() > 18 {
            session_id[session_id.len() - 18..].to_string()
        } else {
            session_id.to_string()
        }
    }

    fn truncate_for_report(text: &str, max_chars: usize) -> String {
        let mut out = String::new();
        for (idx, ch) in text.chars().enumerate() {
            if idx >= max_chars {
                out.push_str("...");
                return out;
            }
            out.push(ch);
        }
        out
    }

    fn format_checkpoint_report(
        persisted: Option<&SessionTelemetryCheckpointSummary>,
        local: Option<&llore_brain::SessionTelemetryCheckpoint>,
    ) -> String {
        if let Some(cp) = persisted {
            let ts_end = if cp.ts_end.trim().is_empty() {
                "-".to_string()
            } else {
                cp.ts_end.clone()
            };
            let flags = if cp.anomaly_flags.is_empty() {
                "none".to_string()
            } else {
                cp.anomaly_flags.join(",")
            };
            let preset = cp
                .preset_change
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("-");
            return format!(
                "seg={} steps={}..{} ts_end={} budget={}/{} fb={:.2} tok_total={} tok_delta={} flags={} preset={}",
                cp.segment_seq,
                cp.step_start,
                cp.step_end,
                ts_end,
                cp.token_budget_remaining,
                cp.token_budget,
                cp.llm_fallback_rate,
                cp.model_tokens_used_total,
                cp.model_tokens_used_delta,
                flags,
                preset
            );
        }

        if let Some(cp) = local {
            let flags = if cp.anomaly_flags.is_empty() {
                "none".to_string()
            } else {
                cp.anomaly_flags.join(",")
            };
            let preset = cp
                .preset_change
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("-");
            return format!(
                "seg={} steps={}..{} ts_end={} budget={}/{} fb={:.2} tok_total={} tok_delta={} flags={} preset={}",
                cp.segment_seq,
                cp.step_start,
                cp.step_end,
                cp.ts_end,
                cp.token_budget_remaining,
                cp.token_budget,
                cp.llm_fallback_rate,
                cp.model_tokens_used_total,
                cp.model_tokens_used_delta,
                flags,
                preset
            );
        }

        "none".to_string()
    }

    fn format_anomaly_report(
        persisted: Option<&SessionTelemetryAnomalySummary>,
        local: Option<&llore_brain::SessionTelemetryAnomaly>,
    ) -> String {
        if let Some(anomaly) = persisted {
            let ts = if anomaly.timestamp.trim().is_empty() {
                "-".to_string()
            } else {
                anomaly.timestamp.clone()
            };
            let detail = if anomaly.detail.trim().is_empty() {
                "-".to_string()
            } else {
                Self::truncate_for_report(&anomaly.detail, 140)
            };
            return format!(
                "seg={} step={} iter={} kind={} ts={} detail={}",
                anomaly.segment_seq,
                anomaly.step,
                anomaly.request_iteration,
                anomaly.kind,
                ts,
                detail
            );
        }

        if let Some(anomaly) = local {
            return format!(
                "seg={} step={} iter={} kind={} ts={} detail={}",
                anomaly.segment_seq,
                anomaly.step,
                anomaly.request_iteration,
                format!("{:?}", anomaly.kind).to_lowercase(),
                anomaly.timestamp,
                Self::truncate_for_report(&anomaly.detail, 140)
            );
        }

        "none".to_string()
    }

    /// Envía un mensaje al modelo primario configurado.
    pub fn send_message(&mut self, content: &str) {
        if content.trim().is_empty() {
            return;
        }

        if self.try_handle_chat_control_command(content) {
            self.loading = false;
            return;
        }

        // Sin proyecto abierto no hay identidad de proyecto, y sin ella el
        // contexto recuperado no pertenece a ningún mundo.
        if !self.workspace_is_open() {
            self.messages.push(ChatMessage {
                is_user: false,
                content: "Abre una carpeta de proyecto antes de preguntar.".to_string(),
                meta: Some("system".to_string()),
                citations: vec![],
            });
            self.loading = false;
            self.needs_render = true;
            return;
        }

        // Añadir mensaje del usuario
        self.messages.push(ChatMessage {
            is_user: true,
            content: content.to_string(),
            meta: None,
            citations: vec![],
        });

        self.loading = true;
        self.status_text = "thinking...".to_string();

        // Clonar para el closure async
        let quiron = self.quiron.clone();
        let content_str = content.to_string();
        let context = self.build_chat_context();
        let system = self.chat_system_prompt();
        let workspace_root = self.workspace_root.clone();
        let show_noise = self.explorer_show_noise;

        // Con proyecto abierto, el chat lleva manos: el catálogo de
        // herramientas viaja con la pregunta y el modelo puede leer el
        // proyecto en vez de inventárselo. Las herramientas fijan el modelo
        // (ver nota en TOOLS_CHAT_MODEL), elija lo que elija el selector.
        let model = chat_tools::TOOLS_CHAT_MODEL.to_string();

        let empezo = Instant::now();
        let result = self.runtime.block_on(async {
            let q = quiron.lock().await;
            let tools = chat_tools::tool_catalog()
                .into_iter()
                .map(|tool| LlmToolDef {
                    name: tool.name.to_string(),
                    description: tool.description.to_string(),
                    input_schema: tool.input_schema,
                })
                .collect();
            q.chat_with_tools(
                &content_str,
                &model,
                &context,
                Some(&system),
                tools,
                |name, input| {
                    // La única puerta al disco: cada ejecución pasa por la
                    // guardia del workspace antes de tocar nada.
                    let ctx = chat_tools::ToolContext {
                        workspace_root: &workspace_root,
                        show_noise,
                    };
                    let outcome = chat_tools::execute(&ctx, name, input);
                    (outcome.content, outcome.is_error)
                },
            )
            .await
        });

        let pensado = empezo.elapsed();
        let (response_text, response_meta, tool_trace) = match result {
            Ok(outcome) => (outcome.text, None, outcome.tool_trace),
            Err(err) => (
                format!("No se pudo obtener respuesta: {}", err),
                Some("model-error".to_string()),
                Vec::new(),
            ),
        };

        // Las manos usadas se enseñan antes de la respuesta: qué se leyó y
        // qué negó el arnés son parte de la contestación. Va como bloque de
        // pensamiento: la primera línea es el encabezado —cuánto tardó y qué
        // tocó—, y el resto el detalle, que el render pliega por defecto.
        //
        // Solo se cuenta lo que es verdad. El razonamiento del modelo no viaja
        // en la respuesta y la consulta a la memoria vectorial no existe aún en
        // esta ruta: cuando existan, entran aquí como líneas más del detalle.
        if !tool_trace.is_empty() {
            let leidos = tool_trace
                .iter()
                .filter(|e| !e.is_error && e.summary.starts_with("read_file"))
                .count();
            let busquedas = tool_trace
                .iter()
                .filter(|e| !e.is_error && !e.summary.starts_with("read_file"))
                .count();
            let negadas = tool_trace.iter().filter(|e| e.is_error).count();
            let mut partes = Vec::new();
            if leidos > 0 {
                partes.push(format!(
                    "leyó {leidos} archivo{}",
                    if leidos == 1 { "" } else { "s" }
                ));
            }
            if busquedas > 0 {
                partes.push(format!(
                    "{busquedas} búsqueda{}",
                    if busquedas == 1 { "" } else { "s" }
                ));
            }
            if negadas > 0 {
                partes.push(format!(
                    "{negadas} negada{}",
                    if negadas == 1 { "" } else { "s" }
                ));
            }
            let encabezado = format!(
                "Pensó {:.1} s · {}",
                pensado.as_secs_f32(),
                partes.join(", ")
            );
            let detalle = tool_trace
                .iter()
                .map(|entry| {
                    let marca = if entry.is_error { "✗" } else { "✓" };
                    format!("{marca} {}", entry.summary)
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.messages.push(ChatMessage {
                is_user: false,
                content: format!("{encabezado}\n{detalle}"),
                meta: Some("tools".to_string()),
                citations: vec![],
            });
        }

        self.messages.push(ChatMessage {
            is_user: false,
            content: response_text,
            meta: response_meta.clone(),
            citations: vec![],
        });

        self.status_text = response_meta.unwrap_or_else(|| "response received".to_string());
        self.loading = false;
        self.needs_render = true;
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuración de la aplicación
pub struct AppConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Aplicación Llore
pub struct App<F>
where
    F: FnMut(&mut Window, &mut AppState) + 'static,
{
    config: AppConfig,
    render_fn: F,
    window: Option<Window>,
    state: AppState,
}

impl<F> App<F>
where
    F: FnMut(&mut Window, &mut AppState) + 'static,
{
    /// Crea una nueva aplicación
    pub fn new(title: impl Into<String>, render_fn: F) -> Self {
        Self {
            config: AppConfig {
                title: title.into(),
                width: 1280,
                height: 720,
            },
            render_fn,
            window: None,
            state: AppState::default(),
        }
    }

    /// Establece el tamaño inicial
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.config.width = width;
        self.config.height = height;
        self
    }

    /// Ejecuta la aplicación
    pub fn run(mut self) {
        let event_loop = EventLoop::<AppEvent>::with_user_event()
            .build()
            .expect("Failed to create event loop");
        self.state.event_proxy = Some(event_loop.create_proxy());
        event_loop.set_control_flow(ControlFlow::Wait);
        event_loop.run_app(&mut self).expect("Event loop failed");
    }
}

impl<F> ApplicationHandler<AppEvent> for App<F>
where
    F: FnMut(&mut Window, &mut AppState) + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.window = Some(Window::new(
                event_loop,
                &self.config.title,
                self.config.width,
                self.config.height,
            ));
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if self.state.request_app_exit() {
                    self.state.persist_session_snapshot();
                    self.state.persist_layout_snapshot();
                    event_loop.exit();
                } else if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            WindowEvent::Resized(size) => {
                if let Some(window) = &mut self.window {
                    window.resize(size.width, size.height);
                    self.state.needs_render = true;
                    window.request_redraw();
                }
            }

            // Manejo de teclado
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let ctrl = self.state.modifiers.control_key();
                    let shift = self.state.modifiers.shift_key();
                    let alt = self.state.modifiers.alt_key();
                    if self
                        .state
                        .handle_top_menu_key(&event.logical_key, shift, alt)
                    {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                    if self
                        .state
                        .handle_overlay_key(&event.logical_key, shift, ctrl, alt)
                    {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                    if self
                        .state
                        .handle_search_key(&event.logical_key, shift, ctrl)
                    {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                    if matches!(event.logical_key, Key::Named(NamedKey::F8)) {
                        let handled = if shift {
                            self.state.navigate_problem(-1)
                        } else {
                            self.state.navigate_problem(1)
                        };
                        if handled {
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                            return;
                        }
                    }
                    if alt {
                        let handled = match &event.logical_key {
                            Key::Named(NamedKey::ArrowDown) => {
                                self.state.move_explorer_selection(1)
                            }
                            Key::Named(NamedKey::ArrowUp) => self.state.move_explorer_selection(-1),
                            Key::Named(NamedKey::ArrowRight) => {
                                self.state.expand_explorer_selection()
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                self.state.collapse_explorer_selection()
                            }
                            Key::Named(NamedKey::Enter) => self.state.activate_explorer_selection(),
                            _ => false,
                        };
                        if handled {
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                            return;
                        }
                    }
                    if ctrl {
                        if matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
                            if shift {
                                self.state.cycle_tabs_previous();
                            } else {
                                self.state.cycle_tabs();
                            }
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                            return;
                        }
                        if self.state.editor_focused {
                            match &event.logical_key {
                                Key::Named(NamedKey::ArrowLeft) => {
                                    if shift {
                                        self.state.extend_selection_word_left_in_editor();
                                    } else {
                                        self.state.move_word_left_in_editor();
                                    }
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                    return;
                                }
                                Key::Named(NamedKey::ArrowRight) => {
                                    if shift {
                                        self.state.extend_selection_word_right_in_editor();
                                    } else {
                                        self.state.move_word_right_in_editor();
                                    }
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                    return;
                                }
                                Key::Named(NamedKey::Backspace) => {
                                    self.state.delete_word_backward_in_editor();
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                    return;
                                }
                                Key::Named(NamedKey::Delete) => {
                                    self.state.delete_word_forward_in_editor();
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                    return;
                                }
                                _ => {}
                            }
                        }
                        if let Key::Character(ch) = &event.logical_key {
                            let lower = ch.to_lowercase();
                            if lower == "a" && !alt && self.state.editor_focused {
                                self.state.select_all_in_editor();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "c" && self.state.editor_focused {
                                self.state.copy_selection_to_clipboard();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "x" && self.state.editor_focused {
                                self.state.cut_selection_to_clipboard();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "v" {
                                let handled = if self.state.editor_focused {
                                    self.state.paste_into_editor()
                                } else if self.state.input_focused {
                                    self.state.paste_into_chat_input()
                                } else {
                                    false
                                };
                                if handled {
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                    return;
                                }
                            }
                            if lower == "z" && self.state.editor_focused {
                                if shift {
                                    self.state.redo_in_editor();
                                } else {
                                    self.state.undo_in_editor();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "y" && self.state.editor_focused {
                                self.state.redo_in_editor();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if alt && (lower == "=" || lower == "+") {
                                self.state.increase_editor_font_scale();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if alt && lower == "-" {
                                self.state.decrease_editor_font_scale();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if alt && lower == "0" {
                                self.state.reset_editor_font_scale();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if alt && lower == "d" {
                                self.state.cycle_ui_density_next();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if alt && lower == "a" {
                                self.state.set_sidebar_panel(SidebarPanel::Appearance);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "0" {
                                self.state.reset_layout();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "e" {
                                if self.state.reveal_active_file_in_explorer() {
                                    self.state.status_text =
                                        "explorer revealed active file".to_string();
                                    self.state.needs_render = true;
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                }
                                return;
                            }
                            if lower == "q" {
                                if self.state.request_app_exit() {
                                    self.state.persist_session_snapshot();
                                    self.state.persist_layout_snapshot();
                                    event_loop.exit();
                                } else if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "f" {
                                if shift {
                                    self.state.begin_workspace_text_search_overlay();
                                } else {
                                    self.state.begin_search_mode();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "h" {
                                self.state.begin_replace_mode();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "i" && shift && self.state.editor_focused {
                                self.state.add_selection_to_chat_input();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "m" && shift {
                                self.state.begin_problems_overlay();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "o" {
                                if shift && alt {
                                    self.state.begin_workspace_symbol_overlay();
                                } else if shift {
                                    self.state.begin_symbol_overlay();
                                } else {
                                    self.state.open_file_picker();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "p" {
                                if shift {
                                    self.state.begin_command_palette();
                                } else {
                                    self.state.begin_quick_open();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "t" && shift && !alt {
                                self.state.toggle_telemetry_panel();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "t" && alt {
                                if shift {
                                    self.state.cycle_ui_theme_previous();
                                } else {
                                    self.state.cycle_ui_theme_next();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "g" && shift {
                                self.state.cycle_telemetry_timeline_filter();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "g" {
                                self.state.begin_go_to_line_overlay();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "u" && shift {
                                self.state.refresh_telemetry_persisted_cache();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "j" && shift {
                                self.state.load_more_telemetry_persisted_cache();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "s" {
                                if shift {
                                    self.state.save_active_file_as();
                                } else {
                                    self.state.save_active_file();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "r" {
                                self.state.refresh_explorer();
                                self.state.invalidate_workspace_cache();
                                self.state.status_text = "explorer refreshed".to_string();
                                self.state.needs_render = true;
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "w" {
                                self.state.request_close_active_tab();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if lower == "n" {
                                if shift && !alt {
                                    self.state.open_new_window();
                                } else if alt {
                                    self.state.explorer_create_new_file();
                                } else {
                                    self.state.open_new_tab();
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                        }
                    }

                    let changed = match &event.logical_key {
                        Key::Named(NamedKey::Tab) => {
                            self.state.toggle_focus();
                            true
                        }
                        key if self.state.editor_focused => {
                            self.state.handle_editor_key_with_modifiers(key, shift)
                        }
                        key if self.state.input_focused => self.state.handle_chat_key(key),
                        _ => false,
                    };

                    if changed {
                        self.state.needs_render = true;
                    }

                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.state.modifiers = modifiers.state();
            }

            WindowEvent::CursorMoved { position, .. } => {
                // El ratón llega en píxeles físicos; el layout y las zonas
                // sensibles viven en coordenadas lógicas. Sin esta división, al
                // agrandar la interfaz los clics caerían fuera de los botones.
                let factor = self.state.ui_scale.factor();
                let x = position.x as f32 / factor;
                let y = position.y as f32 / factor;
                self.state.cursor_pos = Some((x, y));
                if self.state.mouse_left_down
                    && self.state.is_layout_resizing()
                    && self.state.update_layout_resize_from_point(x, y)
                {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
                if self.state.mouse_left_down
                    && !self.state.is_overlay_active()
                    && !self.state.is_layout_resizing()
                    && self.state.update_tab_drag_from_point(x, y)
                {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
                if self.state.mouse_left_down && self.state.is_tab_drag_active() {
                    return;
                }
                if self.state.mouse_left_down
                    && !self.state.is_overlay_active()
                    && !self.state.is_layout_resizing()
                    && self.state.update_mouse_selection_from_point(x, y)
                    && self.state.needs_render
                {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    if state == ElementState::Pressed {
                        self.state.mouse_left_down = true;
                        if let Some((x, y)) = self.state.cursor_pos {
                            if self.state.begin_layout_resize(x, y) {
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            if self.state.is_overlay_active() {
                                self.state.handle_click(x, y);
                            } else {
                                self.state.update_focus_from_click(x, y);
                                if self.state.begin_tab_drag(x, y) {
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                    return;
                                }
                                // Intentar click targets PRIMERO (botones, tabs, etc.)
                                let has_click_target = self.state.action_at(x, y).is_some();
                                if has_click_target {
                                    self.state.handle_click(x, y);
                                } else {
                                    let handled_editor = self.state.handle_editor_mouse_press(x, y);
                                    if !handled_editor {
                                        self.state.handle_click(x, y);
                                    }
                                }
                            }
                            if self.state.needs_render {
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                        }
                    } else {
                        self.state.mouse_left_down = false;
                        self.state.end_tab_drag();
                        self.state.end_layout_resize();
                        self.state.end_mouse_selection();
                        if self.state.needs_render {
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.state.is_overlay_active() {
                    return;
                }
                if let Some((x, y)) = self.state.cursor_pos {
                    let delta_lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => {
                            if y > 0.0 {
                                -3
                            } else if y < 0.0 {
                                3
                            } else {
                                0
                            }
                        }
                        MouseScrollDelta::PixelDelta(pos) => {
                            if pos.y > 0.0 {
                                -2
                            } else if pos.y < 0.0 {
                                2
                            } else {
                                0
                            }
                        }
                    };
                    let in_sidebar = self
                        .state
                        .sidebar_bounds
                        .as_ref()
                        .map(|b| b.contains(x, y))
                        .unwrap_or(false);
                    let in_search_results = self
                        .state
                        .search_results_bounds
                        .as_ref()
                        .map(|b| b.contains(x, y))
                        .unwrap_or(false);
                    let in_chat = self
                        .state
                        .chat_bounds
                        .as_ref()
                        .map(|b| b.contains(x, y))
                        .unwrap_or(false);
                    let editor_pane = self.state.editor_pane_at_point(x, y);
                    if in_sidebar {
                        self.state.scroll_sidebar_lines(delta_lines);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    } else if in_search_results {
                        self.state.scroll_search_results_lines(delta_lines);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    } else if let Some(pane) = editor_pane {
                        self.state.scroll_editor_lines_in_pane(pane, delta_lines);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    } else if in_chat && self.state.is_telemetry_panel_enabled() {
                        self.state.scroll_telemetry_timeline_lines(delta_lines);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(window) = &mut self.window {
                    if self.state.needs_render {
                        // Limpiar canvas
                        window.canvas().clear(Color::BACKGROUND);

                        // Los targets de click se recalculan en cada frame.
                        self.state.clear_click_targets();

                        // Renderizar contenido
                        (self.render_fn)(window, &mut self.state);

                        // Presentar
                        window.present();

                        self.state.needs_render = false;
                    }
                }
            }

            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::FileOpened(path) => {
                self.state.open_file_from_explorer(path);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            AppEvent::FolderOpened(folder) => {
                self.state.open_workspace(folder);
                self.state.status_text = format!(
                    "workspace: {}",
                    self.state
                        .workspace_root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("?")
                );
                self.state.needs_render = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let mut should_redraw = false;
        if self.state.poll_workspace_changes() {
            should_redraw = true;
        }
        if self.state.poll_git_sidebar_status() {
            should_redraw = true;
        }
        if self.state.poll_telemetry_persisted_prefetch() {
            should_redraw = true;
        }
        if self.state.poll_quiron_health() {
            should_redraw = true;
        }
        if should_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

// Builder API simple para crear la app
pub fn run<F>(title: &str, width: u32, height: u32, render_fn: F)
where
    F: FnMut(&mut Window, &mut AppState) + 'static,
{
    App::new(title, render_fn).with_size(width, height).run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(tag: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "llore_ui_{}_{}_{}",
                tag,
                std::process::id(),
                stamp
            ));
            fs::create_dir_all(&root).expect("must create test workspace");
            fs::write(root.join("README.md"), "# test workspace\n")
                .expect("must seed README.md for startup tab");
            Self { root }
        }

        fn root_path(&self) -> PathBuf {
            self.root.clone()
        }

        fn create_file(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("must create parent directories");
            }
            fs::write(&path, content).expect("must write test file");
            path
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn overlay_page_navigation_wraps_by_page_size() {
        let workspace = TestWorkspace::new("overlay_page_nav");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.overlay_mode = Some(OverlayMode::QuickOpen);
        state.overlay_items = (0..OVERLAY_RESULTS_MAX)
            .map(|idx| OverlayItem {
                title: format!("item-{}", idx),
                detail: String::new(),
                action: OverlayAction::Command(CommandPaletteAction::QuickOpen),
            })
            .collect();

        state.overlay_selected = OVERLAY_RESULTS_MAX - 2;
        let page_down = Key::Named(NamedKey::PageDown);
        assert!(state.handle_overlay_key(&page_down, false, false, false));
        assert_eq!(state.overlay_selected, 3);

        let page_up = Key::Named(NamedKey::PageUp);
        assert!(state.handle_overlay_key(&page_up, false, false, false));
        assert_eq!(state.overlay_selected, OVERLAY_RESULTS_MAX - 2);

        state.overlay_selected = 0;
        assert!(state.handle_overlay_key(&page_up, false, false, false));
        assert_eq!(state.overlay_selected, OVERLAY_RESULTS_MAX / 2);
    }

    #[test]
    fn quick_open_alt_enter_opens_file_in_secondary_pane() {
        let workspace = TestWorkspace::new("quick_open_side");
        let side_file = workspace.create_file("src/side.rs", "fn side() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.overlay_mode = Some(OverlayMode::QuickOpen);
        state.overlay_items = vec![OverlayItem {
            title: "side.rs".to_string(),
            detail: "src/side.rs".to_string(),
            action: OverlayAction::OpenFile(side_file.clone()),
        }];
        state.overlay_selected = 0;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, true));
        assert!(state.overlay_mode.is_none());
        assert!(state.is_editor_split_active());
        assert_eq!(state.focused_editor_pane, EditorPane::Secondary);
        assert_eq!(
            state.file_path_for_pane(EditorPane::Secondary),
            Some(&side_file)
        );
    }

    #[test]
    fn open_to_side_duplicates_active_file_when_needed() {
        let workspace = TestWorkspace::new("open_to_side_duplicate");
        let file = workspace.create_file("alpha.rs", "fn alpha() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(file.clone());
        let before = state.open_tabs.len();

        state.open_file_to_side(file.clone());

        assert!(state.is_editor_split_active());
        assert_eq!(state.open_tabs.len(), before + 1);
        let duplicated = state
            .open_tabs
            .iter()
            .filter(|tab| tab.path.as_ref() == Some(&file))
            .count();
        assert_eq!(duplicated, 2);
        assert_eq!(state.file_path_for_pane(EditorPane::Secondary), Some(&file));
    }

    #[test]
    fn split_layout_restores_after_restart() {
        let workspace = TestWorkspace::new("split_restore");
        let side_file = workspace.create_file("beta.rs", "fn beta() {}\n");

        {
            let mut state = AppState::new_for_tests(workspace.root_path());
            state.set_sidebar_panel(SidebarPanel::Search);
            state.open_file_to_side(side_file.clone());
            assert!(state.is_editor_split_active());

            let layout_snapshot = fs::read_to_string(state.layout_snapshot_path().expect("el test abre un proyecto"))
                .expect("layout snapshot must exist after open_to_side");
            assert!(layout_snapshot.contains("editor_pane_split_enabled=1"));
            assert!(layout_snapshot.contains("sidebar_panel=search"));
        }

        let restored = AppState::new_for_tests(workspace.root_path());
        assert!(restored.is_editor_split_active());
        assert_eq!(restored.sidebar_panel, SidebarPanel::Search);

        let primary_path = restored.file_path_for_pane(EditorPane::Primary).cloned();
        let secondary_path = restored.file_path_for_pane(EditorPane::Secondary).cloned();
        assert!(
            primary_path.as_ref() == Some(&side_file)
                || secondary_path.as_ref() == Some(&side_file)
        );
    }

    #[test]
    fn layout_snapshot_restores_panel_docks_and_ai_model() {
        let workspace = TestWorkspace::new("layout_docks_model_restore");
        {
            let mut state = AppState::new_for_tests(workspace.root_path());
            state.selected_ai_model = "gpt-5.5".to_string();
            state.execute_command_palette_action(CommandPaletteAction::MoveExplorerRight);
            state.execute_command_palette_action(CommandPaletteAction::MoveChatLeft);
            state.execute_command_palette_action(CommandPaletteAction::CycleAiModel);

            assert_eq!(state.explorer_dock, PanelDock::Right);
            assert_eq!(state.chat_dock, PanelDock::Left);
            assert_eq!(state.selected_ai_model(), "gpt-5.4");
        }

        let restored = AppState::new_for_tests(workspace.root_path());
        assert_eq!(restored.explorer_dock, PanelDock::Right);
        assert_eq!(restored.chat_dock, PanelDock::Left);
        assert_eq!(restored.selected_ai_model(), "gpt-5.4");
    }

    #[test]
    fn layout_snapshot_restores_ui_theme() {
        let workspace = TestWorkspace::new("layout_theme_restore");
        {
            let mut state = AppState::new_for_tests(workspace.root_path());
            state.set_ui_theme(UiTheme::CopperLight);
            let layout_snapshot = fs::read_to_string(state.layout_snapshot_path().expect("el test abre un proyecto"))
                .expect("layout snapshot must exist");
            assert!(layout_snapshot.contains("ui_theme=copper_light"));
        }
        let restored = AppState::new_for_tests(workspace.root_path());
        assert_eq!(restored.ui_theme(), UiTheme::CopperLight);
    }

    #[test]
    fn layout_snapshot_restores_editor_font_scale() {
        let workspace = TestWorkspace::new("layout_font_scale_restore");
        {
            let mut state = AppState::new_for_tests(workspace.root_path());
            state.set_editor_font_scale(1.25);
            let layout_snapshot = fs::read_to_string(state.layout_snapshot_path().expect("el test abre un proyecto"))
                .expect("layout snapshot must exist");
            assert!(layout_snapshot.contains("editor_font_scale=1.250"));
        }
        let restored = AppState::new_for_tests(workspace.root_path());
        assert!((restored.editor_font_scale() - 1.25).abs() < 0.001);
    }

    #[test]
    fn layout_snapshot_restores_ui_density() {
        let workspace = TestWorkspace::new("layout_density_restore");
        {
            let mut state = AppState::new_for_tests(workspace.root_path());
            state.set_ui_density(UiDensity::Comfortable);
            let layout_snapshot = fs::read_to_string(state.layout_snapshot_path().expect("el test abre un proyecto"))
                .expect("layout snapshot must exist");
            assert!(layout_snapshot.contains("ui_density=comfortable"));
        }
        let restored = AppState::new_for_tests(workspace.root_path());
        assert_eq!(restored.ui_density(), UiDensity::Comfortable);
    }

    #[test]
    fn layout_snapshot_restores_horizontal_spacing_controls() {
        let workspace = TestWorkspace::new("layout_spacing_controls_restore");
        {
            let mut state = AppState::new_for_tests(workspace.root_path());
            state.set_editor_horizontal_padding(18.0);
            state.set_explorer_indent_step(20.0);
            let layout_snapshot = fs::read_to_string(state.layout_snapshot_path().expect("el test abre un proyecto"))
                .expect("layout snapshot must exist");
            assert!(layout_snapshot.contains("editor_horizontal_padding=18.00"));
            assert!(layout_snapshot.contains("explorer_indent_step=20.00"));
        }
        let restored = AppState::new_for_tests(workspace.root_path());
        assert!((restored.editor_horizontal_padding() - 18.0).abs() < 0.01);
        assert!((restored.explorer_indent_step() - 20.0).abs() < 0.01);
    }

    #[test]
    fn explorer_create_file_and_folder_in_selected_directory() {
        let workspace = TestWorkspace::new("explorer_create_file_folder");
        let target_dir = workspace.root_path().join("src");
        fs::create_dir_all(&target_dir).expect("must create target explorer directory");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.refresh_explorer();
        state.explorer_selected_path = Some(target_dir.clone());

        state.explorer_create_new_folder();
        let created_dir = state
            .explorer_selected_path
            .clone()
            .expect("new folder should be selected");
        assert!(created_dir.is_dir());
        assert_eq!(created_dir.parent(), Some(target_dir.as_path()));

        state.explorer_selected_path = Some(created_dir.clone());
        state.explorer_create_new_file();
        let created_file = state
            .active_file_path()
            .cloned()
            .expect("new file should become active");
        assert!(created_file.is_file());
        assert_eq!(created_file.parent(), Some(created_dir.as_path()));
    }

    #[test]
    fn add_selection_to_chat_input_appends_context_block() {
        let workspace = TestWorkspace::new("add_selection_to_chat_context");
        let file = workspace.create_file("src/main.rs", "fn one() {}\nfn two() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(file);
        state
            .active_editor_mut()
            .set_selection_line_columns(0, 0, 0, 10);

        assert!(state.add_selection_to_chat_input());
        assert!(state.input_text.contains("@context src/main.rs:1"));
        assert!(state.input_text.contains("```rs"));
        assert!(state.input_text.contains("fn one() {"));
        assert!(state.input_focused);
    }

    #[test]
    fn command_palette_open_to_side_uses_active_file() {
        let workspace = TestWorkspace::new("palette_open_side");
        let file = workspace.create_file("gamma.rs", "fn gamma() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(file.clone());
        let before = state.open_tabs.len();

        state.begin_command_palette();
        state.overlay_query = "open side".to_string();
        state.rebuild_command_palette_items();
        let target = state
            .overlay_items
            .iter()
            .position(|item| {
                matches!(
                    item.action,
                    OverlayAction::Command(CommandPaletteAction::OpenToSide)
                )
            })
            .expect("command palette must include Open to Side");
        state.overlay_selected = target;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, false));
        assert!(state.is_editor_split_active());
        assert_eq!(state.open_tabs.len(), before + 1);
    }

    #[test]
    fn command_palette_includes_go_to_symbol_command() {
        let workspace = TestWorkspace::new("palette_goto_symbol");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "go symbol".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::GoToSymbol)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_go_to_line_command() {
        let workspace = TestWorkspace::new("palette_goto_line");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "go line".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::GoToLine)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_go_to_workspace_symbol_command() {
        let workspace = TestWorkspace::new("palette_goto_workspace_symbol");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "workspace symbol".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::GoToWorkspaceSymbol)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_find_in_workspace_command() {
        let workspace = TestWorkspace::new("palette_find_workspace");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "find workspace".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::FindInWorkspace)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_theme_graphite_command() {
        let workspace = TestWorkspace::new("palette_theme_graphite");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "theme graphite".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::SetThemeGraphiteDark)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_theme_cycle_commands() {
        let workspace = TestWorkspace::new("palette_theme_cycle");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "theme cycle".to_string();
        state.rebuild_command_palette_items();

        let has_next = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::CycleThemeNext)
            )
        });
        let has_previous = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::CycleThemePrevious)
            )
        });
        assert!(has_next);
        assert!(has_previous);
    }

    #[test]
    fn command_palette_includes_editor_font_commands() {
        let workspace = TestWorkspace::new("palette_editor_font");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "editor font".to_string();
        state.rebuild_command_palette_items();

        let has_increase = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::IncreaseEditorFontScale)
            )
        });
        let has_decrease = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::DecreaseEditorFontScale)
            )
        });
        let has_reset = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ResetEditorFontScale)
            )
        });
        assert!(has_increase);
        assert!(has_decrease);
        assert!(has_reset);
    }

    #[test]
    fn command_palette_includes_density_commands() {
        let workspace = TestWorkspace::new("palette_density");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "density".to_string();
        state.rebuild_command_palette_items();

        let has_compact = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::SetDensityCompact)
            )
        });
        let has_normal = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::SetDensityNormal)
            )
        });
        let has_comfortable = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::SetDensityComfortable)
            )
        });
        assert!(has_compact);
        assert!(has_normal);
        assert!(has_comfortable);
    }

    #[test]
    fn command_palette_includes_show_problems_command() {
        let workspace = TestWorkspace::new("palette_show_problems");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "show problems".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ShowProblems)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_show_problems_git_command() {
        let workspace = TestWorkspace::new("palette_show_problems_git");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "problems git".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ShowProblemsGit)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_show_appearance_panel_command() {
        let workspace = TestWorkspace::new("palette_show_appearance");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "appearance panel".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ShowAppearancePanel)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_explorer_create_commands() {
        let workspace = TestWorkspace::new("palette_explorer_create");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "explorer new".to_string();
        state.rebuild_command_palette_items();

        let has_new_file = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ExplorerNewFile)
            )
        });
        let has_new_folder = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ExplorerNewFolder)
            )
        });
        assert!(has_new_file);
        assert!(has_new_folder);
    }

    #[test]
    fn command_palette_includes_add_selection_to_chat_command() {
        let workspace = TestWorkspace::new("palette_add_selection_chat");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "add selection chat".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::AddSelectionToChat)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_new_window_command() {
        let workspace = TestWorkspace::new("palette_new_window");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "new window".to_string();
        state.rebuild_command_palette_items();

        assert!(state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::NewWindow)
            )
        }));
    }

    #[test]
    fn command_palette_includes_appearance_preset_commands() {
        let workspace = TestWorkspace::new("palette_appearance_presets");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "appearance preset".to_string();
        state.rebuild_command_palette_items();

        let has_dev = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ApplyAppearancePresetDev)
            )
        });
        let has_focus = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ApplyAppearancePresetFocus)
            )
        });
        let has_reading = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ApplyAppearancePresetReading)
            )
        });
        assert!(has_dev);
        assert!(has_focus);
        assert!(has_reading);
    }

    #[test]
    fn go_to_line_overlay_select_moves_cursor() {
        let workspace = TestWorkspace::new("goto_line_overlay_jump");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_tabs[0].editor = Editor::with_text("first\nsecond\nthird\n");

        state.begin_go_to_line_overlay();
        assert_eq!(state.overlay_mode, Some(OverlayMode::GoToLine));
        state.overlay_query = "3:2".to_string();
        state.rebuild_go_to_line_overlay_items();

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, false));
        assert!(state.overlay_mode.is_none());
        assert_eq!(state.active_editor().cursor().line, 2);
        assert_eq!(state.active_editor().cursor().column, 1);
    }

    #[test]
    fn go_to_line_overlay_invalid_query_shows_no_results() {
        let workspace = TestWorkspace::new("goto_line_overlay_invalid");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_go_to_line_overlay();
        state.overlay_query = "abc".to_string();
        state.rebuild_go_to_line_overlay_items();

        assert_eq!(state.overlay_mode, Some(OverlayMode::GoToLine));
        assert!(state.overlay_items.is_empty());
    }

    #[test]
    fn symbol_overlay_select_moves_cursor() {
        let workspace = TestWorkspace::new("symbol_overlay_jump");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_tabs[0].editor = Editor::with_text("fn alpha() {}\n\nfn beta_two() {}\n");

        state.begin_symbol_overlay();
        assert_eq!(state.overlay_mode, Some(OverlayMode::Symbols));
        state.overlay_query = "beta".to_string();
        state.rebuild_symbol_overlay_items();

        let target = state
            .overlay_items
            .iter()
            .position(|item| item.title == "beta_two")
            .expect("symbol overlay must include beta_two");
        state.overlay_selected = target;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, false));
        assert!(state.overlay_mode.is_none());
        assert_eq!(state.active_editor().cursor().line, 2);
        assert_eq!(state.active_editor().cursor().column, 0);
    }

    #[test]
    fn workspace_symbol_overlay_select_opens_file_and_moves_cursor() {
        let workspace = TestWorkspace::new("workspace_symbol_overlay_jump");
        let target = workspace.create_file(
            "src/worker.rs",
            "fn alpha() {}\n\npub fn beta_workspace() {}\n",
        );
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_workspace_symbol_overlay();
        assert_eq!(state.overlay_mode, Some(OverlayMode::WorkspaceSymbols));
        state.overlay_query = "beta_workspace".to_string();
        state.rebuild_workspace_symbol_overlay_items();

        let target_row = state
            .overlay_items
            .iter()
            .position(|item| item.title == "beta_workspace")
            .expect("workspace symbol overlay must include beta_workspace");
        state.overlay_selected = target_row;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, false));
        assert!(state.overlay_mode.is_none());
        assert_eq!(state.active_file_path(), Some(&target));
        assert_eq!(state.active_editor().cursor().line, 2);
        assert_eq!(state.active_editor().cursor().column, 0);
    }

    #[test]
    fn workspace_symbol_overlay_alt_enter_opens_to_side() {
        let workspace = TestWorkspace::new("workspace_symbol_overlay_side");
        let base = workspace.create_file("src/base.rs", "fn base() {}\n");
        let side = workspace.create_file("src/side.rs", "fn side_target() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(base);

        state.begin_workspace_symbol_overlay();
        state.overlay_query = "side_target".to_string();
        state.rebuild_workspace_symbol_overlay_items();
        let target_row = state
            .overlay_items
            .iter()
            .position(|item| item.title == "side_target")
            .expect("workspace symbol overlay must include side_target");
        state.overlay_selected = target_row;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, true));
        assert!(state.overlay_mode.is_none());
        assert!(state.is_editor_split_active());
        assert_eq!(state.file_path_for_pane(EditorPane::Secondary), Some(&side));
        assert_eq!(
            state.editor_for_pane(EditorPane::Secondary).cursor().line,
            0
        );
    }

    #[test]
    fn workspace_text_search_overlay_select_opens_file_and_moves_cursor() {
        let workspace = TestWorkspace::new("workspace_text_search_jump");
        let target = workspace.create_file(
            "src/search_target.rs",
            "fn alpha() {}\nlet important_token = 7;\n",
        );
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_workspace_text_search_overlay();
        assert_eq!(state.overlay_mode, Some(OverlayMode::WorkspaceTextSearch));
        state.overlay_query = "important_token".to_string();
        state.rebuild_workspace_text_search_overlay_items();

        let target_row = state
            .overlay_items
            .iter()
            .position(|item| item.detail.contains("important_token"))
            .expect("workspace text search must include important_token");
        state.overlay_selected = target_row;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, false));
        assert!(state.overlay_mode.is_none());
        assert_eq!(state.active_file_path(), Some(&target));
        assert_eq!(state.active_editor().cursor().line, 1);
    }

    #[test]
    fn workspace_text_search_overlay_alt_enter_opens_to_side() {
        let workspace = TestWorkspace::new("workspace_text_search_side");
        let base = workspace.create_file("src/base.rs", "fn base() {}\n");
        let side = workspace.create_file("src/side.rs", "let side_marker = 1;\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(base);

        state.begin_workspace_text_search_overlay();
        state.overlay_query = "side_marker".to_string();
        state.rebuild_workspace_text_search_overlay_items();
        let target_row = state
            .overlay_items
            .iter()
            .position(|item| item.detail.contains("side_marker"))
            .expect("workspace text search must include side_marker");
        state.overlay_selected = target_row;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, true));
        assert!(state.overlay_mode.is_none());
        assert!(state.is_editor_split_active());
        assert_eq!(state.file_path_for_pane(EditorPane::Secondary), Some(&side));
        assert_eq!(
            state.editor_for_pane(EditorPane::Secondary).cursor().line,
            0
        );
    }

    #[test]
    fn problems_overlay_select_unsaved_tabs_uses_real_problem() {
        let workspace = TestWorkspace::new("problems_overlay_unsaved");
        let file = workspace.create_file("src/dirty.rs", "fn dirty() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(file.clone());
        state.active_editor_mut().insert("// pending\n");
        assert!(state.active_editor().is_modified());

        state.begin_problems_overlay();
        assert_eq!(state.overlay_mode, Some(OverlayMode::Problems));

        let unsaved_idx = state
            .overlay_items
            .iter()
            .position(|item| item.title == "Unsaved tabs")
            .expect("problems overlay must include Unsaved tabs");
        state.overlay_selected = unsaved_idx;

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_overlay_key(&enter, false, false, false));
        assert!(state.overlay_mode.is_none());
        assert!(state.editor_focused);
        assert_eq!(state.sidebar_panel, SidebarPanel::Problems);
    }

    #[test]
    fn problems_overlay_context_filter_by_git() {
        let workspace = TestWorkspace::new("problems_overlay_git_filter");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.active_editor_mut().insert("// unsaved\n");
        state.git_sidebar_error = Some("workspace is not a git repository".to_string());

        state.begin_problems_overlay_with_query("git");
        assert_eq!(state.overlay_mode, Some(OverlayMode::Problems));
        assert_eq!(state.overlay_query, "git");
        assert!(!state.overlay_items.is_empty());
        assert!(state
            .overlay_items
            .iter()
            .all(|item| item.detail.to_lowercase().contains("[git]")));
    }

    #[test]
    fn navigate_problem_next_previous_wraps() {
        let workspace = TestWorkspace::new("problem_navigation_wrap");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.active_editor_mut().insert("// unsaved\n");
        state.search_active = true;
        state.search_query = "no-match-token".to_string();
        state.search_matches.clear();

        assert!(state.navigate_problem(1));
        assert!(state.status_text.contains("problem 1/"));

        assert!(state.navigate_problem(1));
        assert!(state.status_text.contains("problem 2/"));

        assert!(state.navigate_problem(-1));
        assert!(state.status_text.contains("problem 1/"));
    }

    #[test]
    fn sidebar_problem_click_select_uses_real_problem_index() {
        let workspace = TestWorkspace::new("sidebar_problem_click");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.active_editor_mut().insert("// unsaved\n");
        state.set_sidebar_panel(SidebarPanel::Problems);

        state.add_click_target(
            Bounds::new(0.0, 0.0, 24.0, 18.0),
            ClickTargetAction::SidebarProblemSelect(0),
        );
        state.handle_click(8.0, 8.0);

        assert_eq!(state.active_problem_index(), Some(0));
        assert!(state.status_text.contains("Unsaved tabs"));
        assert_eq!(state.sidebar_panel, SidebarPanel::Problems);
    }

    #[test]
    fn command_palette_runtime_budget_action_updates_quiron() {
        let workspace = TestWorkspace::new("palette_runtime_budget");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::SetRuntimeBudgetDeep);

        let quiron = state.quiron.clone();
        let metrics = state.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.delegation_metrics()
        });
        assert_eq!(metrics.token_budget, RUNTIME_TOKEN_BUDGET_DEEP);
        assert!(state.status_text.contains("runtime token budget"));
    }

    #[test]
    fn command_palette_theme_action_updates_ui_theme() {
        let workspace = TestWorkspace::new("palette_theme_update");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::SetThemeGraphiteDark);
        assert_eq!(state.ui_theme(), UiTheme::GraphiteDark);
        assert!(state.status_text.contains("theme: Graphite Dark"));
    }

    /// Una pestaña sin título no es un archivo abierto. Llore arranca con una,
    /// así que sin esta distinción la columna del editor se montaría siempre y
    /// volvería a comerse el centro sin tener nada que enseñar.
    #[test]
    fn una_pestana_sin_titulo_no_cuenta_como_archivo_abierto() {
        let workspace = TestWorkspace::new("hay_archivo_abierto");
        let mut state = AppState::new_for_tests(workspace.root_path());

        // Abrir un proyecto abre su README: eso sí es un archivo abierto, y el
        // editor debe montarse.
        assert!(!state.open_tabs.is_empty(), "arranca con una pestaña");
        assert!(
            state.hay_archivo_abierto(),
            "el arranque abre el README del proyecto"
        );

        // Sin proyecto la pestaña nace sin título, y eso no es un archivo: el
        // editor no debe montarse y el chat se queda la fila.
        for tab in &mut state.open_tabs {
            tab.path = None;
        }
        assert!(
            !state.hay_archivo_abierto(),
            "una pestaña sin título no es un archivo abierto"
        );
    }

    /// La regla de montaje del editor, contra los números de la maqueta.
    ///
    /// Con la lateral en su ancho de partida (252) hacen falta 936 px de fila
    /// para montar el editor, y 1200 si además está abierto el panel de segundo
    /// plano. Por debajo de eso el editor no aparece y el chat se queda el
    /// centro, que es el comportamiento que se ve al estrechar la ventana.
    #[test]
    fn el_editor_solo_se_monta_si_cabe() {
        let workspace = TestWorkspace::new("cabe_el_editor");
        let mut state = AppState::new_for_tests(workspace.root_path());
        assert_eq!(state.sidebar_width, DEFAULT_SIDEBAR_WIDTH);

        // Con el panel de segundo plano abierto: 252+12+320+12+340+12+252.
        state.background_panel_visible = true;
        assert!(!state.cabe_el_editor(1199.0), "no debería caber en 1199");
        assert!(state.cabe_el_editor(1200.0), "debería caber justo en 1200");
        assert!(state.cabe_el_editor(1416.0), "1440 de ventana menos su margen");

        // Al cerrar el panel de segundo plano, el editor cabe mucho antes.
        state.background_panel_visible = false;
        assert!(!state.cabe_el_editor(935.0), "no debería caber en 935");
        assert!(state.cabe_el_editor(936.0), "debería caber justo en 936");

        // Y una lateral más ancha empuja el umbral hacia arriba.
        state.sidebar_width = MAX_SIDEBAR_WIDTH;
        assert!(!state.cabe_el_editor(936.0), "con la lateral a 420 ya no cabe");
    }

    /// Los mínimos de columna de la maqueta: por mucho que se arrastre el asa,
    /// ni el editor baja de 340 ni el chat de 320. Antes ambos compartían un
    /// suelo de 180, con lo que se podía dejar cualquiera de los dos en una
    /// tira inservible.
    #[test]
    fn el_arrastre_respeta_los_minimos_de_columna() {
        let workspace = TestWorkspace::new("resize_min_columnas");
        let mut state = AppState::new_for_tests(workspace.root_path());

        // Fila de 1200 px de contenido: da de sobra para ambos mínimos.
        let fila = Bounds::new(0.0, 0.0, 1200.0, 800.0);
        state.set_main_content_bounds(fila, 12.0);
        state.chat_dock = PanelDock::Right;
        state.dragging_editor_resizer = true;
        let disponible = 1200.0 - 12.0;

        // Arrastre al extremo izquierdo: el editor querría quedarse en nada.
        state.update_layout_resize_from_point(-500.0, 0.0);
        let editor = state.editor_split_ratio * disponible;
        assert!(
            editor >= MIN_EDITOR_WIDTH - 0.5,
            "el editor bajó a {editor}, por debajo de {MIN_EDITOR_WIDTH}"
        );

        // Y al extremo derecho: ahora es el chat el que querría desaparecer.
        state.update_layout_resize_from_point(5000.0, 0.0);
        let chat = disponible - state.editor_split_ratio * disponible;
        assert!(
            chat >= MIN_CHAT_WIDTH - 0.5,
            "el chat bajó a {chat}, por debajo de {MIN_CHAT_WIDTH}"
        );
    }

    /// El bloque de pensamiento nace plegado y se abre y cierra por clic sobre
    /// su encabezado. Un chat nuevo olvida qué estaba abierto: los índices de
    /// un hilo vaciado no significan nada en el siguiente.
    #[test]
    fn el_bloque_de_pensamiento_se_pliega_y_despliega_por_clic() {
        let workspace = TestWorkspace::new("toggle_thought");
        let mut state = AppState::new_for_tests(workspace.root_path());
        assert!(state.expanded_thoughts.is_empty(), "nace plegado");

        let cabecera = Bounds::new(0.0, 0.0, 200.0, 18.0);
        state.clear_click_targets();
        state.add_click_target(cabecera, ClickTargetAction::ToggleThought(3));

        state.handle_click(10.0, 9.0);
        assert!(state.expanded_thoughts.contains(&3), "un clic lo abre");

        state.clear_click_targets();
        state.add_click_target(cabecera, ClickTargetAction::ToggleThought(3));
        state.handle_click(10.0, 9.0);
        assert!(!state.expanded_thoughts.contains(&3), "otro clic lo cierra");

        state.expanded_thoughts.insert(7);
        state.new_chat();
        assert!(state.expanded_thoughts.is_empty(), "el chat nuevo lo olvida");
    }

    /// «Nuevo chat» vacía el hilo, tanto desde el botón de la lateral como desde
    /// la paleta. Y avisa cuando no había nada que vaciar, en vez de fingir que
    /// ha hecho algo.
    #[test]
    fn nuevo_chat_vacia_el_hilo() {
        let workspace = TestWorkspace::new("nuevo_chat");
        let mut state = AppState::new_for_tests(workspace.root_path());

        // El arranque trae un mensaje de bienvenida, así que el hilo no nace
        // vacío: la primera pasada sí tiene algo que descartar.
        assert!(!state.messages.is_empty());
        state.new_chat();
        assert!(state.messages.is_empty());
        assert!(state.status_text.contains("conversación nueva"));

        // Y sobre un hilo ya vacío, lo dice en vez de fingir que hizo algo.
        state.new_chat();
        assert!(state.status_text.contains("ya estaba vacío"));

        state.messages.push(ChatMessage {
            is_user: true,
            content: "hola".to_string(),
            meta: None,
            citations: Vec::new(),
        });
        assert_eq!(state.messages.len(), 1);

        state.execute_command_palette_action(CommandPaletteAction::NewChat);
        assert!(state.messages.is_empty(), "el hilo debería quedar vacío");
        assert!(state.status_text.contains("conversación nueva"));
    }

    /// Al retirar la barra de menús superior, la paleta de comandos quedó como
    /// única entrada a sus acciones. Este test lo sostiene: si alguien añade una
    /// entrada de menú sin darle su descriptor, aquí se entera.
    ///
    /// Fue así como apareció `OpenFolderPicker`, la única de las treinta y cinco
    /// que no estaba en la paleta —y sin la cual no habría forma de abrir una
    /// carpeta una vez quitada la barra.
    #[test]
    fn cada_accion_de_menu_vive_tambien_en_la_paleta() {
        for menu in [
            TopMenuKind::File,
            TopMenuKind::Edit,
            TopMenuKind::View,
            TopMenuKind::Go,
            TopMenuKind::Project,
            TopMenuKind::Help,
        ] {
            for entrada in top_menu_entries(menu) {
                assert!(
                    COMMAND_DESCRIPTORS
                        .iter()
                        .any(|d| d.action == entrada.action),
                    "{:?} está en el menú {:?} pero no en la paleta",
                    entrada.action,
                    menu
                );
            }
        }
    }

    /// La barra de actividad era la única entrada a cinco de estos paneles.
    /// Al retirarla, la paleta pasa a ser la única que queda: si este test cae,
    /// hay paneles que el usuario no puede abrir por ningún camino.
    #[test]
    fn command_palette_reaches_every_sidebar_panel() {
        let workspace = TestWorkspace::new("palette_sidebar_panels");
        let mut state = AppState::new_for_tests(workspace.root_path());

        for (accion, esperado) in [
            (
                CommandPaletteAction::ShowExplorerPanel,
                SidebarPanel::Explorer,
            ),
            (CommandPaletteAction::ShowSearchPanel, SidebarPanel::Search),
            (CommandPaletteAction::ShowGitPanel, SidebarPanel::Git),
            (CommandPaletteAction::ShowOutlinePanel, SidebarPanel::Outline),
            (
                CommandPaletteAction::ShowAppearancePanel,
                SidebarPanel::Appearance,
            ),
            (
                CommandPaletteAction::ShowSecurityPanel,
                SidebarPanel::Security,
            ),
        ] {
            state.execute_command_palette_action(accion);
            assert_eq!(
                state.sidebar_panel, esperado,
                "{accion:?} no abre {esperado:?}"
            );
        }
    }

    #[test]
    fn command_palette_theme_cycle_actions_rotate_with_wrap() {
        let workspace = TestWorkspace::new("palette_theme_cycle_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        // El anillo arranca en Modernist Light, que es el aspecto por defecto.
        let anillo = [
            UiTheme::ModernistLight,
            UiTheme::QuironDark,
            UiTheme::GraphiteDark,
            UiTheme::CopperLight,
        ];
        assert_eq!(state.ui_theme(), anillo[0]);
        for esperado in anillo.iter().skip(1).chain(std::iter::once(&anillo[0])) {
            state.execute_command_palette_action(CommandPaletteAction::CycleThemeNext);
            assert_eq!(state.ui_theme(), *esperado);
        }
        // `rev()` ya termina en el tema de arranque: encadenar otro lo pasaría.
        for esperado in anillo.iter().rev() {
            state.execute_command_palette_action(CommandPaletteAction::CycleThemePrevious);
            assert_eq!(state.ui_theme(), *esperado);
        }
    }

    #[test]
    fn command_palette_editor_font_actions_update_scale() {
        let workspace = TestWorkspace::new("palette_editor_font_scale_state");
        let mut state = AppState::new_for_tests(workspace.root_path());
        assert!((state.editor_font_scale() - DEFAULT_EDITOR_FONT_SCALE).abs() < 0.0001);

        state.execute_command_palette_action(CommandPaletteAction::IncreaseEditorFontScale);
        assert!(state.editor_font_scale() > DEFAULT_EDITOR_FONT_SCALE);

        state.execute_command_palette_action(CommandPaletteAction::ResetEditorFontScale);
        assert!((state.editor_font_scale() - DEFAULT_EDITOR_FONT_SCALE).abs() < 0.0001);
    }

    #[test]
    fn command_palette_density_actions_update_state() {
        let workspace = TestWorkspace::new("palette_density_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        assert_eq!(state.ui_density(), UiDensity::Normal);
        state.execute_command_palette_action(CommandPaletteAction::SetDensityCompact);
        assert_eq!(state.ui_density(), UiDensity::Compact);
        state.execute_command_palette_action(CommandPaletteAction::CycleDensityNext);
        assert_eq!(state.ui_density(), UiDensity::Normal);
        state.execute_command_palette_action(CommandPaletteAction::SetDensityComfortable);
        assert_eq!(state.ui_density(), UiDensity::Comfortable);
    }

    #[test]
    fn command_palette_show_appearance_panel_action_updates_state() {
        let workspace = TestWorkspace::new("palette_show_appearance_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.set_sidebar_panel(SidebarPanel::Search);
        state.execute_command_palette_action(CommandPaletteAction::ShowAppearancePanel);
        assert_eq!(state.sidebar_panel, SidebarPanel::Appearance);
    }

    #[test]
    fn command_palette_go_to_line_action_opens_overlay() {
        let workspace = TestWorkspace::new("palette_goto_line_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::GoToLine);
        assert_eq!(state.overlay_mode, Some(OverlayMode::GoToLine));
    }

    #[test]
    fn command_palette_find_in_workspace_action_opens_overlay() {
        let workspace = TestWorkspace::new("palette_find_workspace_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::FindInWorkspace);
        assert_eq!(state.overlay_mode, Some(OverlayMode::WorkspaceTextSearch));
        assert_eq!(state.sidebar_panel, SidebarPanel::Search);
    }

    #[test]
    fn command_palette_reset_appearance_action_resets_all_state() {
        let workspace = TestWorkspace::new("palette_reset_appearance");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.set_ui_theme(UiTheme::CopperLight);
        state.set_ui_density(UiDensity::Comfortable);
        state.set_editor_font_scale(1.25);
        state.execute_command_palette_action(CommandPaletteAction::ResetAppearance);

        assert_eq!(state.ui_theme(), UiTheme::ModernistLight);
        assert_eq!(state.ui_density(), UiDensity::Normal);
        assert!((state.editor_font_scale() - DEFAULT_EDITOR_FONT_SCALE).abs() < 0.0001);
    }

    #[test]
    fn command_palette_appearance_preset_actions_update_state() {
        let workspace = TestWorkspace::new("palette_apply_appearance_presets");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::ApplyAppearancePresetDev);
        assert_eq!(state.ui_theme(), UiTheme::GraphiteDark);
        assert_eq!(state.ui_density(), UiDensity::Compact);
        assert!((state.editor_font_scale() - 0.95).abs() < 0.0001);

        state.execute_command_palette_action(CommandPaletteAction::ApplyAppearancePresetFocus);
        assert_eq!(state.ui_theme(), UiTheme::QuironDark);
        assert_eq!(state.ui_density(), UiDensity::Normal);
        assert!((state.editor_font_scale() - 1.10).abs() < 0.0001);

        state.execute_command_palette_action(CommandPaletteAction::ApplyAppearancePresetReading);
        assert_eq!(state.ui_theme(), UiTheme::CopperLight);
        assert_eq!(state.ui_density(), UiDensity::Comfortable);
        assert!((state.editor_font_scale() - 1.20).abs() < 0.0001);
    }

    #[test]
    fn command_palette_runtime_threshold_action_updates_scales() {
        let workspace = TestWorkspace::new("palette_runtime_threshold");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::SetRuntimeThresholdWorkerBias);

        let quiron = state.quiron.clone();
        let metrics = state.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.delegation_metrics()
        });
        assert!((metrics.worker_threshold_scale - RUNTIME_WORKER_SCALE_WORKER_BIAS).abs() < 0.0001);
        assert!(
            (metrics.primary_threshold_scale - RUNTIME_PRIMARY_SCALE_WORKER_BIAS).abs() < 0.0001
        );
        assert!(state.status_text.contains("runtime thresholds"));
    }

    #[test]
    fn command_palette_runtime_parallel_action_updates_cap() {
        let workspace = TestWorkspace::new("palette_runtime_parallel");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::SetRuntimeParallelWide);

        let quiron = state.quiron.clone();
        let metrics = state.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.delegation_metrics()
        });
        assert_eq!(metrics.parallel_subtasks_cap, RUNTIME_PARALLEL_CAP_WIDE);
        assert!(state.status_text.contains("runtime parallel cap"));
    }

    #[test]
    fn command_palette_includes_session_telemetry_status_command() {
        let workspace = TestWorkspace::new("palette_telemetry_status");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "telemetry status".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ShowSessionTelemetryStatus)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_toggle_telemetry_panel_command() {
        let workspace = TestWorkspace::new("palette_telemetry_toggle");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "telemetry panel".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ToggleTelemetryPanel)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_toggle_telemetry_panel_action_updates_state() {
        let workspace = TestWorkspace::new("palette_telemetry_toggle_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        assert!(!state.is_telemetry_panel_enabled());
        state.execute_command_palette_action(CommandPaletteAction::ToggleTelemetryPanel);
        assert!(state.is_telemetry_panel_enabled());
        assert!(state.status_text.contains("telemetry panel enabled"));
        state.execute_command_palette_action(CommandPaletteAction::ToggleTelemetryPanel);
        assert!(!state.is_telemetry_panel_enabled());
    }

    #[test]
    fn command_palette_includes_secure_reconnect_command() {
        let workspace = TestWorkspace::new("palette_secure_reconnect");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "secure reconnect".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ReconnectQuironSecureLocal)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_connection_status_posts_chat_message() {
        let workspace = TestWorkspace::new("palette_connection_status");
        let mut state = AppState::new_for_tests(workspace.root_path());
        let before = state.messages.len();

        state.execute_command_palette_action(CommandPaletteAction::ShowQuironConnectionStatus);

        assert!(state.messages.len() > before);
        assert!(state
            .messages
            .last()
            .expect("connection status message")
            .content
            .contains("Connection status"));
    }

    #[test]
    fn chat_control_command_connection_does_not_call_llm() {
        let workspace = TestWorkspace::new("chat_control_connection");
        let mut state = AppState::new_for_tests(workspace.root_path());
        let before = state.messages.len();

        state.send_message("/connection");

        assert!(state.messages.len() >= before + 2);
        let last = state.messages.last().expect("connection status response");
        assert!(last.content.contains("Connection status"));
        assert!(!state.loading);
    }

    #[test]
    fn reconnect_starts_async_health_check_task() {
        let workspace = TestWorkspace::new("async_health_task");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.execute_command_palette_action(CommandPaletteAction::ReconnectQuironSecureLocal);

        assert!(state.quiron_health_task.is_some());
        assert_eq!(state.quiron_connection_health_label(), "health=checking");
    }

    #[test]
    fn activity_click_targets_drive_overlay_and_focus() {
        let workspace = TestWorkspace::new("activity_click_targets");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityQuickOpen,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.overlay_mode, Some(OverlayMode::QuickOpen));

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityCommandPalette,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.overlay_mode, Some(OverlayMode::CommandPalette));

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityFocusChat,
        );
        state.handle_click(10.0, 10.0);
        assert!(state.input_focused);

        state
            .active_editor_mut()
            .set_selection_line_columns(0, 0, 0, 2);
        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 40.0, 20.0),
            ClickTargetAction::ActivityAddSelectionToChat,
        );
        state.handle_click(10.0, 10.0);
        assert!(state.input_text.contains("@context"));

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySidebarSearch,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.sidebar_panel, SidebarPanel::Search);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySidebarGit,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.sidebar_panel, SidebarPanel::Git);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySidebarProblems,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.sidebar_panel, SidebarPanel::Problems);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySidebarOutline,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.sidebar_panel, SidebarPanel::Outline);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySidebarAppearance,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.sidebar_panel, SidebarPanel::Appearance);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySidebarSecurity,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.sidebar_panel, SidebarPanel::Security);

        let before_telemetry = state.is_telemetry_panel_enabled();
        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityToggleTelemetry,
        );
        state.handle_click(10.0, 10.0);
        assert_ne!(state.is_telemetry_panel_enabled(), before_telemetry);

        let before_theme = state.ui_theme();
        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityCycleTheme,
        );
        state.handle_click(10.0, 10.0);
        assert_ne!(state.ui_theme(), before_theme);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySetTheme(UiTheme::CopperLight),
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.ui_theme(), UiTheme::CopperLight);

        let before_density = state.ui_density();
        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityCycleDensity,
        );
        state.handle_click(10.0, 10.0);
        assert_ne!(state.ui_density(), before_density);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivitySetDensity(UiDensity::Compact),
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.ui_density(), UiDensity::Compact);

        let before_font = state.editor_font_scale();
        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityIncreaseEditorFontScale,
        );
        state.handle_click(10.0, 10.0);
        assert!(state.editor_font_scale() > before_font);

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityResetEditorFontScale,
        );
        state.handle_click(10.0, 10.0);
        assert!((state.editor_font_scale() - DEFAULT_EDITOR_FONT_SCALE).abs() < 0.0001);

        state.set_ui_theme(UiTheme::CopperLight);
        state.set_ui_density(UiDensity::Comfortable);
        state.set_editor_font_scale(1.30);
        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityResetAppearance,
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.ui_theme(), UiTheme::ModernistLight);
        assert_eq!(state.ui_density(), UiDensity::Normal);
        assert!((state.editor_font_scale() - DEFAULT_EDITOR_FONT_SCALE).abs() < 0.0001);
    }

    #[test]
    fn activity_click_target_apply_appearance_preset_updates_state() {
        let workspace = TestWorkspace::new("activity_apply_appearance_preset");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.set_ui_theme(UiTheme::CopperLight);
        state.set_ui_density(UiDensity::Comfortable);
        state.set_editor_font_scale(1.30);
        state.add_click_target(
            Bounds::new(0.0, 0.0, 20.0, 20.0),
            ClickTargetAction::ActivityApplyAppearancePreset(UiAppearancePreset::Focus),
        );
        state.handle_click(10.0, 10.0);
        assert_eq!(state.ui_theme(), UiTheme::QuironDark);
        assert_eq!(state.ui_density(), UiDensity::Normal);
        assert!((state.editor_font_scale() - 1.10).abs() < 0.0001);
    }

    #[test]
    fn top_menu_toggle_execute_and_close_on_outside_click() {
        let workspace = TestWorkspace::new("top_menu_toggle_execute");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 24.0, 18.0),
            ClickTargetAction::TopMenuToggle(TopMenuKind::File),
        );
        state.handle_click(8.0, 8.0);
        assert_eq!(state.top_menu_open(), Some(TopMenuKind::File));

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 64.0, 18.0),
            ClickTargetAction::TopMenuExecute(CommandPaletteAction::RefreshExplorer),
        );
        state.handle_click(8.0, 8.0);
        assert_eq!(state.top_menu_open(), None);
        assert!(state.status_text.contains("explorer refreshed"));

        state.clear_click_targets();
        state.add_click_target(
            Bounds::new(0.0, 0.0, 24.0, 18.0),
            ClickTargetAction::TopMenuToggle(TopMenuKind::Edit),
        );
        state.handle_click(8.0, 8.0);
        assert_eq!(state.top_menu_open(), Some(TopMenuKind::Edit));

        state.clear_click_targets();
        state.handle_click(99999.0, 99999.0);
        assert_eq!(state.top_menu_open(), None);
    }

    #[test]
    fn top_menu_keyboard_navigation_and_execute() {
        let workspace = TestWorkspace::new("top_menu_keyboard_nav");
        let mut state = AppState::new_for_tests(workspace.root_path());

        let alt_p = Key::Character("p".into());
        assert!(state.handle_top_menu_key(&alt_p, false, true));
        assert_eq!(state.top_menu_open(), Some(TopMenuKind::Project));
        assert_eq!(state.top_menu_selected_index(), 0);

        let down = Key::Named(NamedKey::ArrowDown);
        assert!(state.handle_top_menu_key(&down, false, false));
        assert_eq!(state.top_menu_selected_index(), 1);
        assert!(state.handle_top_menu_key(&down, false, false));
        assert_eq!(state.top_menu_selected_index(), 2);

        let enter = Key::Named(NamedKey::Enter);
        assert!(state.handle_top_menu_key(&enter, false, false));
        assert_eq!(state.top_menu_open(), None);
        assert!(state.status_text.contains("explorer refreshed"));

        let alt_f = Key::Character("f".into());
        assert!(state.handle_top_menu_key(&alt_f, false, true));
        assert_eq!(state.top_menu_open(), Some(TopMenuKind::File));
        let right = Key::Named(NamedKey::ArrowRight);
        assert!(state.handle_top_menu_key(&right, false, false));
        assert_eq!(state.top_menu_open(), Some(TopMenuKind::Edit));

        let escape = Key::Named(NamedKey::Escape);
        assert!(state.handle_top_menu_key(&escape, false, false));
        assert_eq!(state.top_menu_open(), None);
    }

    #[test]
    fn parse_git_branch_header_extracts_branch_and_divergence() {
        let (branch, ahead, behind) =
            AppState::parse_git_branch_header("## main...origin/main [ahead 3, behind 2]");
        assert_eq!(branch, "main");
        assert_eq!(ahead, 3);
        assert_eq!(behind, 2);
    }

    #[test]
    fn sidebar_problems_snapshot_reports_real_conditions() {
        let workspace = TestWorkspace::new("sidebar_problems");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.active_editor_mut().insert("dirty");
        state.telemetry_persisted_last_error = Some("persisted fetch failed".to_string());
        state.git_sidebar_error = Some("workspace is not a git repository".to_string());
        state.search_active = true;
        state.search_query = "abc123".to_string();
        state.search_matches.clear();
        state.status_text = "replace failed: pattern error".to_string();

        let issues = state.sidebar_problems_snapshot();
        assert!(issues.iter().any(|v| v.title == "Unsaved tabs"));
        assert!(issues.iter().any(|v| v.title == "Telemetry persisted"));
        assert!(issues.iter().any(|v| v.title == "Git status"));
        assert!(issues.iter().any(|v| v.title == "Find no matches"));
        assert!(issues.iter().any(|v| v.title == "Latest status"));
    }

    #[test]
    fn sidebar_problems_snapshot_includes_syntax_error_for_rust() {
        let workspace = TestWorkspace::new("sidebar_problems_syntax_error");
        let broken = workspace.create_file("src/broken.rs", "fn broken( {\n let x = 1;\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(broken);

        let issues = state.sidebar_problems_snapshot();
        assert!(issues.iter().any(|v| v.title == "Syntax error"));
    }

    #[test]
    fn sidebar_problems_snapshot_skips_syntax_error_for_valid_rust() {
        let workspace = TestWorkspace::new("sidebar_problems_syntax_ok");
        let ok = workspace.create_file("src/ok.rs", "fn ok() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(ok);

        let issues = state.sidebar_problems_snapshot();
        assert!(!issues.iter().any(|v| v.title == "Syntax error"));
    }

    #[test]
    fn sidebar_outline_snapshot_detects_symbols() {
        let workspace = TestWorkspace::new("sidebar_outline");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_tabs[0].editor = Editor::with_text(
            "# Intro\n\npub struct Brain {}\n\nimpl Brain {\n    pub fn think(&self) {}\n}\n",
        );

        let outline = state.sidebar_outline_snapshot(EditorPane::Primary);
        assert!(outline.iter().any(|v| v.kind == "H1" && v.label == "Intro"));
        assert!(outline
            .iter()
            .any(|v| v.kind == "struct" && v.label == "Brain"));
        assert!(outline.iter().any(|v| v.kind == "fn" && v.label == "think"));
    }

    #[test]
    fn outline_select_click_moves_cursor() {
        let workspace = TestWorkspace::new("outline_select");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_tabs[0].editor = Editor::with_text("fn one() {}\nfn two() {}\n");

        state.add_click_target(
            Bounds::new(0.0, 0.0, 24.0, 18.0),
            ClickTargetAction::OutlineSelect {
                pane: EditorPane::Primary,
                line: 1,
                column: 3,
            },
        );
        state.handle_click(8.0, 8.0);

        assert_eq!(state.sidebar_panel, SidebarPanel::Outline);
        assert_eq!(state.active_editor().cursor().line, 1);
        assert_eq!(state.active_editor().cursor().column, 3);
    }

    #[test]
    fn breadcrumb_segment_click_reveals_path_in_explorer() {
        let workspace = TestWorkspace::new("breadcrumb_click");
        let nested_file = workspace.create_file("a/b/c/file.rs", "fn test() {}\n");
        let nested_dir = workspace.root_path().join("a/b");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(nested_file);
        state.set_sidebar_panel(SidebarPanel::Search);

        state.add_click_target(
            Bounds::new(0.0, 0.0, 24.0, 18.0),
            ClickTargetAction::BreadcrumbSegment {
                pane: EditorPane::Primary,
                path: nested_dir.clone(),
                is_file: false,
            },
        );
        state.handle_click(8.0, 8.0);

        assert_eq!(state.sidebar_panel, SidebarPanel::Explorer);
        assert_eq!(state.explorer_selected_path.as_ref(), Some(&nested_dir));
        assert!(state.is_dir_expanded(&nested_dir));
    }

    #[test]
    fn toggle_telemetry_panel_resets_and_primes_prefetch_state() {
        let workspace = TestWorkspace::new("telemetry_prefetch_toggle");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.toggle_telemetry_panel();
        assert!(state.is_telemetry_panel_enabled());

        state.telemetry_persisted_prefetch_session = Some("session-x".to_string());
        state.telemetry_persisted_prefetch_task = Some(
            state
                .runtime
                .spawn(async { Err("prefetch cancelled".to_string()) }),
        );
        state.telemetry_persisted_prefetch_last_at =
            Instant::now() - TELEMETRY_PERSISTED_PREFETCH_INTERVAL * 2;

        state.toggle_telemetry_panel();
        assert!(!state.is_telemetry_panel_enabled());
        assert!(state.telemetry_persisted_prefetch_task.is_none());
        assert!(state.telemetry_persisted_prefetch_session.is_none());
        assert!(
            Instant::now()
                .duration_since(state.telemetry_persisted_prefetch_last_at)
                .as_secs_f32()
                < 1.0
        );

        state.toggle_telemetry_panel();
        assert!(state.is_telemetry_panel_enabled());
        assert!(
            Instant::now().duration_since(state.telemetry_persisted_prefetch_last_at)
                >= TELEMETRY_PERSISTED_PREFETCH_INTERVAL
        );
    }

    #[test]
    fn command_palette_includes_cycle_telemetry_timeline_filter_command() {
        let workspace = TestWorkspace::new("palette_telemetry_filter");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "telemetry filter".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::CycleTelemetryTimelineFilter)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_cycle_telemetry_timeline_filter_rotates_state() {
        let workspace = TestWorkspace::new("palette_telemetry_filter_state");
        let mut state = AppState::new_for_tests(workspace.root_path());

        assert_eq!(
            state.telemetry_timeline_filter(),
            TelemetryTimelineFilter::All
        );
        state.execute_command_palette_action(CommandPaletteAction::CycleTelemetryTimelineFilter);
        assert_eq!(
            state.telemetry_timeline_filter(),
            TelemetryTimelineFilter::Checkpoints
        );
        state.execute_command_palette_action(CommandPaletteAction::CycleTelemetryTimelineFilter);
        assert_eq!(
            state.telemetry_timeline_filter(),
            TelemetryTimelineFilter::Anomalies
        );
        state.execute_command_palette_action(CommandPaletteAction::CycleTelemetryTimelineFilter);
        assert_eq!(
            state.telemetry_timeline_filter(),
            TelemetryTimelineFilter::All
        );
    }

    #[test]
    fn command_palette_includes_refresh_telemetry_persisted_cache_command() {
        let workspace = TestWorkspace::new("palette_telemetry_sync");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "telemetry sync persisted".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::RefreshTelemetryPersistedCache)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn command_palette_includes_load_more_telemetry_persisted_cache_command() {
        let workspace = TestWorkspace::new("palette_telemetry_load_more");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "telemetry load older".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::LoadMoreTelemetryPersistedCache)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn telemetry_panel_info_merges_persisted_cache_entries() {
        let workspace = TestWorkspace::new("telemetry_panel_persisted");
        let mut state = AppState::new_for_tests(workspace.root_path());

        let quiron = state.quiron.clone();
        let session_id = state.runtime.block_on(async move {
            let q = quiron.lock().await;
            q.session_telemetry_snapshot().session_id
        });
        state.telemetry_persisted_cache = Some(SessionTelemetryResponse {
            schema_version: 1,
            session_id,
            checkpoints_total: 1,
            anomalies_total: 1,
            checkpoints_offset: 0,
            anomalies_offset: 0,
            has_more_checkpoints: false,
            has_more_anomalies: false,
            checkpoints: vec![SessionTelemetryCheckpointSummary {
                segment_seq: 99,
                step_start: 100,
                step_end: 120,
                ts_start: "2026-02-15T12:00:00Z".to_string(),
                ts_end: "2026-02-15T12:05:00Z".to_string(),
                model_tokens_used_delta: 500,
                model_tokens_used_total: 4200,
                token_budget: 100_000,
                token_budget_remaining: 95_800,
                llm_fallback_rate: 0.05,
                anomaly_flags: vec![],
                preset_change: None,
            }],
            anomalies: vec![SessionTelemetryAnomalySummary {
                segment_seq: 99,
                step: 118,
                kind: "budget_low".to_string(),
                request_iteration: 7,
                timestamp: "2026-02-15T12:04:00Z".to_string(),
                detail: "remaining 14%".to_string(),
            }],
        });
        state.telemetry_persisted_last_error = None;

        let panel = state.session_telemetry_panel_info();
        assert_eq!(panel.persisted_checkpoints, 1);
        assert_eq!(panel.persisted_anomalies, 1);
        assert_eq!(panel.persisted_checkpoints_total, 1);
        assert_eq!(panel.persisted_anomalies_total, 1);
        assert_eq!(panel.persisted_sync, "ok");
        assert!(panel
            .timeline
            .iter()
            .any(|entry| entry.source == SessionTelemetryTimelineSource::Persisted));
    }

    #[test]
    fn command_palette_includes_session_telemetry_report_command() {
        let workspace = TestWorkspace::new("palette_telemetry_report");
        let mut state = AppState::new_for_tests(workspace.root_path());

        state.begin_command_palette();
        state.overlay_query = "telemetry report".to_string();
        state.rebuild_command_palette_items();

        let has_command = state.overlay_items.iter().any(|item| {
            matches!(
                item.action,
                OverlayAction::Command(CommandPaletteAction::ShowSessionTelemetryReport)
            )
        });
        assert!(has_command);
    }

    #[test]
    fn telemetry_report_action_posts_system_message() {
        let workspace = TestWorkspace::new("telemetry_report_action");
        let mut state = AppState::new_for_tests(workspace.root_path());
        let before = state.messages.len();

        state.execute_command_palette_action(CommandPaletteAction::ShowSessionTelemetryReport);

        assert_eq!(state.messages.len(), before + 1);
        let last = state.messages.last().expect("message must exist");
        assert!(!last.is_user);
        assert_eq!(last.meta.as_deref(), Some("telemetry_report"));
        assert!(last.content.contains("Session telemetry report"));
        assert!(state.status_text.contains("telemetry report posted"));
    }

    #[test]
    fn open_selected_or_active_to_side_prefers_explorer_selection() {
        let workspace = TestWorkspace::new("explorer_open_side");
        let active_file = workspace.create_file("active.rs", "fn active() {}\n");
        let selected_file = workspace.create_file("selected.rs", "fn selected() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(active_file);
        state.explorer_selected_path = Some(selected_file.clone());

        state.open_selected_or_active_to_side();

        assert!(state.is_editor_split_active());
        assert_eq!(
            state.file_path_for_pane(EditorPane::Secondary),
            Some(&selected_file)
        );
    }

    #[test]
    fn arranca_sin_proyecto_abierto() {
        let state = AppState::new_for_tests(PathBuf::new());

        assert!(!state.workspace_is_open());
        assert_eq!(state.open_tabs.len(), 1);
        assert!(state.open_tabs[0].path.is_none(), "no debe abrir ningún archivo");
    }

    #[test]
    fn sin_proyecto_el_explorador_queda_vacio() {
        let mut state = AppState::new_for_tests(PathBuf::new());
        state.refresh_explorer();

        assert!(state.explorer_entries.is_empty());
    }

    #[test]
    fn sin_proyecto_toda_ruta_queda_fuera_de_alcance() {
        let workspace = TestWorkspace::new("sin_proyecto_guardia");
        let archivo = workspace.create_file("visible.rs", "fn f() {}\n");
        let state = AppState::new_for_tests(PathBuf::new());

        assert_eq!(state.access_to(&archivo), Access::Outside);
        assert!(state.read_project_file(&archivo).is_none());
    }

    #[test]
    fn sin_proyecto_el_chat_pide_abrir_una_carpeta() {
        let mut state = AppState::new_for_tests(PathBuf::new());
        let previos = state.messages.len();

        state.send_message("¿qué hace este repositorio?");

        assert_eq!(state.messages.len(), previos + 1);
        let ultimo = state.messages.last().expect("debe haber respuesta");
        assert!(!ultimo.is_user);
        assert!(ultimo.content.contains("Abre una carpeta"));
        assert!(!state.loading, "no debe quedarse esperando al modelo");
    }

    #[test]
    fn el_explorador_oculta_secretos_y_ruido() {
        let workspace = TestWorkspace::new("explorador_secretos");
        workspace.create_file("src/main.rs", "fn main() {}\n");
        workspace.create_file(".env", "TOKEN=secreto\n");
        workspace.create_file(".env.example", "TOKEN=\n");
        workspace.create_file("target/debug/artefacto", "binario\n");

        let mut state = AppState::new_for_tests(workspace.root_path());
        state.refresh_explorer();

        let nombres: Vec<&str> = state
            .explorer_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();

        assert!(nombres.contains(&"src"), "el código debe verse: {nombres:?}");
        assert!(nombres.contains(&".env.example"), "las plantillas se ven: {nombres:?}");
        assert!(!nombres.contains(&".env"), "el secreto no debe listarse: {nombres:?}");
        assert!(!nombres.contains(&"target"), "el ruido se oculta: {nombres:?}");
    }

    #[test]
    fn no_se_lee_un_secreto_del_propio_proyecto() {
        let workspace = TestWorkspace::new("lectura_secreto");
        let secreto = workspace.create_file(".env", "TOKEN=secreto\n");
        let state = AppState::new_for_tests(workspace.root_path());

        assert_eq!(state.access_to(&secreto), Access::Secret);
        assert!(state.read_project_file(&secreto).is_none());
        assert!(state.access_to(&secreto).is_absolute_denial());
    }

    #[test]
    fn abrir_un_archivo_de_fuera_del_proyecto_se_deniega() {
        let workspace = TestWorkspace::new("abrir_fuera");
        let ajeno = std::env::temp_dir().join(format!("llore_ajeno_{}.rs", std::process::id()));
        fs::write(&ajeno, "fn ajeno() {}\n").expect("crear archivo ajeno");

        let mut state = AppState::new_for_tests(workspace.root_path());
        let pestanas = state.open_tabs.len();

        state.open_file_in_pane(ajeno.clone(), EditorPane::Primary, false);

        assert_eq!(state.open_tabs.len(), pestanas, "no debe abrirse");
        assert!(state.status_text.contains("Acceso denegado"));
        let _ = fs::remove_file(&ajeno);
    }

    #[test]
    fn sin_proyecto_no_se_escribe_estado_en_disco() {
        // `workspace_root.join(...)` sobre una raíz vacía produce una ruta
        // relativa: el editor escribiría su estado en el directorio de trabajo,
        // fuese cual fuese.
        let state = AppState::new_for_tests(PathBuf::new());

        assert!(state.session_snapshot_path().is_none());
        assert!(state.layout_snapshot_path().is_none());

        state.persist_layout_snapshot();
        state.persist_session_snapshot();

        assert!(!Path::new(LAYOUT_SNAPSHOT_RELATIVE_PATH).exists());
        assert!(!Path::new(SESSION_SNAPSHOT_RELATIVE_PATH).exists());
    }

    #[test]
    fn sin_proyecto_el_chat_no_tiene_contexto() {
        let state = AppState::new_for_tests(PathBuf::new());
        assert!(state.build_chat_context().is_empty());
    }

    #[test]
    fn el_chat_recibe_el_archivo_abierto_y_su_contenido() {
        let workspace = TestWorkspace::new("contexto_chat");
        let archivo = workspace.create_file("src/display.rs", "fn pintar() { todo!() }\n");

        let mut state = AppState::new_for_tests(PathBuf::new());
        state.open_workspace(workspace.root_path());
        state.open_file_in_pane(archivo, EditorPane::Primary, false);

        let contexto = state.build_chat_context();

        assert!(contexto.contains("src/display.rs"), "falta la ruta: {contexto}");
        assert!(contexto.contains("fn pintar()"), "falta el contenido");
        assert!(contexto.contains("Identidad:"), "falta la identidad del proyecto");
        assert!(
            contexto.contains(state.project_id.as_deref().unwrap()),
            "la identidad no es el ULID"
        );
    }

    #[test]
    fn un_archivo_enorme_se_recorta_y_el_recorte_se_declara() {
        let workspace = TestWorkspace::new("contexto_grande");
        let gigante = "x".repeat(CHAT_CONTEXT_MAX_CHARS + 5_000);
        let archivo = workspace.create_file("grande.txt", &gigante);

        let mut state = AppState::new_for_tests(PathBuf::new());
        state.open_workspace(workspace.root_path());
        state.open_file_in_pane(archivo, EditorPane::Primary, false);

        let contexto = state.build_chat_context();

        assert!(contexto.contains("[recortado:"), "el recorte debe declararse");
        assert!(
            contexto.len() < gigante.len(),
            "no se recortó nada"
        );
    }

    #[test]
    fn las_instrucciones_declaran_las_manos_y_sus_reglas() {
        // Antes de la capa 4 este test aseguraba lo contrario: que el prompt
        // negara herramientas inexistentes. Ahora existen, y un prompt que
        // las niegue hace que el modelo las ignore aunque viajen en el
        // request (pasó: «pídelo por su ruta y espera a que el usuario lo
        // abra» venció a read_file).
        let workspace = TestWorkspace::new("system_prompt");
        let mut state = AppState::new_for_tests(PathBuf::new());
        state.open_workspace(workspace.root_path());

        let system = state.chat_system_prompt();
        for herramienta in ["read_file", "list_files", "search_text"] {
            assert!(
                system.contains(herramienta),
                "el modelo debe saber que tiene {herramienta}: {system}"
            );
        }
        assert!(
            !system.contains("No puedes abrir otros archivos"),
            "la negación de la era sin manos no puede sobrevivir"
        );
        assert!(
            system.contains("guardia") && system.contains("deniegan"),
            "las reglas del arnés forman parte de las instrucciones"
        );
    }

    #[test]
    fn la_escala_de_la_interfaz_se_ajusta_y_vuelve_a_su_sitio() {
        let mut state = AppState::new_for_tests(PathBuf::new());
        assert_eq!(state.ui_scale.user(), 1.0);

        state.increase_ui_scale();
        let agrandada = state.ui_scale.user();
        assert!(agrandada > 1.0, "no creció: {agrandada}");
        assert!(state.status_text.contains("interfaz al"));

        state.decrease_ui_scale();
        assert!((state.ui_scale.user() - 1.0).abs() < 0.001);

        // Los topes protegen de una interfaz inservible.
        for _ in 0..100 {
            state.increase_ui_scale();
        }
        assert_eq!(state.ui_scale.user(), crate::design::UI_SCALE_MAX);

        state.reset_ui_scale();
        assert_eq!(state.ui_scale.user(), 1.0);
    }

    #[test]
    fn sin_proyecto_no_hay_identidad() {
        let state = AppState::new_for_tests(PathBuf::new());
        assert!(state.project_id.is_none());
    }

    #[test]
    fn conceder_acceso_crea_la_identidad_del_proyecto() {
        let workspace = TestWorkspace::new("identidad");
        let mut state = AppState::new_for_tests(PathBuf::new());

        state.open_workspace(workspace.root_path());

        let id = state.project_id.clone().expect("debe existir identidad");
        assert_eq!(id.len(), 26, "debe ser un ULID");

        // Persistida dentro del proyecto, no derivada de la ruta.
        let en_disco = crate::project_id::load(&workspace.root_path());
        assert_eq!(en_disco.as_deref(), Some(id.as_str()));
        assert_ne!(
            id,
            workspace.root_path().display().to_string(),
            "la identidad no es la ruta"
        );
    }

    #[test]
    fn reabrir_el_mismo_proyecto_conserva_la_identidad() {
        let workspace = TestWorkspace::new("reabrir");

        let primero = {
            let mut state = AppState::new_for_tests(PathBuf::new());
            state.open_workspace(workspace.root_path());
            state.project_id.clone().expect("identidad")
        };

        let mut otra_sesion = AppState::new_for_tests(PathBuf::new());
        otra_sesion.open_workspace(workspace.root_path());

        assert_eq!(otra_sesion.project_id, Some(primero));
    }

    #[test]
    fn un_identificador_corrupto_impide_abrir_el_proyecto() {
        let workspace = TestWorkspace::new("corrupto");
        let ruta = crate::project_id::path_for(&workspace.root_path());
        fs::create_dir_all(ruta.parent().unwrap()).unwrap();
        fs::write(&ruta, "no-es-un-ulid").unwrap();

        let mut state = AppState::new_for_tests(PathBuf::new());
        state.open_workspace(workspace.root_path());

        assert!(!state.workspace_is_open(), "no debe abrirse");
        assert!(state.project_id.is_none());
        assert!(state.status_text.contains("no se puede abrir el proyecto"));
    }

    #[test]
    fn el_explorador_no_muestra_el_directorio_llore() {
        let workspace = TestWorkspace::new("oculta_llore");
        workspace.create_file("src/main.rs", "fn main() {}\n");

        let mut state = AppState::new_for_tests(PathBuf::new());
        state.open_workspace(workspace.root_path());

        let nombres: Vec<&str> = state
            .explorer_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert!(nombres.contains(&"src"), "{nombres:?}");
        assert!(!nombres.contains(&".llore"), "{nombres:?}");
    }

    #[test]
    fn abrir_una_carpeta_activa_el_proyecto_y_puebla_el_explorador() {
        let workspace = TestWorkspace::new("abrir_carpeta");
        workspace.create_file("src/main.rs", "fn main() {}\n");
        workspace.create_file(".env", "TOKEN=secreto\n");

        let mut state = AppState::new_for_tests(PathBuf::new());
        assert!(!state.workspace_is_open());

        state.open_workspace(workspace.root_path());

        assert!(state.workspace_is_open());
        assert!(!state.explorer_entries.is_empty());
        let nombres: Vec<&str> = state
            .explorer_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert!(!nombres.contains(&".env"), "{nombres:?}");
    }

    #[test]
    fn la_busqueda_no_recorre_secretos_ni_artefactos() {
        let workspace = TestWorkspace::new("busqueda_confinada");
        workspace.create_file("src/main.rs", "fn main() {}\n");
        workspace.create_file(".env", "TOKEN=secreto\n");
        workspace.create_file("target/debug/generado.rs", "fn generado() {}\n");

        let state = AppState::new_for_tests(workspace.root_path());
        let archivos = state.collect_workspace_files();

        let rutas: Vec<String> = archivos.iter().map(|p| p.display().to_string()).collect();
        assert!(rutas.iter().any(|r| r.ends_with("src/main.rs")), "{rutas:?}");
        assert!(!rutas.iter().any(|r| r.ends_with(".env")), "{rutas:?}");
        assert!(!rutas.iter().any(|r| r.contains("/target/")), "{rutas:?}");
    }

    #[test]
    fn dual_pane_workflow_open_edit_save_and_focus_navigation() {
        let workspace = TestWorkspace::new("dual_workflow");
        let left_file = workspace.create_file("left.rs", "fn left() {}\n");
        let right_file = workspace.create_file("right.rs", "fn right() {}\n");
        let mut state = AppState::new_for_tests(workspace.root_path());
        state.open_file_from_explorer(left_file.clone());
        state.open_file_to_side(right_file.clone());

        assert!(state.is_editor_split_active());
        assert_eq!(
            state.file_path_for_pane(EditorPane::Primary),
            Some(&left_file)
        );
        assert_eq!(
            state.file_path_for_pane(EditorPane::Secondary),
            Some(&right_file)
        );

        state.set_focus(FocusTarget::EditorPrimary);
        state.active_editor_mut().insert("// edited primary\n");
        state.save_active_file();
        let left_content = fs::read_to_string(&left_file).expect("must save primary file");
        assert!(left_content.starts_with("// edited primary\n"));

        state.toggle_focus();
        assert_eq!(state.focused_editor_pane, EditorPane::Secondary);
        state.active_editor_mut().insert("// edited secondary\n");
        state.save_active_file();
        let right_content = fs::read_to_string(&right_file).expect("must save secondary file");
        assert!(right_content.starts_with("// edited secondary\n"));

        state.toggle_focus();
        assert!(state.input_focused);
        state.toggle_focus();
        assert_eq!(state.focused_editor_pane, EditorPane::Primary);
    }
}
