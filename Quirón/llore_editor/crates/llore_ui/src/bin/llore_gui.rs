//! # Llore GUI
//!
//! Aplicación principal de Llore Editor con UI propia.
//!
//! > **Arquitectura**: UI local → llore_brain → gateway configurado.

use llore_ui::app::{
    run, top_menu_entries, AppState, ChatMessage, ClickTargetAction, CommandPaletteAction,
    EditorPane, OverlayMode, PanelDock, SearchInputFocus, SessionTelemetryTimelineSource,
    SidebarPanel, SidebarProblemSeverity, TelemetryTimelineFilter, TopMenuKind, UiAppearancePreset,
    UiDensity, EDITOR_BODY_BOTTOM_PADDING, EDITOR_BODY_TOP_PADDING, EDITOR_GUTTER_WIDTH,
    EDITOR_TAB_BAR_HEIGHT, OVERLAY_RESULTS_MAX, SEARCH_RESULTS_MAX_ROWS,
};
use llore_ui::design;
use llore_ui::icons;
use llore_ui::syntax_highlight::{highlight_line, syntax_diagnostic, SyntaxClass};
use llore_ui::theme::{ThemePalette, UiTheme};
use llore_ui::{Bounds, Canvas, Color, Window};
use std::path::{Path, PathBuf};

/// Anchura de la columna central de la pantalla de bienvenida.
const WELCOME_COLUMN_WIDTH: f32 = 420.0;
/// Alto de cada fila de estado o de proyecto reciente.
const WELCOME_ROW_HEIGHT: f32 = 26.0;

/// Tamaño de los iconos de la barra de actividad.
const ACTIVITY_ICON_SIZE: f32 = 16.0;
/// Tamaño de los iconos de acción del explorador.
const CONTROL_ICON_SIZE: f32 = 12.0;
/// Tamaño del icono que identifica a quien habla en el chat.
const ROLE_ICON_SIZE: f32 = 11.0;

/// Distancia entre el borde superior de la tarjeta y la base de la primera línea.
const CHAT_TEXT_TOP: f32 = 14.0;
/// Alto de línea del cuerpo del mensaje (`font_size * 1.2`, con `font_size = 12`).
const CHAT_LINE_HEIGHT: f32 = 14.4;
/// Alto de cada línea de detalle: la meta y cada cita.
const CHAT_DETAIL_LINE: f32 = 14.0;
/// Aire bajo el último elemento de la tarjeta.
const CHAT_CARD_PADDING_BOTTOM: f32 = 10.0;
/// Recorte del cuerpo del mensaje. Antes eran 84 caracteres: cortaba la respuesta
/// a media frase.
const CHAT_MESSAGE_MAX_CHARS: usize = 600;

const SEARCH_RESULTS_ROW_HEIGHT_BASE: f32 = 16.0;
const SEARCH_RESULTS_HEADER_HEIGHT_BASE: f32 = 16.0;
const SEARCH_RESULTS_PANEL_PADDING_TOP_BASE: f32 = 4.0;
const SEARCH_RESULTS_PANEL_PADDING_BOTTOM_BASE: f32 = 4.0;
const ACTIVITY_BAR_WIDTH: f32 = 40.0;
const ACTIVITY_BUTTON_HEIGHT_BASE: f32 = 24.0;
const ACTIVITY_BUTTON_GAP_BASE: f32 = 5.0;

fn theme_hex(palette: &ThemePalette, dark: u32, light: u32) -> Color {
    if palette.is_light {
        Color::from_hex(light)
    } else {
        Color::from_hex(dark)
    }
}

fn overlay_scrim_color(palette: &ThemePalette) -> Color {
    if palette.is_light {
        Color::from_hex(0x8c7b63).with_alpha(110)
    } else {
        Color::BLACK.with_alpha(170)
    }
}

fn syntax_color(class: SyntaxClass, palette: &ThemePalette) -> Color {
    match class {
        SyntaxClass::Plain => Color::from_hex(palette.text),
        SyntaxClass::Comment => Color::from_hex(palette.text_muted),
        SyntaxClass::String => Color::from_hex(palette.success),
        SyntaxClass::Number => Color::from_hex(palette.warning),
        SyntaxClass::Keyword => Color::from_hex(palette.accent),
        SyntaxClass::Type => Color::from_hex(palette.accent_alt),
        SyntaxClass::Function => Color::from_hex(palette.text),
        SyntaxClass::Macro => Color::from_hex(palette.error),
    }
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    LLORE EDITOR GUI                          ║");
    println!("║              Editor local y chat de proyecto                 ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Conectando al gateway local...                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Crear un runtime Tokio que persista durante la vida de la app
    // para que las llamadas zbus de rfd no hagan panic al faltar reactor
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime for main loop");

    let _enter = rt.enter();

    // Ejecutar aplicación
    run("Llore Editor", 1280, 720, |window, state| {
        render_app(window, state);
    });
}

fn render_app(window: &mut Window, state: &mut AppState) {
    let factor = state.ui_scale.factor();
    let canvas = window.canvas();
    canvas.set_scale(factor);

    // El lienzo mide píxeles físicos; el layout razona en unidades lógicas y el
    // lienzo las escala. Así la interfaz entera crece a la vez, letras y huecos,
    // sin tocar ninguna de las coordenadas del render.
    let bounds = Bounds::new(
        0.0,
        0.0,
        canvas.width() as f32 / factor,
        canvas.height() as f32 / factor,
    );

    let palette = state.theme_palette();
    let density_scale = state.ui_density_scale();
    let activity_button_height = (ACTIVITY_BUTTON_HEIGHT_BASE * density_scale).clamp(20.0, 34.0);
    let activity_button_gap = (ACTIVITY_BUTTON_GAP_BASE * density_scale).clamp(4.0, 12.0);
    let explorer_row_h = (16.0 * density_scale).clamp(14.0, 22.0);

    // Fondo base
    canvas.fill_rect(bounds, Color::from_hex(palette.background));

    // Barra superior compacta, alineada con editores de escritorio.
    let header_height = 36.0;
    canvas.fill_rect(
        Bounds::new(0.0, 0.0, bounds.width, header_height),
        Color::from_hex(palette.surface),
    );

    let status_height = 22.0;

    // Sidebar izquierda (resizable)
    let max_sidebar = (bounds.width * 0.45).max(200.0);
    let sidebar_width = state.sidebar_width.clamp(180.0, max_sidebar.max(180.0));
    state.sidebar_width = sidebar_width;
    let activity_bar_width = ACTIVITY_BAR_WIDTH.min((sidebar_width - 120.0).max(32.0));
    let sidebar_region_x = match state.explorer_dock {
        PanelDock::Left => 0.0,
        PanelDock::Right => bounds.width - sidebar_width,
    };
    let activity_bar_x = match state.explorer_dock {
        PanelDock::Left => sidebar_region_x,
        PanelDock::Right => bounds.width - activity_bar_width,
    };
    let sidebar_panel_x = match state.explorer_dock {
        PanelDock::Left => sidebar_region_x + activity_bar_width,
        PanelDock::Right => sidebar_region_x,
    };
    let sidebar_content_x = sidebar_panel_x + 10.0;
    let sidebar_content_width = (sidebar_width - activity_bar_width - 18.0).max(80.0);
    canvas.fill_rect(
        Bounds::new(
            activity_bar_x,
            header_height,
            activity_bar_width,
            bounds.height - header_height - status_height,
        ),
        Color::from_hex(palette.background),
    );
    canvas.fill_rect(
        Bounds::new(
            sidebar_panel_x,
            header_height,
            sidebar_width - activity_bar_width,
            bounds.height - header_height - status_height,
        ),
        Color::from_hex(palette.surface),
    );
    canvas.fill_rect(
        Bounds::new(
            if state.explorer_dock == PanelDock::Left {
                sidebar_panel_x
            } else {
                activity_bar_x
            },
            header_height,
            1.0,
            bounds.height - header_height - status_height,
        ),
        Color::from_hex(palette.border).with_alpha(120),
    );
    state.set_sidebar_bounds(Bounds::new(
        sidebar_panel_x,
        header_height,
        sidebar_width - activity_bar_width,
        bounds.height - header_height - status_height,
    ));
    let sidebar_resizer_bounds = Bounds::new(
        match state.explorer_dock {
            PanelDock::Left => sidebar_width - 2.0,
            PanelDock::Right => sidebar_region_x - 2.0,
        },
        header_height,
        4.0,
        bounds.height - header_height - status_height,
    );
    state.set_sidebar_resizer_bounds(sidebar_resizer_bounds);
    canvas.fill_rect(
        sidebar_resizer_bounds,
        if state.dragging_sidebar_resizer {
            Color::from_hex(palette.accent).with_alpha(140)
        } else {
            Color::from_hex(palette.border).with_alpha(90)
        },
    );

    // Status bar inferior
    canvas.fill_rect(
        Bounds::new(
            0.0,
            bounds.height - status_height,
            bounds.width,
            status_height,
        ),
        Color::from_hex(palette.surface),
    );

    // === Layout general ===
    let main_x = match state.explorer_dock {
        PanelDock::Left => sidebar_width,
        PanelDock::Right => 0.0,
    };
    let main_y = header_height;
    let main_width = (bounds.width - sidebar_width).max(240.0);
    let content_height = (bounds.height - header_height - status_height).max(120.0);

    let split_gap = 4.0;
    let split_available = (main_width - split_gap).max(220.0);
    let panel_min = 180.0_f32.min((split_available - 40.0).max(80.0));
    let max_editor = (split_available - panel_min).max(panel_min);
    let editor_width = (split_available * state.editor_split_ratio).clamp(panel_min, max_editor);
    let chat_width = (split_available - editor_width).max(panel_min);
    state.editor_split_ratio = (editor_width / split_available).clamp(0.1, 0.9);

    let (editor_region_bounds, chat_bounds) = match state.chat_dock {
        PanelDock::Left => (
            Bounds::new(
                main_x + chat_width + split_gap,
                main_y,
                editor_width,
                content_height,
            ),
            Bounds::new(main_x, main_y, chat_width, content_height),
        ),
        PanelDock::Right => (
            Bounds::new(main_x, main_y, editor_width, content_height),
            Bounds::new(
                main_x + editor_width + split_gap,
                main_y,
                chat_width,
                content_height,
            ),
        ),
    };
    let input_bounds = Bounds::new(
        chat_bounds.x + 8.0,
        chat_bounds.y + chat_bounds.height - 46.0,
        (chat_bounds.width - 16.0).max(80.0),
        38.0,
    );

    state.set_input_bounds(input_bounds);
    state.set_chat_bounds(chat_bounds);
    state.set_main_content_bounds(
        Bounds::new(main_x, main_y, main_width, content_height),
        split_gap,
    );
    let editor_resizer_bounds = Bounds::new(
        match state.chat_dock {
            PanelDock::Left => chat_bounds.x + chat_bounds.width,
            PanelDock::Right => editor_region_bounds.x + editor_region_bounds.width,
        },
        main_y,
        split_gap,
        content_height,
    );
    state.set_editor_resizer_bounds(editor_resizer_bounds);

    let pane_gap = 10.0;
    let split_editors = state.is_editor_split_active();
    let (primary_editor_bounds, secondary_editor_bounds, pane_resizer_bounds) = if split_editors {
        let available = (editor_region_bounds.width - pane_gap).max(220.0);
        let pane_min = 140.0_f32.min((available - 40.0).max(80.0));
        let pane_max = (available - pane_min).max(pane_min);
        let left_width = (available * state.editor_pane_split_ratio).clamp(pane_min, pane_max);
        let right_width = (available - left_width).max(pane_min);
        state.editor_pane_split_ratio = (left_width / available).clamp(0.2, 0.8);

        let left = Bounds::new(
            editor_region_bounds.x,
            editor_region_bounds.y,
            left_width,
            editor_region_bounds.height,
        );
        let right = Bounds::new(
            left.x + left.width + pane_gap,
            editor_region_bounds.y,
            right_width,
            editor_region_bounds.height,
        );
        let handle = Bounds::new(left.x + left.width, left.y, pane_gap, left.height);
        (left, Some(right), Some(handle))
    } else {
        (editor_region_bounds, None, None)
    };

    state.set_editor_primary_bounds(primary_editor_bounds);
    state.set_editor_secondary_bounds(secondary_editor_bounds);
    state.set_editor_pane_resizer_bounds(pane_resizer_bounds);
    state.clear_search_results_bounds();

    // === Paneles ===
    let editor_bg = Color::from_hex(palette.surface);
    let chat_bg = Color::from_hex(palette.surface);
    canvas.fill_rect(editor_region_bounds, editor_bg);
    canvas.fill_rect(chat_bounds, chat_bg);

    // We remove the hard 1.0px strokes around all primary/secondary panes and the chat pane
    // to give a clean, borderless Antigravity look.
    // Instead, we just draw a very subtle dividing line if split.
    if let Some(secondary_bounds) = secondary_editor_bounds {
        canvas.draw_line(
            secondary_bounds.x - (pane_gap / 2.0),
            secondary_bounds.y,
            secondary_bounds.x - (pane_gap / 2.0),
            secondary_bounds.y + secondary_bounds.height,
            Color::from_hex(palette.border).with_alpha(100),
            1.0,
        );
    }
    canvas.fill_rect(
        editor_resizer_bounds,
        if state.dragging_editor_resizer {
            Color::from_hex(palette.accent).with_alpha(120)
        } else {
            Color::from_hex(palette.border).with_alpha(70)
        },
    );
    if let Some(pane_resizer_bounds) = pane_resizer_bounds {
        canvas.fill_rect(
            pane_resizer_bounds,
            if state.dragging_editor_pane_resizer {
                Color::from_hex(palette.accent).with_alpha(120)
            } else {
                Color::from_hex(palette.border).with_alpha(70)
            },
        );
    }

    // Input chat (modern pill shape)
    let input_bg = if state.input_focused {
        Color::from_hex(palette.surface)
    } else {
        Color::from_hex(palette.background)
    };
    canvas.fill_rounded_rect(input_bounds, 8.0, input_bg);
    if state.input_focused {
        canvas.stroke_rect(input_bounds, Color::from_hex(palette.accent), 1.0);
    }

    // === Menú superior estilo editor ===
    let menu_row_y = 10.0;
    let menu_row_h = 24.0;
    let menu_items = [
        TopMenuKind::File,
        TopMenuKind::Edit,
        TopMenuKind::View,
        TopMenuKind::Go,
        TopMenuKind::Project,
        TopMenuKind::Help,
    ];
    let mut menu_x = 12.0;
    let mut menu_layout: Vec<(TopMenuKind, Bounds)> = Vec::new();
    for menu in menu_items {
        let label = menu.label();
        let item_w = (label.chars().count() as f32 * 8.5 + 16.0).clamp(40.0, 90.0);
        let item_bounds = Bounds::new(menu_x, menu_row_y, item_w, menu_row_h);
        let active = state.top_menu_open() == Some(menu);
        if active {
            canvas.fill_rounded_rect(
                item_bounds,
                4.0,
                Color::from_hex(palette.selection).with_alpha(150),
            );
        }
        let item_buf = state.text_system.create_line_buffer(label, design::type_scale::SM, item_w - 8.0);
        state.text_system.draw_buffer(
            canvas,
            &item_buf,
            menu_x + 5.0,
            menu_row_y + 12.0,
            if active {
                Color::from_hex(palette.text)
            } else {
                Color::from_hex(palette.text_muted)
            },
        );
        state.add_click_target(item_bounds, ClickTargetAction::TopMenuToggle(menu));
        menu_layout.push((menu, item_bounds));
        menu_x += item_w + 5.0;
    }

    // === Título central discreto ===
    let ws_name = state
        .workspace_root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("Sin proyecto");
    let window_title = format!("{} — Llore", ws_name);
    let title_width = 240.0;
    let workspace_buf = state
        .text_system
        .create_line_buffer(&window_title, design::type_scale::SM, title_width);
    state.text_system.draw_buffer(
        canvas,
        &workspace_buf,
        (bounds.width - title_width) * 0.5,
        22.0,
        Color::from_hex(palette.text_muted),
    );

    let sidebar_title_label = match state.sidebar_panel {
        SidebarPanel::Explorer => "EXPLORER",
        SidebarPanel::Search => "SEARCH",
        SidebarPanel::Git => "SOURCE CONTROL",
        SidebarPanel::Problems => "PROBLEMS",
        SidebarPanel::Outline => "OUTLINE",
        SidebarPanel::Appearance => "APPEARANCE",
        SidebarPanel::Security => "SECURITY",
    };
    let sidebar_title =
        state
            .text_system
            .create_line_buffer(sidebar_title_label, design::type_scale::SM, sidebar_content_width);

    let activity_items = vec![
        (
            icons::EXPLORER,
            state.sidebar_panel == SidebarPanel::Explorer,
            ClickTargetAction::ActivityFocusExplorer,
        ),
        (
            icons::SEARCH,
            state.sidebar_panel == SidebarPanel::Search,
            ClickTargetAction::ActivitySidebarSearch,
        ),
        (
            icons::GIT,
            state.sidebar_panel == SidebarPanel::Git,
            ClickTargetAction::ActivitySidebarGit,
        ),
        (
            icons::PROBLEMS,
            state.sidebar_panel == SidebarPanel::Problems,
            ClickTargetAction::ActivitySidebarProblems,
        ),
        (
            icons::OUTLINE,
            state.sidebar_panel == SidebarPanel::Outline,
            ClickTargetAction::ActivitySidebarOutline,
        ),
        (
            icons::APPEARANCE,
            state.sidebar_panel == SidebarPanel::Appearance,
            ClickTargetAction::ActivitySidebarAppearance,
        ),
        (
            icons::SECURITY,
            state.sidebar_panel == SidebarPanel::Security,
            ClickTargetAction::ActivitySidebarSecurity,
        ),
        (
            icons::FOLDER_OPEN,
            state.overlay_mode == Some(OverlayMode::QuickOpen),
            ClickTargetAction::ActivityQuickOpen,
        ),
        (
            icons::COMMANDS,
            state.overlay_mode == Some(OverlayMode::CommandPalette),
            ClickTargetAction::ActivityCommandPalette,
        ),
    ];
    for (idx, (icon, active, action)) in activity_items.into_iter().enumerate() {
        let btn_y =
            header_height + 12.0 + idx as f32 * (activity_button_height + activity_button_gap);
        let btn_bounds = Bounds::new(
            activity_bar_x + 6.0,
            btn_y,
            activity_bar_width - 12.0,
            activity_button_height,
        );
        if active {
            canvas.fill_rounded_rect(btn_bounds, 5.0, Color::from_hex(palette.selection));
            canvas.draw_line(
                btn_bounds.x,
                btn_bounds.y + 4.0,
                btn_bounds.x,
                btn_bounds.y + btn_bounds.height - 4.0,
                Color::from_hex(palette.accent),
                2.0,
            );
        }
        let icon_buffer = state.text_system.create_icon_buffer(icon, ACTIVITY_ICON_SIZE);
        state.text_system.draw_buffer(
            canvas,
            &icon_buffer,
            btn_bounds.x + (btn_bounds.width - ACTIVITY_ICON_SIZE) * 0.5,
            btn_bounds.y + (btn_bounds.height + ACTIVITY_ICON_SIZE) * 0.5 - 1.0,
            if active {
                Color::from_hex(palette.text)
            } else {
                Color::from_hex(palette.text_muted)
            },
        );
        state.add_click_target(btn_bounds, action);
    }
    state.text_system.draw_buffer(
        canvas,
        &sidebar_title,
        sidebar_content_x,
        header_height + 24.0,
        Color::from_hex(palette.text_muted),
    );
    // La barra vertical ya selecciona la vista; evitamos una segunda navegación duplicada.
    let explorer_start_y = header_height + 38.0;
    let explorer_end_y = bounds.height - status_height - 8.0;
    match state.sidebar_panel {
        SidebarPanel::Explorer => {
            let controls_h = (explorer_row_h + 1.0).clamp(18.0, 24.0);
            let controls = [
                (icons::NEW_FILE, ClickTargetAction::ActivityExplorerNewFile),
                (
                    icons::NEW_FOLDER,
                    ClickTargetAction::ActivityExplorerNewFolder,
                ),
                (icons::REFRESH, ClickTargetAction::ActivityRefreshExplorer),
            ];
            let control_w = 24.0;
            let controls_width =
                controls.len() as f32 * control_w + controls.len().saturating_sub(1) as f32 * 3.0;
            let controls_x = sidebar_content_x + sidebar_content_width - controls_width;
            for (idx, (icon, action)) in controls.into_iter().enumerate() {
                let x = controls_x + idx as f32 * (control_w + 3.0);
                let row_bounds = Bounds::new(x, explorer_start_y, control_w, controls_h);
                canvas.fill_rounded_rect(
                    row_bounds,
                    3.0,
                    Color::from_hex(palette.background).with_alpha(100),
                );
                let icon_buffer = state.text_system.create_icon_buffer(icon, CONTROL_ICON_SIZE);
                state.text_system.draw_buffer(
                    canvas,
                    &icon_buffer,
                    x + (control_w - CONTROL_ICON_SIZE) * 0.5,
                    explorer_start_y + (controls_h + CONTROL_ICON_SIZE) * 0.5 - 1.0,
                    Color::from_hex(palette.text_muted),
                );
                state.add_click_target(row_bounds, action);
            }

            let list_start_y = explorer_start_y + controls_h + 6.0;
            let max_visible_entries = ((explorer_end_y - list_start_y) / explorer_row_h)
                .floor()
                .max(0.0) as usize;
            let start_idx = state.explorer_scroll.min(state.explorer_entries.len());
            let end_idx = (start_idx + max_visible_entries).min(state.explorer_entries.len());

            let visible_entries = state
                .explorer_entries
                .iter()
                .skip(start_idx)
                .take(end_idx.saturating_sub(start_idx))
                .cloned()
                .collect::<Vec<_>>();
            for (idx, entry) in visible_entries.into_iter().enumerate() {
                let y = list_start_y + idx as f32 * explorer_row_h;
                let row_bounds = Bounds::new(
                    sidebar_content_x,
                    y - 10.0,
                    sidebar_content_width,
                    explorer_row_h,
                );
                if (start_idx + idx) % 2 == 1 {
                    canvas.fill_rect(row_bounds, Color::from_hex(palette.text).with_alpha(5));
                }

                let indent_step = state.explorer_indent_step();
                let indent = sidebar_content_x + 6.0 + entry.depth as f32 * indent_step;
                // Removed the vertical indent guide lines to clean up the look
                let icon = if entry.is_dir {
                    if state.is_dir_expanded(&entry.path) {
                        "v"
                    } else {
                        ">"
                    }
                } else {
                    "-"
                };
                let label = format!("{} {}", icon, entry.name);
                let x = indent;
                let width = (sidebar_content_x + sidebar_content_width - x - 4.0).max(28.0);
                let is_primary_active = state
                    .file_path_for_pane(EditorPane::Primary)
                    .map(|p| p == &entry.path)
                    .unwrap_or(false);
                let is_secondary_active = state.is_editor_split_active()
                    && state
                        .file_path_for_pane(EditorPane::Secondary)
                        .map(|p| p == &entry.path)
                        .unwrap_or(false);
                let is_selected = state
                    .explorer_selected_path
                    .as_ref()
                    .map(|p| p == &entry.path)
                    .unwrap_or(false);

                if is_selected {
                    canvas.fill_rounded_rect(row_bounds, 4.0, Color::from_hex(palette.selection));
                }

                if (is_primary_active || is_secondary_active) && !entry.is_dir {
                    canvas.fill_rounded_rect(
                        row_bounds,
                        4.0,
                        if is_primary_active && is_secondary_active {
                            Color::from_hex(palette.selection).with_alpha(150)
                        } else if is_primary_active {
                            Color::from_hex(palette.selection)
                        } else {
                            Color::from_hex(palette.selection).with_alpha(100)
                        },
                    );
                }

                let color = if entry.is_dir {
                    Color::from_hex(palette.text_muted)
                } else {
                    Color::from_hex(palette.text)
                };
                let text_buf = state.text_system.create_line_buffer(&label, design::type_scale::SM, width);
                state
                    .text_system
                    .draw_buffer(canvas, &text_buf, x, y, color);

                if entry.is_dir {
                    state.add_click_target(row_bounds, ClickTargetAction::ExplorerDir(entry.path));
                } else {
                    state.add_click_target(row_bounds, ClickTargetAction::ExplorerFile(entry.path));
                }
            }
        }
        SidebarPanel::Search => {
            let search_active = state.search_active;
            let search_query = state.search_query.clone();
            let search_matches = state.search_matches.clone();
            let active_match = state.active_search_match;
            let editor_lines: Vec<String> = state
                .active_editor()
                .text()
                .split('\n')
                .map(|line| line.to_string())
                .collect();

            let header = if !search_active {
                "Find in editor desactivado (Ctrl+F)".to_string()
            } else if search_query.is_empty() {
                "Find activo: escribe query en Ctrl+F".to_string()
            } else {
                format!(
                    "query='{}' matches={}",
                    truncate_chars(&search_query, 24),
                    search_matches.len()
                )
            };
            let header_buf = state
                .text_system
                .create_line_buffer(&header, design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &header_buf,
                sidebar_content_x,
                explorer_start_y + 4.0,
                Color::from_hex(palette.text_muted),
            );

            let list_start_y = explorer_start_y + 20.0;
            let visible_rows = ((explorer_end_y - list_start_y) / explorer_row_h)
                .floor()
                .max(0.0) as usize;

            if search_matches.is_empty() {
                let empty_buf = state.text_system.create_line_buffer(
                    "Sin resultados. Usa Ctrl+F y Enter/Shift+Enter.", design::type_scale::SM,
                    sidebar_content_width,
                );
                state.text_system.draw_buffer(
                    canvas,
                    &empty_buf,
                    sidebar_content_x,
                    list_start_y + 14.0,
                    Color::from_hex(palette.text_muted),
                );
            } else {
                let max_scroll = search_matches.len().saturating_sub(visible_rows.max(1));
                let start = state.sidebar_search_scroll.min(max_scroll);
                state.sidebar_search_scroll = start;

                for (offset, (index, matched)) in search_matches
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(visible_rows)
                    .enumerate()
                {
                    let y = list_start_y + offset as f32 * explorer_row_h;
                    let row_bounds = Bounds::new(
                        sidebar_content_x,
                        y - 10.0,
                        sidebar_content_width,
                        explorer_row_h,
                    );
                    if active_match == Some(index) {
                        canvas.fill_rounded_rect(
                            row_bounds,
                            4.0,
                            Color::from_hex(palette.selection),
                        );
                    }

                    let preview_source = editor_lines
                        .get(matched.start_line)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let preview = preview_source
                        .chars()
                        .skip(matched.start_col)
                        .take(28)
                        .collect::<String>();
                    let label = format!(
                        "L{}:{} {}",
                        matched.start_line + 1,
                        matched.start_col + 1,
                        truncate_chars(&preview, 28)
                    );
                    let row_buf =
                        state
                            .text_system
                            .create_line_buffer(&label, design::type_scale::SM, sidebar_content_width - 4.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &row_buf,
                        sidebar_content_x + 2.0,
                        y,
                        if active_match == Some(index) {
                            Color::from_hex(palette.text)
                        } else {
                            Color::from_hex(palette.text_muted)
                        },
                    );
                    state.add_click_target(row_bounds, ClickTargetAction::SearchMatchSelect(index));
                }
            }
        }
        SidebarPanel::Git => {
            if let Some(status) = &state.git_sidebar_status {
                let line1 = format!(
                    "branch={} {}{}",
                    status.branch,
                    if status.ahead > 0 {
                        format!("ahead {}", status.ahead)
                    } else {
                        String::new()
                    },
                    if status.behind > 0 {
                        format!(" behind {}", status.behind)
                    } else {
                        String::new()
                    }
                );
                let line2 = format!(
                    "mod={} add={} del={} untracked={} conflict={}",
                    status.modified,
                    status.added,
                    status.deleted,
                    status.untracked,
                    status.conflicted
                );
                let line3 = format!("total changes={}", status.total);
                let line1_buf =
                    state
                        .text_system
                        .create_line_buffer(&line1, design::type_scale::SM, sidebar_content_width);
                let line2_buf =
                    state
                        .text_system
                        .create_line_buffer(&line2, design::type_scale::SM, sidebar_content_width);
                let line3_buf =
                    state
                        .text_system
                        .create_line_buffer(&line3, design::type_scale::SM, sidebar_content_width);
                state.text_system.draw_buffer(
                    canvas,
                    &line1_buf,
                    sidebar_content_x,
                    explorer_start_y + 14.0,
                    Color::from_hex(palette.text),
                );
                state.text_system.draw_buffer(
                    canvas,
                    &line2_buf,
                    sidebar_content_x,
                    explorer_start_y + 30.0,
                    Color::from_hex(palette.text_muted),
                );
                state.text_system.draw_buffer(
                    canvas,
                    &line3_buf,
                    sidebar_content_x,
                    explorer_start_y + 46.0,
                    Color::from_hex(palette.text_muted),
                );
            } else if let Some(err) = state.git_sidebar_error.as_deref() {
                let err_line = format!("git unavailable: {}", truncate_chars(err, 48));
                let err_buf =
                    state
                        .text_system
                        .create_line_buffer(&err_line, design::type_scale::SM, sidebar_content_width);
                state.text_system.draw_buffer(
                    canvas,
                    &err_buf,
                    sidebar_content_x,
                    explorer_start_y + 14.0,
                    Color::from_hex(palette.accent),
                );
            } else {
                let loading_buf = state.text_system.create_line_buffer(
                    "leyendo estado git...", design::type_scale::SM,
                    sidebar_content_width,
                );
                state.text_system.draw_buffer(
                    canvas,
                    &loading_buf,
                    sidebar_content_x,
                    explorer_start_y + 14.0,
                    Color::from_hex(palette.text_muted),
                );
            }
        }
        SidebarPanel::Problems => {
            let problems = state.sidebar_problems_snapshot();
            let list_start_y = explorer_start_y + 4.0;
            let visible_rows = ((explorer_end_y - list_start_y) / explorer_row_h)
                .floor()
                .max(0.0) as usize;
            let max_scroll = problems.len().saturating_sub(visible_rows.max(1));
            let start = state.sidebar_problems_scroll.min(max_scroll);
            state.sidebar_problems_scroll = start;
            let active_problem = state.active_problem_index();

            for (offset, problem) in problems.iter().skip(start).take(visible_rows).enumerate() {
                let index = start + offset;
                let y = list_start_y + offset as f32 * explorer_row_h;
                let row_bounds = Bounds::new(
                    sidebar_content_x,
                    y - 10.0,
                    sidebar_content_width,
                    explorer_row_h,
                );
                if active_problem == Some(index) {
                    canvas.fill_rounded_rect(row_bounds, 4.0, Color::from_hex(palette.selection));
                }
                let marker = match problem.severity {
                    SidebarProblemSeverity::Error => "ERR",
                    SidebarProblemSeverity::Warning => "WRN",
                    SidebarProblemSeverity::Info => "INF",
                };
                let ctx = AppState::sidebar_problem_context_label(problem).to_uppercase();
                let line = format!(
                    "[{}][{}] {}: {}",
                    ctx,
                    marker,
                    problem.title,
                    truncate_chars(&problem.detail, 40)
                );
                let row_buf = state
                    .text_system
                    .create_line_buffer(&line, design::type_scale::SM, sidebar_content_width);
                let color = match problem.severity {
                    SidebarProblemSeverity::Error => Color::from_hex(palette.error),
                    SidebarProblemSeverity::Warning => Color::from_hex(palette.warning),
                    SidebarProblemSeverity::Info => {
                        if active_problem == Some(index) {
                            Color::from_hex(palette.text)
                        } else {
                            Color::from_hex(palette.text_muted)
                        }
                    }
                };
                state
                    .text_system
                    .draw_buffer(canvas, &row_buf, sidebar_content_x, y, color);
                state.add_click_target(row_bounds, ClickTargetAction::SidebarProblemSelect(index));
            }
        }
        SidebarPanel::Outline => {
            let pane = state.focused_editor_pane;
            let outline = state.sidebar_outline_snapshot(pane);
            let list_start_y = explorer_start_y + 4.0;
            let visible_rows = ((explorer_end_y - list_start_y) / explorer_row_h)
                .floor()
                .max(0.0) as usize;

            if outline.is_empty() {
                let empty_buf = state.text_system.create_line_buffer(
                    "No symbols detected in active file", design::type_scale::SM,
                    sidebar_content_width,
                );
                state.text_system.draw_buffer(
                    canvas,
                    &empty_buf,
                    sidebar_content_x,
                    list_start_y + 14.0,
                    Color::from_hex(palette.text_muted),
                );
            } else {
                let max_scroll = outline.len().saturating_sub(visible_rows.max(1));
                let start = state.sidebar_outline_scroll.min(max_scroll);
                state.sidebar_outline_scroll = start;
                let cursor_line = state.editor_for_pane(pane).cursor().line;

                for (offset, item) in outline.iter().skip(start).take(visible_rows).enumerate() {
                    let y = list_start_y + offset as f32 * explorer_row_h;
                    let row_bounds = Bounds::new(
                        sidebar_content_x,
                        y - 10.0,
                        sidebar_content_width,
                        explorer_row_h,
                    );
                    if item.line == cursor_line {
                        canvas.fill_rounded_rect(
                            row_bounds,
                            4.0,
                            Color::from_hex(palette.selection),
                        );
                    }

                    let line = format!(
                        "[{}] L{} {}",
                        item.kind,
                        item.line + 1,
                        truncate_chars(&item.label, 28)
                    );
                    let row_buf =
                        state
                            .text_system
                            .create_line_buffer(&line, design::type_scale::SM, sidebar_content_width - 2.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &row_buf,
                        sidebar_content_x + 1.0,
                        y,
                        if item.line == cursor_line {
                            Color::from_hex(palette.text)
                        } else {
                            Color::from_hex(palette.text_muted)
                        },
                    );
                    state.add_click_target(
                        row_bounds,
                        ClickTargetAction::OutlineSelect {
                            pane,
                            line: item.line,
                            column: item.column,
                        },
                    );
                }
            }
        }
        SidebarPanel::Appearance => {
            let section_gap = (8.0 * density_scale).clamp(6.0, 12.0);
            let section_label_h = (14.0 * density_scale).clamp(12.0, 18.0);
            let row_h = (20.0 * density_scale).clamp(18.0, 28.0);
            let row_gap = (6.0 * density_scale).clamp(4.0, 10.0);
            let mut y = explorer_start_y + 4.0;

            let theme_title = state
                .text_system
                .create_line_buffer("Theme", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &theme_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;
            let theme_rows = [
                ("Quiron Dark", UiTheme::QuironDark),
                ("Graphite Dark", UiTheme::GraphiteDark),
                ("Copper Light", UiTheme::CopperLight),
            ];
            for (label, theme) in theme_rows {
                let row_bounds = Bounds::new(sidebar_content_x, y, sidebar_content_width, row_h);
                let active = state.ui_theme() == theme;
                if active {
                    canvas.fill_rect(row_bounds, Color::from_hex(palette.selection));
                }
                canvas.stroke_rect(row_bounds, Color::from_hex(palette.border), 1.0);
                let row_buf =
                    state
                        .text_system
                        .create_line_buffer(label, design::type_scale::SM, sidebar_content_width - 12.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    sidebar_content_x + 6.0,
                    y + row_h * 0.65,
                    if active {
                        Color::from_hex(palette.text)
                    } else {
                        Color::from_hex(palette.text_muted)
                    },
                );
                state.add_click_target(row_bounds, ClickTargetAction::ActivitySetTheme(theme));
                y += row_h + row_gap;
            }
            y += section_gap;

            let density_title =
                state
                    .text_system
                    .create_line_buffer("Density", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &density_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;
            let density_rows = [
                ("Compact", UiDensity::Compact),
                ("Normal", UiDensity::Normal),
                ("Comfortable", UiDensity::Comfortable),
            ];
            for (label, density) in density_rows {
                let row_bounds = Bounds::new(sidebar_content_x, y, sidebar_content_width, row_h);
                let active = state.ui_density() == density;
                if active {
                    canvas.fill_rect(row_bounds, Color::from_hex(palette.selection));
                }
                canvas.stroke_rect(row_bounds, Color::from_hex(palette.border), 1.0);
                let row_buf =
                    state
                        .text_system
                        .create_line_buffer(label, design::type_scale::SM, sidebar_content_width - 12.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    sidebar_content_x + 6.0,
                    y + row_h * 0.65,
                    if active {
                        Color::from_hex(palette.text)
                    } else {
                        Color::from_hex(palette.text_muted)
                    },
                );
                state.add_click_target(row_bounds, ClickTargetAction::ActivitySetDensity(density));
                y += row_h + row_gap;
            }
            y += section_gap;

            let preset_title =
                state
                    .text_system
                    .create_line_buffer("Presets", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &preset_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;
            let preset_rows = [
                (
                    "Dev",
                    UiAppearancePreset::Dev,
                    UiTheme::GraphiteDark,
                    UiDensity::Compact,
                    0.95_f32,
                ),
                (
                    "Focus",
                    UiAppearancePreset::Focus,
                    UiTheme::QuironDark,
                    UiDensity::Normal,
                    1.10_f32,
                ),
                (
                    "Reading",
                    UiAppearancePreset::Reading,
                    UiTheme::CopperLight,
                    UiDensity::Comfortable,
                    1.20_f32,
                ),
            ];
            for (label, preset, preset_theme, preset_density, preset_font) in preset_rows {
                let row_bounds = Bounds::new(sidebar_content_x, y, sidebar_content_width, row_h);
                let active = state.ui_theme() == preset_theme
                    && state.ui_density() == preset_density
                    && (state.editor_font_scale() - preset_font).abs() < 0.02;
                if active {
                    canvas.fill_rect(row_bounds, Color::from_hex(palette.selection));
                }
                // Removed canvas.stroke_rect
                let row_buf =
                    state
                        .text_system
                        .create_line_buffer(label, design::type_scale::SM, sidebar_content_width - 12.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    sidebar_content_x + 6.0,
                    y + row_h * 0.65,
                    if active {
                        Color::from_hex(palette.text)
                    } else {
                        Color::from_hex(palette.text_muted)
                    },
                );
                state.add_click_target(
                    row_bounds,
                    ClickTargetAction::ActivityApplyAppearancePreset(preset),
                );
                y += row_h + row_gap;
            }
            y += section_gap;

            let font_title =
                state
                    .text_system
                    .create_line_buffer("Editor Font", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &font_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;

            let control_w = ((sidebar_content_width - 8.0) / 3.0).max(34.0);
            let controls = [
                ("-", ClickTargetAction::ActivityDecreaseEditorFontScale),
                ("+", ClickTargetAction::ActivityIncreaseEditorFontScale),
                ("100%", ClickTargetAction::ActivityResetEditorFontScale),
            ];
            for (idx, (label, action)) in controls.into_iter().enumerate() {
                let x = sidebar_content_x + idx as f32 * (control_w + 4.0);
                let row_bounds = Bounds::new(x, y, control_w, row_h);
                canvas.fill_rect(row_bounds, Color::from_hex(palette.surface));
                canvas.stroke_rect(row_bounds, Color::from_hex(palette.border), 1.0);
                let row_buf = state
                    .text_system
                    .create_line_buffer(label, design::type_scale::SM, control_w - 8.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    x + 6.0,
                    y + row_h * 0.65,
                    Color::from_hex(palette.text),
                );
                state.add_click_target(row_bounds, action);
            }
            y += row_h + row_gap;

            let font_meta = format!(
                "Font scale: {:.0}% | density={} | Ctrl+Alt+D cycle",
                state.editor_font_scale() * 100.0,
                state.ui_density().config_value()
            );
            let font_meta_buf =
                state
                    .text_system
                    .create_line_buffer(&font_meta, design::type_scale::XS, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &font_meta_buf,
                sidebar_content_x,
                y + 12.0,
                Color::from_hex(palette.text_muted),
            );
            y += 20.0;

            let hpad_title =
                state
                    .text_system
                    .create_line_buffer("Editor Left Padding", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &hpad_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;
            let hpad_controls = [
                (
                    "-",
                    ClickTargetAction::ActivityDecreaseEditorHorizontalPadding,
                ),
                (
                    "+",
                    ClickTargetAction::ActivityIncreaseEditorHorizontalPadding,
                ),
                (
                    "Default",
                    ClickTargetAction::ActivityResetEditorHorizontalPadding,
                ),
            ];
            for (idx, (label, action)) in hpad_controls.into_iter().enumerate() {
                let x = sidebar_content_x + idx as f32 * (control_w + 4.0);
                let row_bounds = Bounds::new(x, y, control_w, row_h);
                canvas.fill_rect(row_bounds, Color::from_hex(palette.surface));
                canvas.stroke_rect(row_bounds, Color::from_hex(palette.border), 1.0);
                let row_buf = state
                    .text_system
                    .create_line_buffer(label, design::type_scale::SM, control_w - 8.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    x + 6.0,
                    y + row_h * 0.65,
                    Color::from_hex(palette.text),
                );
                state.add_click_target(row_bounds, action);
            }
            y += row_h + row_gap;
            let hpad_meta = format!("Left padding: {:.0}px", state.editor_horizontal_padding());
            let hpad_meta_buf =
                state
                    .text_system
                    .create_line_buffer(&hpad_meta, design::type_scale::XS, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &hpad_meta_buf,
                sidebar_content_x,
                y + 12.0,
                Color::from_hex(palette.text_muted),
            );
            y += 20.0;

            let indent_title =
                state
                    .text_system
                    .create_line_buffer("Explorer Indent", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &indent_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;
            let indent_controls = [
                ("-", ClickTargetAction::ActivityDecreaseExplorerIndent),
                ("+", ClickTargetAction::ActivityIncreaseExplorerIndent),
                ("Default", ClickTargetAction::ActivityResetExplorerIndent),
            ];
            for (idx, (label, action)) in indent_controls.into_iter().enumerate() {
                let x = sidebar_content_x + idx as f32 * (control_w + 4.0);
                let row_bounds = Bounds::new(x, y, control_w, row_h);
                canvas.fill_rect(row_bounds, Color::from_hex(palette.surface));
                canvas.stroke_rect(row_bounds, Color::from_hex(palette.border), 1.0);
                let row_buf = state
                    .text_system
                    .create_line_buffer(label, design::type_scale::SM, control_w - 8.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    x + 6.0,
                    y + row_h * 0.65,
                    Color::from_hex(palette.text),
                );
                state.add_click_target(row_bounds, action);
            }
            y += row_h + row_gap;
            let indent_meta = format!("Explorer indent: {:.0}px", state.explorer_indent_step());
            let indent_meta_buf =
                state
                    .text_system
                    .create_line_buffer(&indent_meta, design::type_scale::XS, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &indent_meta_buf,
                sidebar_content_x,
                y + 12.0,
                Color::from_hex(palette.text_muted),
            );

            let reset_y = (y + 24.0).min(explorer_end_y - row_h - 2.0);
            let reset_bounds =
                Bounds::new(sidebar_content_x, reset_y, sidebar_content_width, row_h);
            canvas.fill_rect(reset_bounds, Color::from_hex(palette.surface));
            canvas.stroke_rect(reset_bounds, Color::from_hex(palette.accent), 1.0);
            let reset_buf = state.text_system.create_line_buffer(
                "Reset Appearance", design::type_scale::SM,
                sidebar_content_width - 10.0,
            );
            state.text_system.draw_buffer(
                canvas,
                &reset_buf,
                sidebar_content_x + 6.0,
                reset_y + row_h * 0.65,
                Color::from_hex(palette.accent),
            );
            state.add_click_target(reset_bounds, ClickTargetAction::ActivityResetAppearance);
        }
        SidebarPanel::Security => {
            let section_gap = (8.0 * density_scale).clamp(6.0, 12.0);
            let section_label_h = (14.0 * density_scale).clamp(12.0, 18.0);
            let row_h = (20.0 * density_scale).clamp(18.0, 28.0);
            let row_gap = (6.0 * density_scale).clamp(4.0, 10.0);
            let mut y = explorer_start_y + 4.0;

            let title = state.text_system.create_line_buffer(
                "Passwords & Secure Connection", design::type_scale::SM,
                sidebar_content_width,
            );
            state.text_system.draw_buffer(
                canvas,
                &title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;

            let mind_line = state.text_system.create_line_buffer(
                "All model traffic -> local gateway", design::type_scale::XS,
                sidebar_content_width,
            );
            state.text_system.draw_buffer(
                canvas,
                &mind_line,
                sidebar_content_x,
                y + 12.0,
                Color::from_hex(palette.accent),
            );
            y += 18.0;

            let endpoint = format!(
                "Endpoint: {}",
                truncate_chars(&state.quiron_connection_endpoint_label(), 40)
            );
            let mode = format!("Mode: {}", state.quiron_connection_mode_label());
            let health = format!("Status: {}", state.quiron_connection_health_label());
            let status_lines = [endpoint, mode, health];
            for line in status_lines {
                let line_buf = state
                    .text_system
                    .create_line_buffer(&line, design::type_scale::XS, sidebar_content_width);
                state.text_system.draw_buffer(
                    canvas,
                    &line_buf,
                    sidebar_content_x,
                    y + 11.0,
                    Color::from_hex(palette.text),
                );
                y += 14.0;
            }
            y += section_gap;

            let controls_title =
                state
                    .text_system
                    .create_line_buffer("Controls", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &controls_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;

            let controls = [
                (
                    "Post Status",
                    ClickTargetAction::TopMenuExecute(
                        CommandPaletteAction::ShowQuironConnectionStatus,
                    ),
                ),
                (
                    "Reconnect Local",
                    ClickTargetAction::TopMenuExecute(
                        CommandPaletteAction::ReconnectQuironSecureLocal,
                    ),
                ),
                (
                    "Reconnect Env URL",
                    ClickTargetAction::TopMenuExecute(
                        CommandPaletteAction::ReconnectQuironSecureEnv,
                    ),
                ),
            ];
            for (label, action) in controls {
                let row_bounds = Bounds::new(sidebar_content_x, y, sidebar_content_width, row_h);
                canvas.fill_rect(row_bounds, Color::from_hex(palette.surface));
                canvas.stroke_rect(row_bounds, Color::from_hex(palette.border), 1.0);
                let row_buf =
                    state
                        .text_system
                        .create_line_buffer(label, design::type_scale::SM, sidebar_content_width - 12.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    sidebar_content_x + 6.0,
                    y + row_h * 0.65,
                    Color::from_hex(palette.text),
                );
                state.add_click_target(row_bounds, action);
                y += row_h + row_gap;
            }
            y += section_gap;

            let env_title =
                state
                    .text_system
                    .create_line_buffer("Credential Sources", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &env_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;

            let token_file_line = match std::env::var("QUIRON_API_TOKEN_FILE") {
                Ok(path) if !path.trim().is_empty() => format!(
                    "QUIRON_API_TOKEN_FILE=set ({})",
                    truncate_chars(path.trim(), 26)
                ),
                _ => "QUIRON_API_TOKEN_FILE=missing (preferred)".to_string(),
            };
            let token_env_line = match std::env::var("QUIRON_API_TOKEN") {
                Ok(v) if !v.trim().is_empty() => "QUIRON_API_TOKEN=set (fallback)".to_string(),
                _ => "QUIRON_API_TOKEN=missing".to_string(),
            };
            let url_line = match std::env::var("QUIRON_BRAIN_URL") {
                Ok(url) if !url.trim().is_empty() => {
                    format!("QUIRON_BRAIN_URL={}", truncate_chars(url.trim(), 30))
                }
                _ => "QUIRON_BRAIN_URL=default".to_string(),
            };
            let env_lines = [token_file_line, token_env_line, url_line];
            for line in env_lines {
                let line_buf = state
                    .text_system
                    .create_line_buffer(&line, design::type_scale::XS, sidebar_content_width);
                state.text_system.draw_buffer(
                    canvas,
                    &line_buf,
                    sidebar_content_x,
                    y + 11.0,
                    Color::from_hex(palette.text),
                );
                y += 14.0;
            }
            y += section_gap;

            let cmd_title =
                state
                    .text_system
                    .create_line_buffer("Slash Commands", design::type_scale::SM, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &cmd_title,
                sidebar_content_x,
                y + section_label_h,
                Color::from_hex(palette.text_muted),
            );
            y += section_label_h + 2.0;
            let commands = "/connection  /connect-local  /connect-env";
            let commands_buf =
                state
                    .text_system
                    .create_line_buffer(commands, design::type_scale::XS, sidebar_content_width);
            state.text_system.draw_buffer(
                canvas,
                &commands_buf,
                sidebar_content_x,
                y + 11.0,
                Color::from_hex(palette.accent),
            );
        }
    }

    // === Editor ===
    if state.workspace_is_open() {
        render_editor_pane(canvas, state, primary_editor_bounds, EditorPane::Primary);
        if let Some(secondary_bounds) = secondary_editor_bounds {
            render_editor_pane(canvas, state, secondary_bounds, EditorPane::Secondary);
        }
    } else {
        render_welcome(canvas, state, editor_region_bounds, &palette);
    }

    // === Chat ===
    let chat_header = state
        .text_system
        .create_line_buffer("CHAT", design::type_scale::SM, chat_bounds.width - 16.0);
    state.text_system.draw_buffer(
        canvas,
        &chat_header,
        chat_bounds.x + 8.0,
        chat_bounds.y + 20.0,
        Color::from_hex(palette.text_muted),
    );
    let model_bounds = Bounds::new(
        chat_bounds.x + chat_bounds.width - 102.0,
        chat_bounds.y + 6.0,
        94.0,
        20.0,
    );
    canvas.fill_rounded_rect(
        model_bounds,
        4.0,
        Color::from_hex(palette.background).with_alpha(150),
    );
    let model_label = state.selected_ai_model().to_string();
    let model_buf = state
        .text_system
        .create_line_buffer(&model_label, design::type_scale::XS, model_bounds.width - 10.0);
    state.text_system.draw_buffer(
        canvas,
        &model_buf,
        model_bounds.x + 6.0,
        model_bounds.y + 13.0,
        Color::from_hex(palette.accent),
    );
    state.add_click_target(model_bounds, ClickTargetAction::ActivityCycleAiModel);
    let add_ctx_bounds = Bounds::new(model_bounds.x - 28.0, chat_bounds.y + 6.0, 24.0, 20.0);
    canvas.fill_rounded_rect(
        add_ctx_bounds,
        4.0,
        Color::from_hex(palette.selection).with_alpha(150),
    );
    // Eliminar stroke_rect
    let add_ctx_buf = state
        .text_system
        .create_line_buffer("+", design::type_scale::SM, add_ctx_bounds.width - 8.0);
    state.text_system.draw_buffer(
        canvas,
        &add_ctx_buf,
        add_ctx_bounds.x + 8.0,
        add_ctx_bounds.y + 14.0,
        Color::from_hex(palette.text),
    );
    state.add_click_target(
        add_ctx_bounds,
        ClickTargetAction::ActivityAddSelectionToChat,
    );
    canvas.draw_line(
        chat_bounds.x,
        chat_bounds.y + 32.0,
        chat_bounds.x + chat_bounds.width,
        chat_bounds.y + 32.0,
        Color::from_hex(palette.border).with_alpha(90),
        1.0,
    );

    let mut messages_start_y = chat_bounds.y + 40.0;
    if state.is_telemetry_panel_enabled() {
        let telemetry = state.session_telemetry_panel_info();
        let timeline_filter = state.telemetry_timeline_filter();
        let filter_label = match timeline_filter {
            TelemetryTimelineFilter::All => "all",
            TelemetryTimelineFilter::Checkpoints => "checkpoints",
            TelemetryTimelineFilter::Anomalies => "anomalies",
        };
        let panel_height = (chat_bounds.height * 0.42).clamp(126.0, 236.0);
        let panel_bounds = Bounds::new(
            chat_bounds.x + 8.0,
            chat_bounds.y + 24.0,
            (chat_bounds.width - 16.0).max(140.0),
            panel_height,
        );
        canvas.fill_rounded_rect(panel_bounds, 6.0, Color::from_hex(palette.surface));
        // Eliminar stroke_rect

        let session_label = short_session_id(&telemetry.session_id);
        let parent_label = telemetry
            .parent_session_id
            .as_deref()
            .map(short_session_id)
            .unwrap_or_else(|| "-".to_string());
        let line_1 = format!(
            "TEL [{}] seg={} cp={} an={} smp={} parent={}",
            session_label,
            telemetry.segment_seq,
            telemetry.checkpoints,
            telemetry.anomalies,
            telemetry.samples,
            parent_label
        );
        let line_2 = format!(
            "tokens={}/{} rem={} fb={:.2} p(cp)={}/{} p(an)={}/{}",
            telemetry.model_tokens_used_total,
            telemetry.token_budget,
            telemetry.token_budget_remaining,
            telemetry.fallback_rate,
            telemetry.persisted_checkpoints,
            telemetry.persisted_checkpoints_total,
            telemetry.persisted_anomalies,
            telemetry.persisted_anomalies_total
        );
        let cp_range = match (
            telemetry.last_checkpoint_step_start,
            telemetry.last_checkpoint_step_end,
        ) {
            (Some(start), Some(end)) => format!("{}..{}", start, end),
            _ => "none".to_string(),
        };
        let flags = if telemetry.last_checkpoint_flags.is_empty() {
            "none".to_string()
        } else {
            truncate_chars(&telemetry.last_checkpoint_flags.join(","), 24)
        };
        let last_anomaly = match (&telemetry.last_anomaly_kind, telemetry.last_anomaly_step) {
            (Some(kind), Some(step)) => format!("{}@{}", kind, step),
            _ => "none".to_string(),
        };
        let line_3 = format!(
            "last_cp={} flags={} last_an={} filter={} sync={}",
            cp_range, flags, last_anomaly, filter_label, telemetry.persisted_sync
        );

        let line_1_buf = state.text_system.create_line_buffer(
            &truncate_chars(&line_1, 96), design::type_scale::SM,
            panel_bounds.width - 10.0,
        );
        state.text_system.draw_buffer(
            canvas,
            &line_1_buf,
            panel_bounds.x + 6.0,
            panel_bounds.y + 14.0,
            Color::from_hex(palette.accent),
        );
        let line_2_buf = state.text_system.create_line_buffer(
            &truncate_chars(&line_2, 96), design::type_scale::SM,
            panel_bounds.width - 10.0,
        );
        state.text_system.draw_buffer(
            canvas,
            &line_2_buf,
            panel_bounds.x + 6.0,
            panel_bounds.y + 30.0,
            Color::from_hex(palette.text),
        );
        let line_3_buf = state.text_system.create_line_buffer(
            &truncate_chars(&line_3, 96), design::type_scale::SM,
            panel_bounds.width - 10.0,
        );
        state.text_system.draw_buffer(
            canvas,
            &line_3_buf,
            panel_bounds.x + 6.0,
            panel_bounds.y + 46.0,
            Color::from_hex(palette.text_muted),
        );

        let timeline_top = panel_bounds.y + 62.0;
        let timeline_bottom = panel_bounds.y + panel_bounds.height - 8.0;
        let timeline_row_h = (14.0 * density_scale).clamp(12.0, 20.0);
        let visible_rows = ((timeline_bottom - timeline_top) / timeline_row_h)
            .floor()
            .max(1.0) as usize;
        let filtered_entries = telemetry
            .timeline
            .iter()
            .filter(|entry| match timeline_filter {
                TelemetryTimelineFilter::All => true,
                TelemetryTimelineFilter::Checkpoints => !entry.is_anomaly,
                TelemetryTimelineFilter::Anomalies => entry.is_anomaly,
            })
            .collect::<Vec<_>>();

        if filtered_entries.is_empty() {
            let empty_buf = state.text_system.create_line_buffer(
                "timeline empty (esperando checkpoint/anomalia)", design::type_scale::SM,
                panel_bounds.width - 10.0,
            );
            state.text_system.draw_buffer(
                canvas,
                &empty_buf,
                panel_bounds.x + 6.0,
                timeline_top + 12.0,
                Color::from_hex(palette.text_muted),
            );
        } else {
            let max_scroll = filtered_entries.len().saturating_sub(visible_rows);
            let start = state.telemetry_timeline_scroll().min(max_scroll);
            for (row, entry) in filtered_entries
                .iter()
                .skip(start)
                .take(visible_rows)
                .enumerate()
            {
                let y = timeline_top + row as f32 * timeline_row_h;
                let marker = match (entry.is_anomaly, entry.source) {
                    (true, SessionTelemetryTimelineSource::Local) => "[AN-L]",
                    (true, SessionTelemetryTimelineSource::Persisted) => "[AN-P]",
                    (false, SessionTelemetryTimelineSource::Local) => "[CP-L]",
                    (false, SessionTelemetryTimelineSource::Persisted) => "[CP-P]",
                };
                let line = format!("{} {}", marker, truncate_chars(&entry.label, 88));
                let row_buf =
                    state
                        .text_system
                        .create_line_buffer(&line, design::type_scale::SM, panel_bounds.width - 10.0);
                state.text_system.draw_buffer(
                    canvas,
                    &row_buf,
                    panel_bounds.x + 6.0,
                    y + 11.0,
                    match (entry.is_anomaly, entry.source) {
                        (true, SessionTelemetryTimelineSource::Local) => {
                            Color::from_hex(palette.accent)
                        }
                        (true, SessionTelemetryTimelineSource::Persisted) => {
                            Color::from_hex(palette.accent).with_alpha(170)
                        }
                        (false, SessionTelemetryTimelineSource::Local) => {
                            Color::from_hex(palette.text)
                        }
                        (false, SessionTelemetryTimelineSource::Persisted) => {
                            Color::from_hex(palette.text_muted)
                        }
                    },
                );
            }
            if filtered_entries.len() > visible_rows {
                let end = (start + visible_rows).min(filtered_entries.len());
                let scroll_hint = format!(
                    "timeline {}/{}..{} wheel=scroll | CP/AN + L/P",
                    filtered_entries.len(),
                    start + 1,
                    end
                );
                let hint_buf =
                    state
                        .text_system
                        .create_line_buffer(&scroll_hint, design::type_scale::XS, panel_bounds.width - 10.0);
                state.text_system.draw_buffer(
                    canvas,
                    &hint_buf,
                    panel_bounds.x + 6.0,
                    panel_bounds.y + panel_bounds.height - 2.0,
                    Color::from_hex(palette.text_muted),
                );
            }
        }

        messages_start_y = panel_bounds.y + panel_bounds.height + 8.0;
    }

    let messages_to_show = state
        .messages
        .iter()
        .rev()
        .take(10)
        .cloned()
        .collect::<Vec<_>>();
    let messages_to_show: Vec<_> = messages_to_show.into_iter().rev().collect();

    let msg_card_x = chat_bounds.x + 7.0;
    let msg_card_w = (chat_bounds.width - 14.0).max(80.0);
    let msg_limit_y = input_bounds.y - 8.0;
    let msg_text_w = (msg_card_w - 34.0).max(40.0);

    // La altura de cada tarjeta depende de cuántas líneas ocupe su texto al
    // envolverse. Se mide antes de dibujar; suponerla fija apilaba los mensajes.
    let measured: Vec<(ChatMessage, String, f32, f32)> = messages_to_show
        .into_iter()
        .map(|msg| {
            let text = truncate_chars(&msg.content, CHAT_MESSAGE_MAX_CHARS);
            let (_, text_h) = state.text_system.measure(&text, 12.0, msg_text_w);
            let text_h = text_h.max(CHAT_LINE_HEIGHT);

            let mut block_h = CHAT_TEXT_TOP + text_h + CHAT_CARD_PADDING_BOTTOM;
            if msg.meta.is_some() {
                block_h += CHAT_DETAIL_LINE;
            }
            block_h += msg.citations.len().min(2) as f32 * CHAT_DETAIL_LINE;

            (msg, text, text_h, block_h)
        })
        .collect();

    // Se coloca desde el último mensaje hacia arriba: la respuesta recién
    // llegada siempre queda visible, aunque las anteriores sean largas.
    let mut placements: Vec<(usize, f32)> = Vec::new();
    let mut cursor_y = msg_limit_y;
    for (index, (_, _, _, block_h)) in measured.iter().enumerate().rev() {
        let top = cursor_y - block_h;
        if top < messages_start_y {
            break;
        }
        placements.push((index, top));
        cursor_y = top - 6.0;
    }
    placements.reverse();

    for (index, msg_y) in placements {
        let (msg, text, text_h, block_h) = &measured[index];
        let (msg, text, text_h, block_h) = (msg, text.as_str(), *text_h, *block_h);

        let card_bounds = Bounds::new(msg_card_x, msg_y, msg_card_w, block_h);
        let card_bg = if msg.is_user {
            Color::from_hex(palette.selection).with_alpha(40)
        } else {
            Color::from_hex(palette.surface).with_alpha(200)
        };
        canvas.fill_rounded_rect(card_bounds, 12.0, card_bg);

        let role_bounds = Bounds::new(card_bounds.x + 8.0, card_bounds.y + 8.0, 16.0, 16.0);
        canvas.fill_rounded_rect(
            role_bounds,
            8.0,
            if msg.is_user {
                Color::from_hex(palette.accent).with_alpha(60)
            } else {
                Color::from_hex(palette.background)
            },
        );
        let role_icon = if msg.is_user {
            icons::USER
        } else {
            icons::ASSISTANT
        };
        let role_buf = state.text_system.create_icon_buffer(role_icon, ROLE_ICON_SIZE);
        state.text_system.draw_buffer(
            canvas,
            &role_buf,
            role_bounds.x + (role_bounds.width - ROLE_ICON_SIZE) * 0.5,
            role_bounds.y + (role_bounds.height + ROLE_ICON_SIZE) * 0.5 - 1.0,
            if msg.is_user {
                Color::from_hex(palette.text)
            } else {
                Color::from_hex(palette.accent)
            },
        );

        // El cuerpo del mensaje siempre en color de texto. El acento distingue
        // el rol y las citas; usarlo para prosa larga la vuelve ilegible.
        let text_color = Color::from_hex(palette.text);
        let msg_buf = state.text_system.create_buffer(text, design::type_scale::MD, msg_text_w);
        state.text_system.draw_buffer(
            canvas,
            &msg_buf,
            card_bounds.x + 26.0,
            card_bounds.y + CHAT_TEXT_TOP,
            text_color,
        );

        // Los detalles empiezan donde termina el texto, no a una altura fija.
        let mut detail_y = card_bounds.y + CHAT_TEXT_TOP + text_h + 2.0;

        if let Some(meta) = &msg.meta {
            let meta_buf = state.text_system.create_line_buffer(
                &truncate_chars(meta, 96), design::type_scale::SM,
                card_bounds.width - 14.0,
            );
            state.text_system.draw_buffer(
                canvas,
                &meta_buf,
                card_bounds.x + 8.0,
                detail_y,
                Color::from_hex(palette.text_muted),
            );
            detail_y += CHAT_DETAIL_LINE;
        }

        for citation in msg.citations.iter().take(2) {
            let short_id = citation.event_id.chars().take(8).collect::<String>();
            let citation_text = format!(
                "-> [{}] {}",
                short_id,
                truncate_chars(&citation.snippet, 56)
            );
            let citation_x = card_bounds.x + 10.0;
            let citation_y = detail_y;
            let citation_buf =
                state
                    .text_system
                    .create_line_buffer(&citation_text, design::type_scale::SM, card_bounds.width - 18.0);
            state.text_system.draw_buffer(
                canvas,
                &citation_buf,
                citation_x,
                citation_y,
                Color::from_hex(palette.accent),
            );
            state.add_click_target(
                Bounds::new(
                    citation_x,
                    citation_y - 10.0,
                    card_bounds.width - 20.0,
                    14.0,
                ),
                ClickTargetAction::Citation(citation.clone()),
            );
            detail_y += CHAT_DETAIL_LINE;
        }
    }

    // === Input de chat ===
    // Con el campo enfocado y vacío se mostraba el texto de ayuda y ningún
    // cursor: no había forma de saber que ya se podía escribir.
    let input_display = match (state.input_text.is_empty(), state.input_focused) {
        (true, true) => "|".to_string(),
        (true, false) => "Pregunta sobre el proyecto...".to_string(),
        (false, true) => format!("{}|", state.input_text),
        (false, false) => state.input_text.clone(),
    };
    let input_buf =
        state
            .text_system
            .create_line_buffer(&input_display, design::type_scale::MD, input_bounds.width - 24.0);
    let input_color = if state.input_text.is_empty() && !state.input_focused {
        Color::from_hex(palette.text_muted)
    } else {
        Color::from_hex(palette.text)
    };
    state.text_system.draw_buffer(
        canvas,
        &input_buf,
        input_bounds.x + 12.0,
        input_bounds.y + 26.0,
        input_color,
    );

    // === Barra de estado compacta ===
    let active_editor = state.active_editor();
    let cursor = active_editor.cursor();
    let mode_and_cursor = format!(
        "{:?} {}:{}",
        active_editor.mode(),
        cursor.line + 1,
        cursor.column + 1
    );
    let active_syntax_diag = state.active_file_path().and_then(|path| {
        let language_name = state
            .language_registry
            .detect_language(path)
            .map(|lang| lang.name().to_string())?;
        let source = state.active_editor().text();
        syntax_diagnostic(Some(&language_name), &source).map(|diag| (language_name, diag))
    });
    let file_label = state
        .active_file_path()
        .map(|p| relative_workspace_path(&state.workspace_root, p))
        .unwrap_or_else(|| "untitled".to_string());
    let file_label = if active_editor.is_modified() {
        format!("*{}", file_label)
    } else {
        file_label
    };
    let conn_health_label = state.quiron_connection_health_label();
    let conn_health_ok = conn_health_label == "health=ok";
    let conn_health_checking = conn_health_label == "health=checking";

    let connection_label = if conn_health_ok {
        "connected".to_string()
    } else if conn_health_checking {
        "connecting".to_string()
    } else {
        "offline".to_string()
    };
    let connection_color = if conn_health_ok {
        Color::from_hex(palette.success)
    } else if conn_health_checking {
        Color::from_hex(palette.warning)
    } else {
        Color::from_hex(palette.error)
    };
    let mut status_chips: Vec<(String, Color, Color, Color, Option<ClickTargetAction>)> = vec![
        (
            connection_label,
            Color::from_hex(palette.surface),
            connection_color,
            Color::from_hex(palette.surface),
            Some(ClickTargetAction::TopMenuExecute(
                CommandPaletteAction::ShowQuironConnectionStatus,
            )),
        ),
        (
            if let Some((_, diag)) = active_syntax_diag.as_ref() {
                format!("! L{}:{}", diag.line + 1, diag.column + 1)
            } else {
                "0 problems".to_string()
            },
            Color::from_hex(palette.surface),
            if active_syntax_diag.is_some() {
                Color::from_hex(palette.error)
            } else {
                Color::from_hex(palette.text_muted)
            },
            Color::from_hex(palette.surface),
            Some(ClickTargetAction::ActivitySidebarProblems),
        ),
        (
            truncate_chars(&file_label, 42),
            Color::from_hex(palette.surface),
            Color::from_hex(palette.text),
            Color::from_hex(palette.surface),
            None,
        ),
        (
            mode_and_cursor,
            Color::from_hex(palette.surface),
            Color::from_hex(palette.text_muted),
            Color::from_hex(palette.surface),
            None,
        ),
    ];
    if let Some(git) = state.git_sidebar_status.as_ref() {
        let mut git_label = format!("git:{}", truncate_chars(&git.branch, 18));
        if git.ahead > 0 {
            git_label.push_str(&format!(" ↑{}", git.ahead));
        }
        if git.behind > 0 {
            git_label.push_str(&format!(" ↓{}", git.behind));
        }
        if git.total > 0 {
            git_label.push_str(&format!(" *{}", git.total));
        }
        status_chips.push((
            git_label,
            Color::from_hex(palette.surface),
            Color::from_hex(palette.success),
            Color::from_hex(palette.border),
            Some(ClickTargetAction::ActivitySidebarGit),
        ));
    }
    if state.loading {
        status_chips.push((
            "thinking...".to_string(),
            Color::from_hex(palette.surface),
            Color::from_hex(palette.accent),
            Color::from_hex(palette.accent).with_alpha(180),
            None,
        ));
    } else {
        status_chips.push((
            truncate_chars(&state.status_text, 56),
            Color::from_hex(palette.surface),
            Color::from_hex(palette.text_muted),
            Color::from_hex(palette.border),
            None,
        ));
    }

    let mut chip_x = 8.0;
    let chip_y = bounds.height - status_height + 4.0;
    let chip_h = (status_height - 8.0).max(14.0);
    for (label, bg, fg, border, action) in status_chips {
        let chip_w = (label.chars().count() as f32 * 6.4 + 14.0).clamp(44.0, bounds.width * 0.58);
        if chip_x + chip_w > bounds.width - 8.0 {
            break;
        }
        let chip_bounds = Bounds::new(chip_x, chip_y, chip_w, chip_h);
        canvas.fill_rect(chip_bounds, bg);
        if border != bg {
            canvas.stroke_rect(chip_bounds, border, 1.0);
        }
        let chip_buf = state.text_system.create_line_buffer(&label, design::type_scale::SM, chip_w - 10.0);
        state
            .text_system
            .draw_buffer(canvas, &chip_buf, chip_x + 6.0, chip_y + 12.0, fg);
        if let Some(action) = action {
            state.add_click_target(chip_bounds, action);
        }
        chip_x += chip_w + 2.0;
    }

    render_top_menu_dropdown(canvas, state, palette, &menu_layout);

    // === Overlay (Quick Open / Command Palette / Symbols) ===
    if let Some(mode) = state.overlay_mode {
        canvas.fill_rect(bounds, overlay_scrim_color(palette));

        let panel_width = (bounds.width * 0.62).clamp(420.0, 900.0);
        let panel_rows = state.overlay_items.len().max(1).min(OVERLAY_RESULTS_MAX);
        let overlay_row_h = (24.0 * density_scale).clamp(20.0, 32.0);
        let overlay_row_box_h = (22.0 * density_scale).clamp(18.0, 28.0);
        let panel_height = 68.0 + panel_rows as f32 * overlay_row_h + 12.0;
        let panel_x = bounds.x + (bounds.width - panel_width) * 0.5;
        let panel_y = header_height + 40.0;
        let panel_bounds = Bounds::new(panel_x, panel_y, panel_width, panel_height);

        canvas.fill_rect(panel_bounds, theme_hex(palette, 0x161a28, 0xf7efe2));
        canvas.stroke_rect(panel_bounds, Color::from_hex(palette.accent), 1.0);

        let title = match mode {
            OverlayMode::QuickOpen => "QUICK OPEN",
            OverlayMode::CommandPalette => "COMMAND PALETTE",
            OverlayMode::GoToLine => "GO TO LINE",
            OverlayMode::Symbols => "GO TO SYMBOL",
            OverlayMode::WorkspaceSymbols => "GO TO WORKSPACE SYMBOL",
            OverlayMode::WorkspaceTextSearch => "FIND IN WORKSPACE",
            OverlayMode::Problems => "PROBLEMS",
        };
        let title_buf = state
            .text_system
            .create_line_buffer(title, design::type_scale::MD, panel_width - 20.0);
        state.text_system.draw_buffer(
            canvas,
            &title_buf,
            panel_x + 10.0,
            panel_y + 16.0,
            Color::from_hex(palette.accent),
        );

        let hint = match mode {
            OverlayMode::QuickOpen => {
                "Enter open | Alt+Enter side | Up/Down move | PgUp/PgDn page | Ctrl+L clear | Esc close"
            }
            OverlayMode::CommandPalette => {
                "Enter run | Up/Down move | PgUp/PgDn page | Ctrl+L clear | Esc close"
            }
            OverlayMode::GoToLine => {
                "Format: line or line:column | Enter jump | Ctrl+L clear | Esc close"
            }
            OverlayMode::Symbols => {
                "Enter jump | Up/Down move | PgUp/PgDn page | Ctrl+L clear | Esc close"
            }
            OverlayMode::WorkspaceSymbols => {
                "Enter jump | Alt+Enter side | Up/Down move | PgUp/PgDn page | Ctrl+L clear | Esc close"
            }
            OverlayMode::WorkspaceTextSearch => {
                "Enter open match | Alt+Enter side | query text | Ctrl+L clear | Esc close"
            }
            OverlayMode::Problems => {
                "Enter apply | query by editor/search/git/telemetry/runtime | Esc close"
            }
        };
        let hint_buf = state
            .text_system
            .create_line_buffer(hint, design::type_scale::SM, panel_width - 20.0);
        state.text_system.draw_buffer(
            canvas,
            &hint_buf,
            panel_x + 10.0,
            panel_y + 30.0,
            Color::from_hex(palette.text_muted),
        );

        let query_prefix = match mode {
            OverlayMode::QuickOpen => "> file: ",
            OverlayMode::CommandPalette => "> command: ",
            OverlayMode::GoToLine => "> line[:column]: ",
            OverlayMode::Symbols => "> symbol: ",
            OverlayMode::WorkspaceSymbols => "> workspace symbol: ",
            OverlayMode::WorkspaceTextSearch => "> workspace text: ",
            OverlayMode::Problems => "> problem: ",
        };
        let query_text = if state.overlay_query.is_empty() {
            format!("{}|", query_prefix)
        } else {
            format!(
                "{}{}|",
                query_prefix,
                truncate_chars(&state.overlay_query, 72)
            )
        };
        let query_buf = state
            .text_system
            .create_line_buffer(&query_text, design::type_scale::MD, panel_width - 20.0);
        state.text_system.draw_buffer(
            canvas,
            &query_buf,
            panel_x + 10.0,
            panel_y + 48.0,
            if state.overlay_query.is_empty() {
                Color::from_hex(palette.text_muted)
            } else {
                Color::from_hex(palette.text)
            },
        );

        let overlay_items = state.overlay_items.clone();
        let mut row_y = panel_y + 64.0;
        if overlay_items.is_empty() {
            let empty_buf = state
                .text_system
                .create_line_buffer("No results", design::type_scale::SM, panel_width - 20.0);
            state.text_system.draw_buffer(
                canvas,
                &empty_buf,
                panel_x + 10.0,
                row_y + 14.0,
                Color::from_hex(palette.text_muted),
            );
        } else {
            for (idx, item) in overlay_items.iter().enumerate() {
                let row_bounds =
                    Bounds::new(panel_x + 6.0, row_y, panel_width - 12.0, overlay_row_box_h);
                if idx == state.overlay_selected {
                    canvas.fill_rect(row_bounds, Color::from_hex(palette.selection));
                    canvas.stroke_rect(row_bounds, Color::from_hex(palette.accent), 1.0);
                }

                let line = match mode {
                    OverlayMode::QuickOpen => {
                        format!("{}  •  {}", item.title, truncate_chars(&item.detail, 82))
                    }
                    OverlayMode::CommandPalette => {
                        format!("{}  •  {}", item.title, item.detail)
                    }
                    OverlayMode::GoToLine => {
                        format!("{}  •  {}", item.title, item.detail)
                    }
                    OverlayMode::Symbols => {
                        format!("{}  •  {}", item.title, item.detail)
                    }
                    OverlayMode::WorkspaceSymbols => {
                        format!("{}  •  {}", item.title, truncate_chars(&item.detail, 82))
                    }
                    OverlayMode::WorkspaceTextSearch => {
                        format!("{}  •  {}", item.title, truncate_chars(&item.detail, 82))
                    }
                    OverlayMode::Problems => {
                        format!("{}  •  {}", item.title, truncate_chars(&item.detail, 82))
                    }
                };
                let line_buf = state
                    .text_system
                    .create_line_buffer(&line, design::type_scale::SM, panel_width - 24.0);
                state.text_system.draw_buffer(
                    canvas,
                    &line_buf,
                    panel_x + 12.0,
                    row_y + 14.0,
                    if idx == state.overlay_selected {
                        Color::from_hex(palette.text)
                    } else {
                        Color::from_hex(palette.text_muted)
                    },
                );

                state.add_click_target(row_bounds, ClickTargetAction::OverlayItemSelect(idx));
                row_y += overlay_row_h;
            }
        }
    }
}

fn render_top_menu_dropdown(
    canvas: &mut Canvas,
    state: &mut AppState,
    palette: &ThemePalette,
    menu_layout: &[(TopMenuKind, Bounds)],
) {
    let Some(open_menu) = state.top_menu_open() else {
        return;
    };
    let Some((_, anchor_bounds)) = menu_layout.iter().find(|(menu, _)| *menu == open_menu) else {
        return;
    };

    let entries = top_menu_entries(open_menu);
    let row_h = 20.0;
    let dropdown_w = entries
        .iter()
        .map(|entry| entry.label.chars().count() as f32 * 8.0 + 28.0)
        .fold(160.0_f32, f32::max)
        .clamp(160.0, 300.0);
    let dropdown_x = anchor_bounds.x;
    let dropdown_y = anchor_bounds.y + anchor_bounds.height + 2.0;
    let dropdown_h = entries.len() as f32 * row_h + 8.0;
    let dropdown_bounds = Bounds::new(dropdown_x, dropdown_y, dropdown_w, dropdown_h);
    canvas.fill_rect(dropdown_bounds, Color::from_hex(palette.surface));
    canvas.stroke_rect(dropdown_bounds, Color::from_hex(palette.border), 1.0);

    let selected_idx = state
        .top_menu_selected_index()
        .min(entries.len().saturating_sub(1));
    for (idx, entry) in entries.iter().enumerate() {
        let row_y = dropdown_y + 4.0 + idx as f32 * row_h;
        let row_bounds = Bounds::new(dropdown_x + 2.0, row_y, dropdown_w - 4.0, row_h);
        if idx == selected_idx {
            canvas.fill_rect(row_bounds, Color::from_hex(palette.selection));
        }
        let row_buf = state
            .text_system
            .create_line_buffer(entry.label, design::type_scale::SM, dropdown_w - 12.0);
        state.text_system.draw_buffer(
            canvas,
            &row_buf,
            dropdown_x + 8.0,
            row_y + 14.0,
            if idx == selected_idx {
                Color::from_hex(palette.text)
            } else {
                Color::from_hex(palette.text_muted)
            },
        );
        state.add_click_target(row_bounds, ClickTargetAction::TopMenuExecute(entry.action));
    }
}

/// Pantalla mostrada mientras no hay proyecto abierto.
///
/// Enseña el estado real del índice —no un eslogan— porque es lo que decide si
/// una consulta al chat va a servir de algo: sin Qdrant no hay recuperación, y
/// sin grafo no hay dependencias.
fn render_welcome(
    canvas: &mut Canvas,
    state: &mut AppState,
    bounds: Bounds,
    palette: &ThemePalette,
) {
    let column_width = WELCOME_COLUMN_WIDTH.min(bounds.width - 64.0).max(240.0);
    let x = bounds.x + (bounds.width - column_width) * 0.5;
    let mut y = bounds.y + (bounds.height * 0.22).max(48.0);

    let title = state.text_system.create_line_buffer("Llore", design::type_scale::XL, column_width);
    state
        .text_system
        .draw_buffer(canvas, &title, x, y, Color::from_hex(palette.text));
    y += 34.0;

    let subtitle = state.text_system.create_buffer(
        "Editor con índice de código y grafo de dependencias", design::type_scale::MD,
        column_width,
    );
    state
        .text_system
        .draw_buffer(canvas, &subtitle, x, y, Color::from_hex(palette.text_muted));
    y += 40.0;

    // --- Acción principal ---
    let button = Bounds::new(x, y, 168.0, 34.0);
    canvas.fill_rounded_rect(button, 8.0, Color::from_hex(palette.accent).with_alpha(38));
    canvas.stroke_rect(button, Color::from_hex(palette.accent).with_alpha(90), 1.0);

    let folder_icon = state
        .text_system
        .create_icon_buffer(icons::FOLDER_OPEN, 14.0);
    state.text_system.draw_buffer(
        canvas,
        &folder_icon,
        button.x + 14.0,
        button.y + 22.0,
        Color::from_hex(palette.accent),
    );
    let button_label = state
        .text_system
        .create_line_buffer("Abrir carpeta…", design::type_scale::MD, button.width - 44.0);
    state.text_system.draw_buffer(
        canvas,
        &button_label,
        button.x + 36.0,
        button.y + 22.0,
        Color::from_hex(palette.text),
    );
    state.add_click_target(button, ClickTargetAction::WelcomeOpenFolder);
    y += 62.0;

    // --- Estado del índice ---
    let heading = state
        .text_system
        .create_line_buffer("ESTADO DEL ÍNDICE", design::type_scale::XS, column_width);
    state
        .text_system
        .draw_buffer(canvas, &heading, x, y, Color::from_hex(palette.text_muted));
    y += 22.0;

    let health = state.quiron_index_health.clone();
    let connected = health.is_some();
    let (units, nodes) = health
        .as_ref()
        .map(|h| (h.event_count, h.node_count))
        .unwrap_or((0, 0));

    let rows: [(char, String, bool); 3] = [
        (
            icons::PLUG,
            match &health {
                Some(h) => format!("quiron-brain  ·  {}", h.status),
                None => "quiron-brain  ·  sin respuesta".to_string(),
            },
            connected,
        ),
        (
            icons::DATABASE,
            if connected {
                format!("almacén vectorial  ·  {units} unidades")
            } else {
                "almacén vectorial  ·  desconocido".to_string()
            },
            connected && units > 0,
        ),
        (
            icons::NETWORK,
            if connected {
                format!("grafo  ·  {nodes} nodos")
            } else {
                "grafo  ·  desconocido".to_string()
            },
            connected && nodes > 0,
        ),
    ];

    for (icon, label, healthy) in rows {
        let tint = if healthy {
            Color::from_hex(palette.success)
        } else {
            Color::from_hex(palette.warning)
        };
        let icon_buffer = state.text_system.create_icon_buffer(icon, 13.0);
        state
            .text_system
            .draw_buffer(canvas, &icon_buffer, x + 1.0, y + 11.0, tint);

        let row = state
            .text_system
            .create_line_buffer(&label, design::type_scale::MD, column_width - 28.0);
        state.text_system.draw_buffer(
            canvas,
            &row,
            x + 24.0,
            y + 11.0,
            Color::from_hex(palette.text_muted),
        );
        y += WELCOME_ROW_HEIGHT;
    }

    // --- Proyectos recientes ---
    if state.recent_projects.is_empty() {
        return;
    }

    y += 18.0;
    let heading = state
        .text_system
        .create_line_buffer("RECIENTES", design::type_scale::XS, column_width);
    state
        .text_system
        .draw_buffer(canvas, &heading, x, y, Color::from_hex(palette.text_muted));
    y += 20.0;

    let limit = bounds.y + bounds.height - 24.0;
    for project in state.recent_projects.clone().iter().take(6) {
        if y + WELCOME_ROW_HEIGHT > limit {
            break;
        }
        let row_bounds = Bounds::new(x - 6.0, y, column_width + 12.0, WELCOME_ROW_HEIGHT);

        let name = llore_ui::recents::display_name(project);
        let name_buffer = state.text_system.create_line_buffer(&name, design::type_scale::MD, 180.0);
        state.text_system.draw_buffer(
            canvas,
            &name_buffer,
            x,
            y + 15.0,
            Color::from_hex(palette.accent),
        );

        let location = project
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        let location_buffer =
            state
                .text_system
                .create_line_buffer(&location, design::type_scale::SM, (column_width - 190.0).max(60.0));
        state.text_system.draw_buffer(
            canvas,
            &location_buffer,
            x + 186.0,
            y + 15.0,
            Color::from_hex(palette.text_muted),
        );

        state.add_click_target(
            row_bounds,
            ClickTargetAction::WelcomeOpenRecent(project.clone()),
        );
        y += WELCOME_ROW_HEIGHT;
    }
}

fn render_editor_pane(
    canvas: &mut Canvas,
    state: &mut AppState,
    editor_bounds: Bounds,
    pane: EditorPane,
) {
    let palette = state.theme_palette();
    let tabs = state.tabs_snapshot_for_pane(pane);
    let editor = state.editor_for_pane(pane);
    let cursor = *editor.cursor();
    let selection = editor.selection().clone();
    let editor_mode = editor.mode();
    let scroll_top = editor.scroll_top();
    let scroll_left = editor.scroll_left();
    let editor_text = editor.text();
    let pane_focused = state.is_editor_pane_focused(pane);
    let pane_language_name = state
        .file_path_for_pane(pane)
        .and_then(|path| state.language_registry.detect_language(path))
        .map(|lang| lang.name().to_string());
    let pane_syntax_diag = pane_language_name
        .as_deref()
        .and_then(|language_name| syntax_diagnostic(Some(language_name), &editor_text));

    let tab_bar_h = EDITOR_TAB_BAR_HEIGHT;
    canvas.fill_rect(
        Bounds::new(
            editor_bounds.x + 1.0,
            editor_bounds.y + 1.0,
            editor_bounds.width - 2.0,
            tab_bar_h,
        ),
        Color::from_hex(palette.surface).with_alpha(100),
    );

    let tab_h = tab_bar_h - 3.0;
    let mut tab_x = editor_bounds.x + 8.0;
    let max_tab_right = editor_bounds.x + editor_bounds.width - 160.0;
    for tab in tabs.iter().take(10) {
        let width = (tab.title.chars().count() as f32 * 7.0 + 24.0).clamp(86.0, 210.0);
        if tab_x + width > max_tab_right {
            break;
        }

        let tab_bounds = Bounds::new(tab_x, editor_bounds.y + 2.0, width, tab_h);
        canvas.fill_rect(
            tab_bounds,
            if tab.active {
                Color::from_hex(palette.surface)
            } else {
                Color::from_hex(palette.background)
            },
        );
        canvas.stroke_rect(
            tab_bounds,
            if tab.active {
                Color::from_hex(palette.accent)
            } else {
                Color::from_hex(palette.border)
            },
            1.0,
        );

        let tab_label = truncate_chars(&tab.title, 26);
        let tab_label_buf = state
            .text_system
            .create_line_buffer(&tab_label, design::type_scale::SM, width - 16.0);
        state.text_system.draw_buffer(
            canvas,
            &tab_label_buf,
            tab_x + 8.0,
            editor_bounds.y + 17.0,
            if tab.active {
                Color::from_hex(palette.text)
            } else {
                Color::from_hex(palette.text_muted)
            },
        );

        state.add_tab_hitbox(pane, tab.index, tab_bounds);
        state.add_click_target(
            tab_bounds,
            ClickTargetAction::TabSelect {
                pane,
                index: tab.index,
            },
        );
        tab_x += width + 6.0;
    }

    let pane_label = match pane {
        EditorPane::Primary => "L",
        EditorPane::Secondary => "R",
    };
    let pane_status = format!(
        "{} {:?} {}:{} t={}",
        pane_label,
        editor_mode,
        cursor.line + 1,
        cursor.column + 1,
        tabs.len()
    );
    let status_w = (pane_status.chars().count() as f32 * 6.5 + 18.0).clamp(130.0, 240.0);
    let status_x =
        (editor_bounds.x + editor_bounds.width - status_w - 8.0).max(max_tab_right + 10.0);
    let status_bounds = Bounds::new(status_x, editor_bounds.y + 4.0, status_w, tab_h - 4.0);
    canvas.fill_rounded_rect(
        status_bounds,
        4.0,
        Color::from_hex(palette.selection).with_alpha(80),
    );
    let status_buf = state
        .text_system
        .create_line_buffer(&pane_status, design::type_scale::SM, status_w - 10.0);
    state.text_system.draw_buffer(
        canvas,
        &status_buf,
        status_x + 6.0,
        editor_bounds.y + 17.0,
        Color::from_hex(palette.text_muted),
    );

    if let Some(diag) = pane_syntax_diag.as_ref() {
        let err_label = format!(
            "ERR L{}:{} {}",
            diag.line + 1,
            diag.column + 1,
            truncate_chars(&diag.message, 20)
        );
        let err_w = (err_label.chars().count() as f32 * 6.1 + 14.0).clamp(92.0, 260.0);
        let err_x = (status_x - err_w - 6.0).max(editor_bounds.x + 8.0);
        let err_bounds = Bounds::new(err_x, editor_bounds.y + 4.0, err_w, tab_h - 4.0);
        canvas.fill_rounded_rect(
            err_bounds,
            4.0,
            Color::from_hex(palette.error).with_alpha(40),
        );
        // Eliminar stroke_rect
        let err_buf = state
            .text_system
            .create_line_buffer(&err_label, design::type_scale::XS, err_w - 8.0);
        state.text_system.draw_buffer(
            canvas,
            &err_buf,
            err_x + 5.0,
            editor_bounds.y + 17.0,
            Color::from_hex(palette.error),
        );
    }

    // Breadcrumb compacto de ruta activa en el tab bar (patrón Zed adaptado).
    let breadcrumb_x = max_tab_right + 8.0;
    let breadcrumb_w = (status_x - breadcrumb_x - 6.0).max(0.0);
    if breadcrumb_w > 32.0 {
        if let Some(path) = state.file_path_for_pane(pane).cloned() {
            let segments = breadcrumb_segments(&state.workspace_root, &path);
            let total_segments = segments.len();
            let mut seg_x = breadcrumb_x;
            for (index, (label, target_path, is_file)) in segments.into_iter().enumerate() {
                let shown = truncate_chars(&label, 18);
                let seg_w = (shown.chars().count() as f32 * 7.0 + 16.0).clamp(30.0, 160.0);
                if seg_x + seg_w > breadcrumb_x + breadcrumb_w {
                    break;
                }

                let seg_bounds = Bounds::new(seg_x, editor_bounds.y + 4.0, seg_w, tab_h - 4.0);
                canvas.fill_rounded_rect(
                    seg_bounds,
                    4.0,
                    Color::from_hex(palette.background).with_alpha(80),
                );
                // Eliminar stroke_rect
                let seg_buf = state.text_system.create_line_buffer(&shown, design::type_scale::SM, seg_w - 8.0);
                state.text_system.draw_buffer(
                    canvas,
                    &seg_buf,
                    seg_x + 5.0,
                    editor_bounds.y + 17.0,
                    if is_file {
                        Color::from_hex(palette.text)
                    } else {
                        Color::from_hex(palette.text_muted)
                    },
                );
                state.add_click_target(
                    seg_bounds,
                    ClickTargetAction::BreadcrumbSegment {
                        pane,
                        path: target_path,
                        is_file,
                    },
                );

                seg_x += seg_w + 4.0;
                if index + 1 < total_segments && seg_x + 10.0 <= breadcrumb_x + breadcrumb_w {
                    let sep_buf = state.text_system.create_line_buffer("/", design::type_scale::SM, 8.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &sep_buf,
                        seg_x,
                        editor_bounds.y + 17.0,
                        Color::from_hex(palette.text_muted),
                    );
                    seg_x += 8.0;
                }
            }
        } else {
            let breadcrumb_buf = state
                .text_system
                .create_line_buffer("untitled", design::type_scale::SM, breadcrumb_w);
            state.text_system.draw_buffer(
                canvas,
                &breadcrumb_buf,
                breadcrumb_x,
                editor_bounds.y + 17.0,
                Color::from_hex(palette.text_muted),
            );
        }
    }

    let line_height = state.editor_line_height();
    let code_font_size = state.editor_code_font_size();
    let char_width = state.editor_char_width();
    let density_scale = state.ui_density_scale();
    let search_results_row_height =
        (SEARCH_RESULTS_ROW_HEIGHT_BASE * density_scale).clamp(14.0, 24.0);
    let search_results_header_height =
        (SEARCH_RESULTS_HEADER_HEIGHT_BASE * density_scale).clamp(14.0, 24.0);
    let search_results_panel_padding_top =
        (SEARCH_RESULTS_PANEL_PADDING_TOP_BASE * density_scale).clamp(3.0, 8.0);
    let search_results_panel_padding_bottom =
        (SEARCH_RESULTS_PANEL_PADDING_BOTTOM_BASE * density_scale).clamp(3.0, 8.0);
    let gutter_width = EDITOR_GUTTER_WIDTH;
    let editor_hpad = state.editor_horizontal_padding();
    let code_left_x = editor_bounds.x + gutter_width + editor_hpad;
    let body_top = editor_bounds.y + tab_bar_h + EDITOR_BODY_TOP_PADDING;
    let body_height_total =
        (editor_bounds.height - tab_bar_h - EDITOR_BODY_BOTTOM_PADDING).max(line_height);
    let lines: Vec<&str> = editor_text.split('\n').collect();
    let max_chars =
        ((editor_bounds.width - gutter_width - editor_hpad - 18.0) / char_width).max(1.0) as usize;
    let show_search_results_panel =
        pane_focused && state.search_active && !state.search_matches.is_empty();
    let search_results_rows = if show_search_results_panel {
        state.search_matches.len().min(SEARCH_RESULTS_MAX_ROWS)
    } else {
        0
    };
    let search_results_panel_height = if show_search_results_panel {
        search_results_panel_padding_top
            + search_results_header_height
            + search_results_rows as f32 * search_results_row_height
            + search_results_panel_padding_bottom
    } else {
        0.0
    };
    let body_height = (body_height_total - search_results_panel_height).max(line_height);
    let max_visible_lines = (body_height / line_height).floor() as usize;

    if pane_focused && state.search_active {
        let active_pos = state.active_search_match.map(|idx| idx + 1).unwrap_or(0);
        let total = state.search_matches.len();
        let flags = format!(
            "{}{}{}{}",
            if state.search_match_case { "Aa" } else { "aA" },
            if state.search_whole_word {
                " | W"
            } else {
                " | *"
            },
            if state.search_regex_mode {
                " | Rx"
            } else {
                " | Txt"
            },
            if state.replace_active && state.replace_in_selection_only {
                " | Sel"
            } else {
                " | All"
            },
        );
        let focus_find = if state.search_input_focus == SearchInputFocus::Find {
            ">"
        } else {
            " "
        };
        let focus_replace = if state.search_input_focus == SearchInputFocus::Replace {
            ">"
        } else {
            " "
        };
        let find_label = if state.replace_active {
            format!(
                "{}FIND:{} | {}REPL:{} ({}/{})",
                focus_find,
                truncate_chars(&state.search_query, 12),
                focus_replace,
                truncate_chars(&state.replace_query, 12),
                active_pos,
                total
            )
        } else {
            format!(
                "{}FIND:{} ({}/{})",
                focus_find,
                truncate_chars(&state.search_query, 22),
                active_pos,
                total
            )
        };
        let find_label = format!("{} [{}]", find_label, flags);
        let find_buf = state.text_system.create_line_buffer(
            &find_label, design::type_scale::SM,
            editor_bounds.width - gutter_width - editor_hpad - 12.0,
        );
        state.text_system.draw_buffer(
            canvas,
            &find_buf,
            code_left_x + 4.0,
            body_top - 2.0,
            Color::from_hex(palette.accent),
        );
    }

    let has_selection = !selection.is_empty();
    let (selection_start, selection_end) = if selection.anchor.offset <= selection.head.offset {
        (selection.anchor, selection.head)
    } else {
        (selection.head, selection.anchor)
    };

    for row in 0..max_visible_lines {
        let line_idx = scroll_top + row;
        if line_idx >= lines.len() {
            break;
        }
        let line_top = body_top + row as f32 * line_height;

        if line_idx == cursor.line {
            canvas.fill_rect(
                Bounds::new(
                    editor_bounds.x + 1.0,
                    line_top + 1.0,
                    editor_bounds.width - 2.0,
                    line_height - 1.0,
                ),
                Color::from_hex(palette.line_highlight),
            );
        }

        if has_selection && line_idx >= selection_start.line && line_idx <= selection_end.line {
            let line_len = lines[line_idx].chars().count();
            let sel_start_col = if line_idx == selection_start.line {
                selection_start.column
            } else {
                0
            };
            let sel_end_col = if line_idx == selection_end.line {
                selection_end.column
            } else {
                line_len
            };

            let visible_start = sel_start_col.saturating_sub(scroll_left).min(max_chars);
            let visible_end = sel_end_col.saturating_sub(scroll_left).min(max_chars);
            if visible_end > visible_start {
                let sel_x = code_left_x + visible_start as f32 * char_width;
                let sel_w = (visible_end - visible_start) as f32 * char_width;
                canvas.fill_rect(
                    Bounds::new(sel_x, line_top + 2.0, sel_w.max(2.0), line_height - 4.0),
                    theme_hex(palette, 0x38486e, 0xd8c3a5),
                );
            }
        }

        if pane_focused && state.search_active && !state.search_query.is_empty() {
            for (match_idx, matched) in
                state
                    .search_matches
                    .iter()
                    .enumerate()
                    .filter(|(_, matched)| {
                        line_idx >= matched.start_line && line_idx <= matched.end_line
                    })
            {
                let line_len = lines[line_idx].chars().count();
                let match_start_col = if line_idx == matched.start_line {
                    matched.start_col
                } else {
                    0
                };
                let match_end_col = if line_idx == matched.end_line {
                    matched.end_col
                } else {
                    line_len
                };
                let visible_start = match_start_col.saturating_sub(scroll_left).min(max_chars);
                let visible_end = match_end_col.saturating_sub(scroll_left).min(max_chars);
                if visible_end > visible_start {
                    let sel_x = code_left_x + visible_start as f32 * char_width;
                    let sel_w = (visible_end - visible_start) as f32 * char_width;
                    let color = if state.active_search_match == Some(match_idx) {
                        Color::from_hex(palette.warning)
                    } else {
                        Color::from_hex(palette.warning).with_alpha(140)
                    };
                    canvas.fill_rect(
                        Bounds::new(sel_x, line_top + 2.0, sel_w.max(2.0), line_height - 4.0),
                        color,
                    );
                }
            }
        }

        let number_buf = state.text_system.create_code_buffer(
            &format!("{:>4}", line_idx + 1),
            code_font_size,
            gutter_width - 10.0,
        );
        state.text_system.draw_buffer(
            canvas,
            &number_buf,
            editor_bounds.x + 6.0,
            line_top + 14.0,
            Color::from_hex(palette.text_muted),
        );

        if pane_syntax_diag
            .as_ref()
            .map(|diag| diag.line == line_idx)
            .unwrap_or(false)
        {
            canvas.fill_rect(
                Bounds::new(
                    editor_bounds.x + 2.0,
                    line_top + 3.0,
                    3.0,
                    line_height - 6.0,
                ),
                Color::from_hex(palette.error),
            );
        }

        let line_text = lines[line_idx];
        let visible_start_col = scroll_left;
        let visible_end_col = scroll_left.saturating_add(max_chars);
        let segments = highlight_line(pane_language_name.as_deref(), line_text);
        for segment in segments {
            let start_col = segment.start_col.max(visible_start_col);
            let end_col = segment.end_col.min(visible_end_col);
            if end_col <= start_col {
                continue;
            }
            let segment_text: String = line_text
                .chars()
                .skip(start_col)
                .take(end_col - start_col)
                .collect();
            if segment_text.is_empty() {
                continue;
            }
            let seg_x = editor_bounds.x
                + gutter_width
                + editor_hpad
                + (start_col.saturating_sub(scroll_left)) as f32 * char_width;
            let line_buf = state.text_system.create_code_buffer(
                &segment_text,
                code_font_size,
                editor_bounds.width - gutter_width - editor_hpad,
            );
            state.text_system.draw_buffer(
                canvas,
                &line_buf,
                seg_x,
                line_top + 14.0,
                syntax_color(segment.class, palette),
            );
        }
    }

    // Gutter separator
    canvas.fill_rect(
        Bounds::new(
            editor_bounds.x + gutter_width - 4.0,
            body_top,
            1.0,
            body_height_total,
        ),
        Color::from_hex(palette.border),
    );

    // Cursor visual (solo cuando el panel tiene foco)
    if pane_focused {
        let visible_start = scroll_top;
        let visible_end = scroll_top.saturating_add(max_visible_lines);
        if cursor.line >= visible_start && cursor.line < visible_end {
            let row = cursor.line - scroll_top;
            let cursor_col = cursor.column.saturating_sub(scroll_left);
            let cursor_x = code_left_x + cursor_col as f32 * char_width;
            let cursor_top = body_top + row as f32 * line_height + 2.0;
            canvas.fill_rect(
                Bounds::new(cursor_x, cursor_top, 1.5, line_height - 4.0),
                Color::from_hex(palette.accent),
            );
        }
    }

    if show_search_results_panel {
        let panel_x = code_left_x + 4.0;
        let panel_y = body_top + body_height + 2.0;
        let panel_width = (editor_bounds.width - gutter_width - editor_hpad - 10.0).max(120.0);
        let panel_height = search_results_panel_height.max(24.0);
        let panel_bounds = Bounds::new(panel_x, panel_y, panel_width, panel_height);
        state.set_search_results_bounds(panel_bounds);
        canvas.fill_rect(
            panel_bounds,
            if state.replace_all_confirm_pending {
                Color::from_hex(palette.selection).with_alpha(150)
            } else {
                Color::from_hex(palette.surface)
            },
        );
        canvas.stroke_rect(
            panel_bounds,
            if state.replace_all_confirm_pending {
                Color::from_hex(palette.accent)
            } else {
                Color::from_hex(palette.border)
            },
            1.0,
        );

        let header = if state.replace_all_confirm_pending {
            format!(
                "Preview {} | Ctrl+Shift+R apply | Esc cancel",
                state.search_matches.len()
            )
        } else {
            format!("Matches {} (click to jump)", state.search_matches.len())
        };
        let header_buf = state
            .text_system
            .create_line_buffer(&header, design::type_scale::SM, panel_width - 10.0);
        state.text_system.draw_buffer(
            canvas,
            &header_buf,
            panel_x + 6.0,
            panel_y + 12.0,
            if state.replace_all_confirm_pending {
                Color::from_hex(palette.accent)
            } else {
                Color::from_hex(palette.text_muted)
            },
        );

        let total_matches = state.search_matches.len();
        let visible_rows = search_results_rows;
        let max_scroll = total_matches.saturating_sub(visible_rows);
        let start_idx = state.search_results_scroll.min(max_scroll);

        for row in 0..visible_rows {
            let match_idx = start_idx + row;
            let Some(matched) = state.search_matches.get(match_idx) else {
                break;
            };

            let row_y = panel_y
                + search_results_header_height
                + search_results_panel_padding_top
                + row as f32 * search_results_row_height;
            let row_bounds = Bounds::new(
                panel_x + 3.0,
                row_y,
                panel_width - 6.0,
                search_results_row_height,
            );
            if state.active_search_match == Some(match_idx) {
                canvas.fill_rect(row_bounds, Color::from_hex(palette.selection));
            }

            let preview_source = lines.get(matched.start_line).copied().unwrap_or("");
            let mut preview: String = preview_source
                .chars()
                .skip(matched.start_col)
                .take(32)
                .collect();
            if preview.is_empty() {
                preview = "<empty>".to_string();
            }
            if matched.end_line > matched.start_line {
                preview.push_str(" ...");
            }
            let label = format!(
                "{:>3}. L{}:{}-L{}:{} {}",
                match_idx + 1,
                matched.start_line + 1,
                matched.start_col + 1,
                matched.end_line + 1,
                matched.end_col + 1,
                preview
            );
            let label_buf = state.text_system.create_code_buffer(
                &truncate_chars(&label, 88),
                10.0,
                panel_width - 14.0,
            );
            state.text_system.draw_buffer(
                canvas,
                &label_buf,
                panel_x + 8.0,
                row_y + 11.0,
                if state.active_search_match == Some(match_idx) {
                    Color::from_hex(palette.text)
                } else {
                    Color::from_hex(palette.text_muted)
                },
            );

            state.add_click_target(row_bounds, ClickTargetAction::SearchMatchSelect(match_idx));
        }
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut iter = s.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        let Some(ch) = iter.next() else {
            return out;
        };
        out.push(ch);
    }
    if iter.next().is_some() {
        out.push_str("...");
    }
    out
}

fn relative_workspace_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn breadcrumb_segments(root: &Path, path: &Path) -> Vec<(String, PathBuf, bool)> {
    let relative = relative_workspace_path(root, path);
    let segments: Vec<&str> = relative.split('/').filter(|seg| !seg.is_empty()).collect();
    if segments.is_empty() {
        return vec![("untitled".to_string(), path.to_path_buf(), true)];
    }
    let mut out: Vec<(String, PathBuf, bool)> = Vec::new();
    let mut current = root.to_path_buf();
    let keep_from = segments.len().saturating_sub(3);

    if keep_from > 0 {
        for segment in &segments[..keep_from] {
            current = current.join(segment);
        }
        out.push(("...".to_string(), current.clone(), false));
    }

    for (idx, segment) in segments.iter().enumerate().skip(keep_from) {
        current = current.join(segment);
        out.push((
            (*segment).to_string(),
            current.clone(),
            idx + 1 == segments.len(),
        ));
    }

    out
}

fn short_session_id(session_id: &str) -> String {
    if session_id.len() > 18 {
        session_id[session_id.len() - 18..].to_string()
    } else {
        session_id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn breadcrumb_segments_compacts_deep_paths() {
        let root = PathBuf::from("/tmp/ws");
        let deep = PathBuf::from("/tmp/ws/a/b/c/d/e/file.rs");
        let segments = breadcrumb_segments(&root, &deep);
        let labels = segments
            .iter()
            .map(|(label, _, _)| label.clone())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["...", "d", "e", "file.rs"]);
        assert_eq!(segments[0].1, PathBuf::from("/tmp/ws/a/b/c"));
        assert!(!segments[0].2);
        assert!(segments.last().map(|v| v.2).unwrap_or(false));
    }

    #[test]
    fn relative_workspace_path_normalizes_separators() {
        let root = PathBuf::from("/tmp/ws");
        let path = PathBuf::from("/tmp/ws/src/main.rs");
        assert_eq!(relative_workspace_path(&root, &path), "src/main.rs");
    }
}
