//! # Llore GUI
//!
//! Aplicación principal de Llore Editor con UI propia.
//!
//! > **Arquitectura**: UI local → llore_brain → gateway configurado.

use llore_ui::app::{
    run, top_menu_entries, AppState, ChatMessage, ClickTargetAction, CommandPaletteAction,
    EditorPane, OverlayMode, PanelDock, SearchInputFocus, SessionTelemetryTimelineSource,
    manual_blocks, manual_sections, AgentTarget, ChatMenuItem, ChatPopover, ManualBlock, ProviderKind, SidebarPanel, SidebarProblemSeverity,
    TabSnapshot, TelemetryTimelineFilter, TopMenuKind,
    UiAppearancePreset,
    UiDensity, EDITOR_BODY_BOTTOM_PADDING, EDITOR_BODY_TOP_PADDING, EDITOR_GUTTER_WIDTH,
    EDITOR_TAB_BAR_HEIGHT, OVERLAY_RESULTS_MAX, SEARCH_RESULTS_MAX_ROWS,
};
use llore_ui::design;
use llore_ui::icons;
use llore_ui::syntax_highlight::{highlight_line, syntax_diagnostic, SyntaxClass};
use llore_ui::theme::{ThemePalette, UiTheme};
use llore_ui::{Bounds, Canvas, Color, Window};
use std::path::{Path, PathBuf};


/// Tamaño de los iconos de la barra de actividad.
/// Tamaño de los iconos de acción del explorador.
const CONTROL_ICON_SIZE: f32 = 12.0;

/// Alto de línea del cuerpo del mensaje (`font_size * 1.2`, con `font_size = 12`).
const CHAT_LINE_HEIGHT: f32 = 14.4;
/// Alto de cada línea de detalle: la meta y cada cita.
const CHAT_DETAIL_LINE: f32 = 14.0;
/// Recorte del cuerpo del mensaje. Antes eran 84 caracteres, luego 600: ambos
/// cortaban respuestas normales a media frase (una contestación con fuentes
/// ronda los 1 000–2 000). Un mensaje que no cabe en la columna se recorta por
/// el principio al colocarlo, no aquí.
const CHAT_MESSAGE_MAX_CHARS: usize = 4000;

const SEARCH_RESULTS_ROW_HEIGHT_BASE: f32 = 16.0;
const SEARCH_RESULTS_HEADER_HEIGHT_BASE: f32 = 16.0;
const SEARCH_RESULTS_PANEL_PADDING_TOP_BASE: f32 = 4.0;
const SEARCH_RESULTS_PANEL_PADDING_BOTTOM_BASE: f32 = 4.0;

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
    println!("║                          QUIRÓN                              ║");
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

    // Una carpeta pasada explícitamente tiene el mismo alcance que Abrir proyecto.
    let mut initial_project = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    run("Quirón", 1280, 720, move |window, state| {
        if let Some(project) = initial_project.take() { state.open_workspace(project); }
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
    // La maqueta da a cada fila del árbol 8 px de relleno arriba y abajo
    // sobre una línea de 13: unos 30. Con 16 la letra tocaba la fila de al
    // lado, que era buena parte de la sensación de lista apretada.
    let explorer_row_h = (28.0 * density_scale).clamp(22.0, 36.0);

    // Fondo base
    canvas.fill_rect(bounds, Color::from_hex(palette.background));

    // Barra superior compacta, alineada con editores de escritorio.
    let header_height = 36.0;
    canvas.fill_rect(
        Bounds::new(0.0, 0.0, bounds.width, header_height),
        Color::from_hex(palette.surface),
    );

    // La maqueta no tiene barra de estado inferior: sus indicadores viven en la
    // cabecera. Se conserva la variable a cero porque una docena de cálculos de
    // alto la restan; poniéndola a cero el contenido crece hasta el borde sin
    // tocar ninguno de ellos.
    let status_height = 0.0;

    // Sidebar izquierda (resizable). El techo sale del menor entre el máximo de
    // la maqueta y el 45 % de la ventana: en pantallas estrechas manda la
    // ventana, en anchas manda el diseño.
    let max_sidebar = (bounds.width * 0.45).min(llore_ui::app::MAX_SIDEBAR_WIDTH).max(llore_ui::app::MIN_SIDEBAR_WIDTH);
    let sidebar_width = state.sidebar_width.clamp(llore_ui::app::MIN_SIDEBAR_WIDTH, max_sidebar);
    state.sidebar_width = sidebar_width;
    // La maqueta no tiene tira de iconos: los siete paneles se abren desde la
    // paleta de comandos (`command_palette_reaches_every_sidebar_panel` lo
    // garantiza). Se conserva la variable a cero porque el resto del layout la
    // suma y la resta; a cero, la lateral se queda esos 40 px de ancho.
    let activity_bar_width = 0.0_f32;
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

    // === Layout general ===
    let main_x = match state.explorer_dock {
        PanelDock::Left => sidebar_width,
        PanelDock::Right => 0.0,
    };
    let main_y = header_height;
    // Cuarta columna: Segundo plano. Se monta si está visible y, además del
    // mínimo del chat, cabe su propio mínimo; se descuenta de la fila antes de
    // repartir chat y editor. Sin asa todavía: nace a su ancho de partida.
    let fila_total = (bounds.width - sidebar_width).max(240.0);
    // El editor decide primero (su regla ya reserva el mínimo de la columna).
    // Después la columna toma lo que sobra tras los mínimos de chat y editor,
    // entre su mínimo y su ancho de partida. Así la regla y el dibujo cuentan
    // lo mismo: antes la regla reservaba 252 y el dibujo se llevaba 360.
    let editor_montado = state.hay_archivo_abierto() && state.cabe_el_editor(bounds.width);
    let reservado = llore_ui::app::MIN_CHAT_WIDTH
        + if editor_montado {
            llore_ui::app::COLUMN_GAP + llore_ui::app::MIN_EDITOR_WIDTH
        } else {
            0.0
        };
    let fondo_montado = state.background_panel_visible
        && fila_total >= reservado + llore_ui::app::COLUMN_GAP + llore_ui::app::MIN_BACKGROUND_WIDTH;
    let fondo_width = if fondo_montado {
        llore_ui::app::DEFAULT_BACKGROUND_WIDTH
            .min(fila_total - reservado - llore_ui::app::COLUMN_GAP)
            .max(llore_ui::app::MIN_BACKGROUND_WIDTH)
    } else {
        0.0
    };
    let main_width = if fondo_montado {
        (fila_total - fondo_width - llore_ui::app::COLUMN_GAP).max(240.0)
    } else {
        fila_total
    };
    let content_height = (bounds.height - header_height - status_height).max(120.0);

    // El editor deja de ser la columna base. La base es el chat, y el editor se
    // monta a su lado solo si caben los mínimos de ambos —la regla que la
    // maqueta escribe como `cabeElEditor`—. Cuando no cabe no se estrujan los
    // dos: el editor no aparece y el chat se queda el centro entero.
    // Dos condiciones, como la maqueta: `!!s.editor && cabeElEditor()`. Esa
    // columna es para archivos. Si no hay ninguno abierto no se monta, y el
    // chat se queda la fila entera.
    let split_gap = if editor_montado {
        llore_ui::app::COLUMN_GAP
    } else {
        0.0
    };
    let split_available = (main_width - split_gap).max(220.0);
    let (editor_width, chat_width) = if editor_montado {
        let max_editor = (split_available - llore_ui::app::MIN_CHAT_WIDTH)
            .max(llore_ui::app::MIN_EDITOR_WIDTH)
            .min(llore_ui::app::MAX_EDITOR_WIDTH);
        let editor_width = (split_available * state.editor_split_ratio)
            .clamp(llore_ui::app::MIN_EDITOR_WIDTH, max_editor);
        state.editor_split_ratio = (editor_width / split_available).clamp(0.1, 0.9);
        (
            editor_width,
            (split_available - editor_width).max(llore_ui::app::MIN_CHAT_WIDTH),
        )
    } else {
        (0.0, split_available)
    };

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
    // Bandeja de escritura flotante. Medida sobre la maqueta: 40 px de margen
    // lateral, 28 del fondo. Estaba pegada al borde con 8 px, que es lo que la
    // hacía leerse como una barra y no como una bandeja.
    let bandeja_lado = design::space::XXL + design::space::SM;
    let bandeja_fondo = design::space::XL + design::space::XS;
    // La bandeja crece con el texto (hasta seis líneas) y con los adjuntos.
    let bandeja_texto_w = (chat_bounds.width - bandeja_lado * 2.0).max(120.0) - 24.0;
    let linea_bandeja = design::type_scale::MD * 1.4;
    let (_, alto_texto) = state.text_system.measure(
        if state.input_text.is_empty() { "x" } else { state.input_text.as_str() },
        design::type_scale::MD,
        bandeja_texto_w,
    );
    let lineas_bandeja = (alto_texto / linea_bandeja).round().clamp(1.0, 6.0);
    let adjuntos_alto = if state.pending_attachments.is_empty() { 0.0 } else { 26.0 };
    let bandeja_alto = 76.0 + (lineas_bandeja - 1.0) * linea_bandeja + adjuntos_alto;
    // Mientras no haya conversación la bandeja se queda en el centro de la
    // columna, como al entrar en Cursor: una columna vacía con la entrada
    // pegada al fondo se lee como un formulario abandonado. En cuanto el
    // usuario escribe, baja a su sitio y el hilo crece hacia arriba.
    let conversacion_vacia = !state.messages.iter().any(|m| m.is_user);
    let bandeja_y = if conversacion_vacia {
        chat_bounds.y + (chat_bounds.height - bandeja_alto) * 0.5
    } else {
        chat_bounds.y + chat_bounds.height - bandeja_fondo - bandeja_alto
    };
    let input_bounds = Bounds::new(
        chat_bounds.x + bandeja_lado,
        bandeja_y,
        (chat_bounds.width - bandeja_lado * 2.0).max(120.0),
        bandeja_alto,
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
    //
    // El editor es una tarjeta que flota sobre el suelo, como en la maqueta:
    // blanca en claro, con esquinas y sombra. Los 12 px de hueco con el chat
    // ya le dan sitio a la sombra, así que se dibuja a la medida de la región
    // sin encoger nada: el contenido sigue usando las mismas coordenadas.
    if editor_montado {
        let tarjeta_bg = if palette.is_light {
            Color::new(255, 255, 255)
        } else {
            Color::from_hex(palette.surface)
        };
        canvas.drop_shadow(
            editor_region_bounds,
            design::radius::LG,
            3.0,
            10.0,
            Color::from_hex(0x2D2B2B).with_alpha(41),
        );
        canvas.fill_rounded_rect(editor_region_bounds, design::radius::LG, tarjeta_bg);
    }
    canvas.fill_rect(chat_bounds, Color::from_hex(palette.surface));

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

    // La bandeja flota: primero la sombra, después la superficie. En claro es
    // blanco puro sobre el fondo cálido —así es como la maqueta la despega del
    // hilo—; en oscuro no hay blanco que valga y se usa la superficie.
    let bandeja_bg = if palette.is_light {
        Color::new(255, 255, 255)
    } else {
        Color::from_hex(palette.surface)
    };
    canvas.drop_shadow(
        input_bounds,
        design::radius::LG,
        3.0,
        10.0,
        Color::from_hex(0x2D2B2B).with_alpha(41),
    );
    canvas.fill_rounded_rect(input_bounds, design::radius::LG, bandeja_bg);
    if state.input_focused {
        canvas.stroke_rounded_rect(input_bounds, design::radius::LG, Color::from_hex(palette.accent), 1.0);
    }

    // === Cabecera: la ficha de identidad de la maqueta ===
    //
    // La barra de menús (File/Edit/View/Go/Project/Help) desaparece: sus 35
    // acciones ya viven en la paleta de comandos, incluida `Open Folder...`,
    // que era la única que faltaba y se añadió al retirarla. En su sitio va lo
    // que el diseño pone ahí: la marca y el proyecto abierto.
    let brand_h = 22.0;
    let brand_y = (header_height - brand_h) * 0.5;
    let logo_bounds = Bounds::new(design::space::MD, brand_y, brand_h, brand_h);
    canvas.fill_rounded_rect(
        logo_bounds,
        design::radius::SM,
        Color::from_hex(palette.accent),
    );
    let logo_buf = state
        .text_system
        .create_heading_buffer("Q", design::type_scale::SM, brand_h);
    state.text_system.draw_buffer(
        canvas,
        &logo_buf,
        logo_bounds.x + 7.0,
        brand_y + brand_h * 0.5 + design::type_scale::SM * 0.36,
        Color::from_hex(palette.background),
    );

    let marca_x = logo_bounds.x + brand_h + design::space::SM;
    let marca_buf = state
        .text_system
        .create_heading_buffer("Quirón", design::type_scale::MD, 120.0);
    state.text_system.draw_buffer(
        canvas,
        &marca_buf,
        marca_x,
        brand_y + brand_h * 0.5 + design::type_scale::MD * 0.36,
        Color::from_hex(palette.text),
    );

    // El proyecto abierto, en monoespaciada y apagado, como en la maqueta.
    let proyecto = state
        .workspace_root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("sin proyecto");
    let proyecto_buf =
        state
            .text_system
            .create_code_buffer(proyecto, design::type_scale::XS, 260.0);
    state.text_system.draw_buffer(
        canvas,
        &proyecto_buf,
        marca_x + 62.0,
        brand_y + brand_h * 0.5 + design::type_scale::XS * 0.36,
        Color::from_hex(palette.text_muted),
    );

    // Los menús ya no existen; el desplegable se queda sin nada que abrir.
    let menu_layout: Vec<(TopMenuKind, Bounds)> = Vec::new();

    // === Título central discreto ===
    let ws_name = state
        .workspace_root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("Sin proyecto");
    let window_title = format!("{} — Quirón", ws_name);
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

    // === Cabeza de la lateral ===
    //
    // La maqueta abre la columna con la acción primaria. Debajo van las
    // pestañas plegables —conversaciones y repositorios— y después la búsqueda
    // y el árbol del proyecto (`render_sidebar_sections`).
    let boton_h = 38.0;
    let boton_y = header_height + design::space::MD;
    let boton_bounds = Bounds::new(sidebar_content_x, boton_y, sidebar_content_width, boton_h);
    canvas.fill_rounded_rect(
        boton_bounds,
        design::radius::MD,
        Color::from_hex(palette.accent),
    );
    let boton_buf = state.text_system.create_heading_buffer(
        "+   Nuevo chat",
        design::type_scale::SM,
        sidebar_content_width - 20.0,
    );
    state.text_system.draw_buffer(
        canvas,
        &boton_buf,
        boton_bounds.x + 14.0,
        boton_y + boton_h * 0.5 + design::type_scale::SM * 0.36,
        Color::from_hex(palette.background),
    );
    state.add_click_target(boton_bounds, ClickTargetAction::NewChat);

    // Fila de búsqueda. En la maqueta el atajo es ⌘P; aquí es Ctrl+P, que es el
    // que realmente funciona en esta máquina.
    let buscar_h = 26.0;
    let buscar_y = render_sidebar_sections(
        canvas,
        state,
        &palette,
        sidebar_content_x,
        sidebar_content_width,
        boton_y + boton_h + design::space::SM,
    );
    let buscar_bounds = Bounds::new(sidebar_content_x, buscar_y, sidebar_content_width, buscar_h);
    let buscar_base = buscar_y + buscar_h * 0.5 + design::type_scale::SM * 0.36;
    let lupa = state.text_system.create_icon_buffer(icons::SEARCH, 12.0);
    state.text_system.draw_buffer(
        canvas,
        &lupa,
        buscar_bounds.x + 2.0,
        buscar_base,
        Color::from_hex(palette.text_muted),
    );
    let buscar_buf =
        state
            .text_system
            .create_line_buffer("Buscar", design::type_scale::SM, sidebar_content_width - 60.0);
    state.text_system.draw_buffer(
        canvas,
        &buscar_buf,
        buscar_bounds.x + 22.0,
        buscar_base,
        Color::from_hex(palette.text_muted),
    );
    let atajo_buf = state
        .text_system
        .create_code_buffer("Ctrl+P", design::type_scale::XS, 60.0);
    state.text_system.draw_buffer(
        canvas,
        &atajo_buf,
        buscar_bounds.x + sidebar_content_width - 44.0,
        buscar_base,
        Color::from_hex(palette.text_muted),
    );
    state.add_click_target(buscar_bounds, ClickTargetAction::ActivityQuickOpen);

    // Rótulos en castellano, como la maqueta.
    let sidebar_title_label = match state.sidebar_panel {
        SidebarPanel::Explorer => "ARCHIVOS",
        SidebarPanel::Search => "BUSCAR",
        SidebarPanel::Git => "CONTROL DE VERSIONES",
        SidebarPanel::Problems => "PROBLEMAS",
        SidebarPanel::Outline => "ESQUEMA",
        SidebarPanel::Appearance => "APARIENCIA",
        SidebarPanel::Security => "SEGURIDAD",
    };
    let sidebar_title =
        state
            .text_system
            .create_line_buffer(sidebar_title_label, design::type_scale::XS, sidebar_content_width);

    let titulo_base = buscar_y + buscar_h + design::space::XL;
    state.text_system.draw_buffer(
        canvas,
        &sidebar_title,
        sidebar_content_x,
        titulo_base,
        Color::from_hex(palette.text_muted),
    );
    let explorer_start_y = titulo_base + design::space::MD;
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
                // `y` es la línea base del texto; la fila va centrada sobre ella.
                let top = list_start_y + idx as f32 * explorer_row_h - explorer_row_h * 0.5;
                let y = top + explorer_row_h * 0.5 + design::type_scale::SM * 0.36;
                let row_bounds = Bounds::new(
                    sidebar_content_x,
                    top,
                    sidebar_content_width,
                    explorer_row_h,
                );

                let indent_step = state.explorer_indent_step();
                let indent = sidebar_content_x + 6.0 + entry.depth as f32 * indent_step;
                // Como la maqueta: chevrón para las carpetas y, para los
                // archivos, la extensión en monoespaciada apagada delante del
                // nombre. Sin guiones ni cebra.
                let (marca, marca_mono) = if entry.is_dir {
                    (
                        if state.is_dir_expanded(&entry.path) { "⌄" } else { "›" }.to_string(),
                        false,
                    )
                } else {
                    (
                        std::path::Path::new(&entry.name)
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.chars().take(4).collect::<String>())
                            .unwrap_or_default(),
                        true,
                    )
                };
                let label = entry.name.clone();
                let x = indent + 26.0;
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

                // Píldora sobre la superficie para el archivo abierto, como
                // `chat_panel.rs` en la maqueta; el seleccionado con teclado va
                // en el rubor de la selección, más tenue.
                if is_selected {
                    canvas.fill_rounded_rect(
                        row_bounds,
                        design::radius::MD,
                        Color::from_hex(palette.selection),
                    );
                }
                if (is_primary_active || is_secondary_active) && !entry.is_dir {
                    canvas.fill_rounded_rect(
                        row_bounds,
                        design::radius::MD,
                        Color::from_hex(palette.surface),
                    );
                }

                if marca_mono {
                    let m_buf = state.text_system.create_code_buffer(
                        &marca,
                        design::type_scale::XS,
                        24.0,
                    );
                    state.text_system.draw_buffer(
                        canvas,
                        &m_buf,
                        indent,
                        y,
                        Color::from_hex(palette.text_muted),
                    );
                } else {
                    let m_buf = state
                        .text_system
                        .create_line_buffer(&marca, design::type_scale::SM, 16.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &m_buf,
                        indent + 2.0,
                        y,
                        Color::from_hex(palette.text_muted),
                    );
                }

                let color = Color::from_hex(palette.text);
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
                ("Modernist Light", UiTheme::ModernistLight),
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
    // Sin sitio no hay columna: ni editor ni bienvenida. El chat, que ya ocupa
    // toda la fila, es lo único que se pinta.
    if editor_montado {
        render_editor_pane(canvas, state, primary_editor_bounds, EditorPane::Primary);
        if let Some(secondary_bounds) = secondary_editor_bounds {
            render_editor_pane(canvas, state, secondary_bounds, EditorPane::Secondary);
        }
    }

    // === Chat: cabecera del hilo ===
    //
    // La maqueta encabeza el hilo con su título en Archivo 800 a 16 px y, a su
    // lado, el contador en monoespaciada. No hay títulos de sesión guardados,
    // así que el título es la primera pregunta del hilo —lo que hacen Cursor y
    // ChatGPT— y, si aún no la hay, «Conversación». El 16 y el relleno
    // 26/40/14 son medidos, no tokens: van con su cita.
    let titulo_hilo: String = state
        .messages
        .iter()
        .find(|m| m.is_user)
        .map(|m| truncate_chars(m.content.lines().next().unwrap_or(""), 48))
        .unwrap_or_else(|| "Conversación".to_string());
    let n_mensajes = state
        .messages
        .iter()
        .filter(|m| m.is_user || m.meta.is_none())
        .count();
    let cabecera_x = chat_bounds.x + bandeja_lado;
    let cabecera_base = chat_bounds.y + 26.0 + 16.0;
    let titulo_w = (chat_bounds.width - bandeja_lado * 2.0 - 120.0).max(80.0);
    let titulo_buf = state
        .text_system
        .create_heading_buffer(&titulo_hilo, 16.0, titulo_w);
    let titulo_ancho = titulo_buf.layout_runs().map(|run| run.line_w).fold(0.0_f32, f32::max);
    // En columna estrecha el título se envuelve en varias líneas: los mensajes
    // empiezan debajo de la última, no debajo de la primera.
    let titulo_lineas = titulo_buf.layout_runs().count().max(1) as f32;
    let titulo_alto_extra = (titulo_lineas - 1.0) * 16.0 * 1.2;
    state.text_system.draw_buffer(
        canvas,
        &titulo_buf,
        cabecera_x,
        cabecera_base,
        Color::from_hex(palette.text),
    );
    let contador = format!(
        "{n_mensajes} mensaje{}",
        if n_mensajes == 1 { "" } else { "s" }
    );
    let contador_buf = state
        .text_system
        .create_code_buffer(&contador, design::type_scale::XS, 140.0);
    state.text_system.draw_buffer(
        canvas,
        &contador_buf,
        cabecera_x + titulo_ancho.min(titulo_w) + 14.0,
        cabecera_base,
        Color::from_hex(palette.text_muted),
    );
    // El modelo y el «+ contexto» viven dentro de la bandeja, en su fila
    // inferior, no sueltos en la cabecera del panel.
    // Ancho del chip del modelo según su nombre, para dejar sitio a los demás.
    let model_chip_w = (12.0 + state.selected_ai_model().chars().count() as f32 * 6.4).clamp(44.0, 110.0);
    let model_bounds = Bounds::new(
        input_bounds.x + 46.0,
        input_bounds.y + input_bounds.height - 28.0,
        model_chip_w,
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
    state.add_click_target(model_bounds, ClickTargetAction::ChatPopoverToggle(ChatPopover::Models));

    // Manos y longitud de respuesta, como chips pequeños tras el modelo; el
    // botón de enviar (o parar) a la derecha, y el gasto de contexto al lado.
    let mut chip_x = model_bounds.x + model_bounds.width + 6.0;
    let boton_w = 26.0;
    let boton = Bounds::new(input_bounds.x + input_bounds.width - boton_w - 10.0, model_bounds.y - 3.0, boton_w, boton_w);
    let mut chip = |state: &mut AppState, canvas: &mut Canvas, texto: &str, activo: bool, accion: ClickTargetAction| {
        let w = 10.0 + texto.chars().count() as f32 * 6.2;
        if chip_x + w > boton.x - 8.0 {
            return;
        }
        let b = Bounds::new(chip_x, model_bounds.y, w, 20.0);
        canvas.fill_rounded_rect(b, 4.0, Color::from_hex(palette.background).with_alpha(150));
        let buf = state.text_system.create_line_buffer(texto, design::type_scale::XS, w);
        state.text_system.draw_buffer(
            canvas,
            &buf,
            b.x + 5.0,
            b.y + 13.0,
            Color::from_hex(if activo { palette.text } else { palette.text_muted }),
        );
        state.add_click_target(b, accion);
        chip_x += w + 6.0;
    };
    let manos_texto = if state.chat_tools_enabled { "manos ✓" } else { "manos ✗" };
    let manos_activas = state.chat_tools_enabled;
    chip(state, canvas, manos_texto, manos_activas, ClickTargetAction::ChatMenu(ChatMenuItem::ToggleTools));
    let longitud = format!("resp. {}", state.response_length.label());
    chip(state, canvas, &longitud, true, ClickTargetAction::ChatMenu(ChatMenuItem::CycleLength));
    if let Some(sn) = state.background_snapshot {
        if sn.token_budget > 0 {
            let gasto = format!("{:.1}k/{:.0}k", sn.tokens_used as f32 / 1000.0, sn.token_budget as f32 / 1000.0);
            chip(state, canvas, &gasto, false, ClickTargetAction::ToggleBackgroundPanel);
        }
    }
    // Enviar / parar.
    let hay_texto = !state.input_text.trim().is_empty();
    let (glifo, accion, fondo) = if state.loading {
        ("■", ClickTargetAction::ChatStop, Color::from_hex(palette.error))
    } else {
        ("➤", ClickTargetAction::ChatSend, Color::from_hex(palette.accent).with_alpha(if hay_texto { 255 } else { 90 }))
    };
    canvas.fill_rounded_rect(boton, boton_w * 0.5, fondo);
    let buf = state.text_system.create_line_buffer(glifo, design::type_scale::SM, boton_w);
    state.text_system.draw_buffer(canvas, &buf, boton.x + 7.0, boton.y + 18.0, Color::from_hex(palette.background));
    state.add_click_target(boton, accion);

    let add_ctx_bounds = Bounds::new(input_bounds.x + 14.0, model_bounds.y, 24.0, 20.0);
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
    state.add_click_target(add_ctx_bounds, ClickTargetAction::ChatPopoverToggle(ChatPopover::Actions));
    canvas.draw_line(
        chat_bounds.x,
        chat_bounds.y + 32.0,
        chat_bounds.x + chat_bounds.width,
        chat_bounds.y + 32.0,
        Color::from_hex(palette.border).with_alpha(90),
        1.0,
    );

    let mut messages_start_y =
        chat_bounds.y + 26.0 + 16.0 + 14.0 + titulo_alto_extra + design::space::SM;
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
        canvas.fill_rounded_rect(panel_bounds, design::radius::MD, Color::from_hex(palette.surface));
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
        .take(40)
        .cloned()
        .collect::<Vec<_>>();
    let messages_to_show: Vec<_> = messages_to_show.into_iter().rev().collect();

    // Los mensajes comparten margen con la bandeja y no pasan de 660 px de
    // ancho, como en la maqueta: una línea de prosa más larga cansa.
    let msg_card_x = chat_bounds.x + bandeja_lado;
    let msg_card_w = (chat_bounds.width - bandeja_lado * 2.0).clamp(80.0, 660.0);
    let msg_limit_y = input_bounds.y - design::space::MD;
    let msg_text_w = (msg_card_w - 40.0).max(40.0);

    // La altura de cada tarjeta depende de cuántas líneas ocupe su texto al
    // envolverse. Se mide antes de dibujar; suponerla fija apilaba los mensajes.
    // Índice absoluto en `state.messages` del primero que se enseña: el
    // plegado de los bloques de pensamiento se guarda por ese índice.
    let primer_indice = state.messages.len().saturating_sub(messages_to_show.len());
    let desplegados: Vec<bool> = (0..messages_to_show.len())
        .map(|i| state.expanded_thoughts.contains(&(primer_indice + i)))
        .collect();

    let measured: Vec<(ChatMessage, String, f32, f32)> = messages_to_show
        .into_iter()
        .enumerate()
        .map(|(i, msg)| {
            // Bloque de pensamiento: una línea de encabezado y, si está
            // desplegado, una por cada herramienta usada. Sin tarjeta.
            if msg.meta.as_deref() == Some("tools") {
                let cuerpo = msg.content.lines().count().saturating_sub(1) as f32;
                let block_h = CHAT_LINE_HEIGHT
                    + if desplegados[i] { cuerpo * CHAT_DETAIL_LINE + design::space::XS } else { 0.0 }
                    + design::space::SM;
                let text = msg.content.clone();
                return (msg, text, CHAT_LINE_HEIGHT, block_h);
            }

            let text = truncate_chars(&msg.content, CHAT_MESSAGE_MAX_CHARS);
            // Se mide al mismo tamaño al que se dibuja. Medir a 12 y dibujar a
            // 14,5 dejaba las tarjetas cortas y los mensajes se pisaban.
            let (_, text_h) = state
                .text_system
                .measure(&text, design::type_scale::MD, msg_text_w);
            let text_h = text_h.max(CHAT_LINE_HEIGHT);

            // Como la maqueta: etiqueta de rol encima y, solo para el usuario,
            // burbuja con relleno 18/20. La respuesta de Quirón va sin burbuja.
            let etiqueta = design::type_scale::XS + design::space::SM;
            let relleno = if msg.is_user { 18.0 * 2.0 } else { 0.0 };
            // `meta` es una etiqueta interna («tools», «connection_error»…):
            // gobierna el render pero no se enseña. Solo las citas añaden alto.
            let mut block_h = etiqueta + relleno + text_h + design::space::SM;
            let fuentes = msg.citations.len().min(2) + msg.code_sources.len().min(8);
            if fuentes > 0 {
                block_h += design::space::XS + fuentes as f32 * CHAT_DETAIL_LINE;
            }

            (msg, text, text_h, block_h)
        })
        .collect();

    // Se coloca desde el último mensaje hacia arriba, desplazado por la rueda:
    // con 0 la respuesta recién llegada queda pegada a la entrada; hacia
    // arriba aparecen las anteriores. Lo que sale de la banda visible se
    // recorta línea a línea al dibujar, porque el lienzo no recorta.
    let alto_visible = msg_limit_y - messages_start_y;
    let total: f32 = measured.iter().map(|m| m.3 + 6.0).sum::<f32>() - 6.0;
    state.chat_scroll_max = (total - alto_visible).max(0.0);
    if state.chat_scroll > state.chat_scroll_max {
        state.chat_scroll = state.chat_scroll_max;
    }
    // Mientras piensa, el holograma pequeño gira donde va a salir la
    // respuesta; los mensajes se apilan por encima de él.
    let hueco_pensando = if state.loading { 96.0 } else { 0.0 };
    let mut placements: Vec<(usize, f32)> = Vec::new();
    let mut cursor_y = msg_limit_y + state.chat_scroll - hueco_pensando;
    for (index, (_, _, _, block_h)) in measured.iter().enumerate().rev() {
        let top = cursor_y - block_h;
        if cursor_y <= messages_start_y {
            break;
        }
        if top < msg_limit_y {
            placements.push((index, top));
        }
        cursor_y = top - 6.0;
    }
    placements.reverse();

    // Aviso de que hay más arriba: el primero colocado empieza por encima de la
    // banda, o quedan mensajes sin colocar. Deja hueco para no pisar la línea.
    let hay_mas_arriba = placements
        .first()
        .map_or(false, |(index, top)| *top < messages_start_y || *index > 0);
    let banda_top = messages_start_y + if hay_mas_arriba { 18.0 } else { 8.0 };
    if hay_mas_arriba {
        let aviso = state.text_system.create_line_buffer(
            "↑ mensajes anteriores (rueda)",
            design::type_scale::XS,
            msg_card_w,
        );
        state.text_system.draw_buffer(
            canvas,
            &aviso,
            msg_card_x,
            messages_start_y + design::type_scale::XS,
            Color::from_hex(palette.text_muted),
        );
    }
    let visible = |base: f32| base >= banda_top && base <= msg_limit_y;

    if state.loading {
        let lado = 72.0;
        let cx = msg_card_x + lado * 0.5 + 6.0;
        let cy = msg_limit_y - hueco_pensando * 0.5;
        draw_brain_hologram(
            canvas,
            cx,
            cy,
            lado,
            state.thinking_secs(),
            (0.0, 0.0),
            None,
            Color::from_hex(palette.accent),
            Color::from_hex(palette.text_muted),
        );
        let ultima = state.chat_activity_lines().last().cloned().unwrap_or_else(|| "pensando".to_string());
        let buf = state.text_system.create_line_buffer(
            &truncate_chars(&ultima, 60),
            design::type_scale::XS,
            msg_card_w - lado - 20.0,
        );
        state.text_system.draw_buffer(canvas, &buf, cx + lado * 0.5 + 12.0, cy + 4.0, Color::from_hex(palette.text_muted));
    }

    for (index, msg_y) in placements {
        let (msg, text, text_h, block_h) = &measured[index];
        let (msg, text, text_h, block_h) = (msg, text.as_str(), *text_h, *block_h);

        // === Bloque de pensamiento ===
        // Como en la maqueta: «› Pensó 8 s · leyó 3 archivos» en apagado, sin
        // tarjeta, y al abrirlo el detalle en monoespaciada. El encabezado es
        // la primera línea del contenido; lo escribe la ruta de envío.
        if msg.meta.as_deref() == Some("tools") {
            let indice_abs = primer_indice + index;
            let abierto = desplegados[index];
            let mut lineas = text.lines();
            let encabezado = lineas.next().unwrap_or("");
            let cabecera_bounds = Bounds::new(msg_card_x, msg_y, msg_card_w, CHAT_LINE_HEIGHT + 4.0);
            let base = msg_y + CHAT_LINE_HEIGHT;
            if visible(base) {
                let chevron = state
                    .text_system
                    .create_line_buffer(if abierto { "⌄" } else { "›" }, design::type_scale::SM, 16.0);
                state.text_system.draw_buffer(
                    canvas,
                    &chevron,
                    msg_card_x + 10.0,
                    base,
                    Color::from_hex(palette.text_muted),
                );
                let enc_buf = state.text_system.create_line_buffer(
                    encabezado,
                    design::type_scale::SM,
                    msg_card_w - 34.0,
                );
                state.text_system.draw_buffer(
                    canvas,
                    &enc_buf,
                    msg_card_x + 26.0,
                    base,
                    Color::from_hex(palette.text_muted),
                );
                state.add_click_target(cabecera_bounds, ClickTargetAction::ToggleThought(indice_abs));
            }

            if abierto {
                let mut y = base + design::space::XS + CHAT_DETAIL_LINE;
                for linea in lineas {
                    if visible(y) {
                        let l_buf = state.text_system.create_code_buffer(
                            &truncate_chars(linea, 110),
                            design::type_scale::XS,
                            msg_card_w - 40.0,
                        );
                        state.text_system.draw_buffer(
                            canvas,
                            &l_buf,
                            msg_card_x + 30.0,
                            y,
                            Color::from_hex(palette.text_muted),
                        );
                    }
                    y += CHAT_DETAIL_LINE;
                }
            }
            continue;
        }

        let card_bounds = Bounds::new(msg_card_x, msg_y, msg_card_w, block_h);

        // Etiqueta de rol solo para el usuario; la respuesta no la necesita:
        // Quirón es el programa, no un interlocutor que se presenta.
        let etiqueta_buf = state.text_system.create_label_buffer("TÚ", design::type_scale::XS, 120.0);
        let etiqueta_base = card_bounds.y + design::type_scale::XS;
        if msg.is_user && visible(etiqueta_base) {
            state.text_system.draw_buffer(
                canvas,
                &etiqueta_buf,
                card_bounds.x,
                etiqueta_base,
                Color::from_hex(palette.text_muted),
            );
        }

        // El cuerpo del mensaje siempre en color de texto. El acento distingue
        // el rol y las citas; usarlo para prosa larga la vuelve ilegible.
        let text_color = Color::from_hex(palette.text);
        let cuerpo_top = etiqueta_base + design::space::SM;
        let (texto_x, texto_top) = if msg.is_user {
            // Burbuja del usuario: superficie, radio 16, relleno 18/20.
            let burbuja = Bounds::new(
                card_bounds.x,
                cuerpo_top,
                msg_card_w,
                text_h + 18.0 * 2.0,
            );
            // Recorte manual a la banda visible: el lienzo no recorta.
            let top = burbuja.y.max(messages_start_y);
            let bottom = (burbuja.y + burbuja.height).min(msg_limit_y);
            if bottom > top {
                canvas.fill_rounded_rect(
                    Bounds::new(burbuja.x, top, burbuja.width, bottom - top),
                    design::radius::LG,
                    Color::from_hex(palette.surface),
                );
            }
            (card_bounds.x + 20.0, cuerpo_top + 18.0)
        } else {
            (card_bounds.x, cuerpo_top)
        };
        let msg_buf = state.text_system.create_buffer(text, design::type_scale::MD, msg_text_w);
        state.text_system.draw_buffer_within(
            canvas,
            &msg_buf,
            texto_x,
            texto_top + design::type_scale::MD,
            text_color,
            banda_top,
            msg_limit_y,
        );

        // Los detalles empiezan donde termina el texto, no a una altura fija.
        // `detail_y` es una línea base: va el hueco más el ascenso de la letra
        // por debajo del cuerpo. Sumarle dos píxeles al pie del cuerpo la
        // ponía encima de su última línea.
        let mut detail_y = texto_top
            + text_h
            + (if msg.is_user { 18.0 } else { 0.0 })
            + design::space::XS
            + design::type_scale::SM;

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
            if visible(citation_y) {
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
            }
            detail_y += CHAT_DETAIL_LINE;
        }

        // Fichas de código: la fuente se enseña como ruta:rango y símbolo, y al
        // pulsarla se abre el archivo en esa línea.
        for hint in msg.code_sources.iter().take(8) {
            // Primero lo que distingue la fuente (archivo, rango y símbolo); la
            // ruta completa va en la barra de estado al pulsarla. En columna
            // estrecha una ruta larga se comía todo lo demás.
            let archivo = hint.path.rsplit('/').next().unwrap_or(&hint.path);
            let source_text = truncate_chars(
                &format!("-> {archivo}:{}-{} · {}", hint.start_line, hint.end_line, hint.symbol),
                80,
            );
            let source_x = card_bounds.x + 10.0;
            let source_y = detail_y;
            let source_buf =
                state
                    .text_system
                    .create_line_buffer(&source_text, design::type_scale::SM, card_bounds.width - 18.0);
            if visible(source_y) {
                state.text_system.draw_buffer(
                    canvas,
                    &source_buf,
                    source_x,
                    source_y,
                    Color::from_hex(palette.accent),
                );
                state.add_click_target(
                    Bounds::new(source_x, source_y - 10.0, card_bounds.width - 20.0, 14.0),
                    ClickTargetAction::CodeSource(hint.clone()),
                );
            }
            detail_y += CHAT_DETAIL_LINE;
        }
    }

    // === Input de chat ===
    // Con el campo enfocado y vacío se mostraba el texto de ayuda y ningún
    // cursor: no había forma de saber que ya se podía escribir.
    // Adjuntos pendientes: chips arriba de la bandeja, con su aspa.
    let mut texto_top = input_bounds.y + 26.0;
    if !state.pending_attachments.is_empty() {
        let mut ax = input_bounds.x + 12.0;
        let adjuntos = state.pending_attachments.clone();
        for (i, adjunto) in adjuntos.iter().enumerate() {
            let etiqueta = format!("📎 {} ×", truncate_chars(&adjunto.label, 28));
            let w = 12.0 + etiqueta.chars().count() as f32 * 6.2;
            if ax + w > input_bounds.x + input_bounds.width - 12.0 {
                break;
            }
            let b = Bounds::new(ax, input_bounds.y + 8.0, w, 20.0);
            canvas.fill_rounded_rect(b, 6.0, Color::from_hex(palette.selection).with_alpha(120));
            let buf = state.text_system.create_line_buffer(&etiqueta, design::type_scale::XS, w);
            state.text_system.draw_buffer(canvas, &buf, b.x + 6.0, b.y + 13.0, Color::from_hex(palette.text));
            state.add_click_target(b, ClickTargetAction::ChatAttachmentRemove(i));
            ax += w + 6.0;
        }
        texto_top += 26.0;
    }
    // El cursor parpadea (caret_on lo lleva el bucle de espera), se queda
    // fijo al escribir y va donde está de verdad, no siempre al final.
    let caret = if state.input_focused && state.caret_on { "|" } else { "" };
    let input_display = match (state.input_text.is_empty(), state.input_focused) {
        (true, true) => caret.to_string(),
        (true, false) => "Pregunta sobre el proyecto…".to_string(),
        (false, _) => {
            let cursor = state.input_cursor.min(state.input_text.chars().count());
            let byte = state.input_text.char_indices().nth(cursor).map(|(i, _)| i).unwrap_or(state.input_text.len());
            format!("{}{caret}{}", &state.input_text[..byte], &state.input_text[byte..])
        }
    };
    let input_buf = state.text_system.create_buffer(&input_display, design::type_scale::MD, input_bounds.width - 24.0);
    let input_color = if state.input_text.is_empty() && !state.input_focused {
        Color::from_hex(palette.text_muted)
    } else {
        Color::from_hex(palette.text)
    };
    state.text_system.draw_buffer(canvas, &input_buf, input_bounds.x + 12.0, texto_top, input_color);

    // Desplegables de la barra: el menú «+» y la lista de modelos, encima de
    // la bandeja y por encima de los mensajes.
    if state.chat_popover != ChatPopover::None {
        render_chat_popover(canvas, state, &palette, input_bounds, chat_bounds);
    }

    // === Indicadores de la cabecera ===
    //
    // La maqueta lleva a la derecha dos cosas: el estado del brain y el
    // conmutador de Segundo plano. Aquí van el brain, git y el estado; fuera
    // el archivo activo, el modo del editor y «0 problems», que ya se enseñan
    // en la barra de pestañas del propio editor (`pane_status`) o se abren
    // desde la paleta. Duplicarlos arriba era lo que llenaba la cabecera de
    // ruido.
    let conn_health_label = state.quiron_connection_health_label();
    let conn_health_ok = conn_health_label == "health=ok";
    let conn_health_checking = conn_health_label == "health=checking";

    // «● brain :8766»: el punto lleva el color del estado; el texto, el puerto
    // real de la conexión.
    // «● Agentes»: el punto lleva el color del estado del cerebro; el nombre
    // dice lo que se abre al pulsarlo.
    let connection_label = "● Agentes".to_string();
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
            "Manual".to_string(),
            Color::from_hex(palette.surface),
            Color::from_hex(palette.text_muted),
            Color::from_hex(palette.surface),
            Some(ClickTargetAction::TopMenuExecute(CommandPaletteAction::OpenManual)),
        ),
        (
            "Segundo plano".to_string(),
            Color::from_hex(palette.surface),
            if state.background_panel_visible {
                Color::from_hex(palette.text)
            } else {
                Color::from_hex(palette.text_muted)
            },
            Color::from_hex(palette.surface),
            Some(ClickTargetAction::ToggleBackgroundPanel),
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
            "pensando…".to_string(),
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

    // Los indicadores se dibujan en la cabecera y de derecha a izquierda, de
    // modo que el primero de la lista —el más importante— queda pegado al
    // borde. Se detienen antes de llegar al menú superior.
    let chip_h = (header_height - 14.0).max(16.0);
    let chip_y = (header_height - chip_h) * 0.5;
    let mut chip_right = bounds.width - design::space::SM;
    for (label, bg, fg, border, action) in status_chips {
        let chip_w = (label.chars().count() as f32 * 6.4 + 14.0).clamp(44.0, bounds.width * 0.34);
        if chip_right - chip_w < bounds.width * 0.45 {
            break;
        }
        let chip_bounds = Bounds::new(chip_right - chip_w, chip_y, chip_w, chip_h);
        canvas.fill_rounded_rect(chip_bounds, design::radius::SM, bg);
        if border != bg {
            canvas.stroke_rect(chip_bounds, border, 1.0);
        }
        let chip_buf =
            state
                .text_system
                .create_line_buffer(&label, design::type_scale::XS, chip_w - 10.0);
        state.text_system.draw_buffer(
            canvas,
            &chip_buf,
            chip_bounds.x + 6.0,
            chip_y + chip_h * 0.5 + design::type_scale::XS * 0.36,
            fg,
        );
        if let Some(action) = action {
            state.add_click_target(chip_bounds, action);
        }
        chip_right -= chip_w + design::space::XS;
    }

    render_top_menu_dropdown(canvas, state, palette, &menu_layout);

    // === Columna de Segundo plano ===
    //
    // Lo que Quirón hizo en la última respuesta, con datos medidos: cada
    // herramienta con su duración, qué archivos leyó, y las métricas de
    // delegación al cierre. Lo que la maqueta enseña y aquí no existe —el
    // razonamiento del modelo, la consulta a la memoria vectorial— no se
    // dibuja; el hueco queda para cuando exista.
    if fondo_montado {
        let fondo = Bounds::new(
            main_x + main_width + llore_ui::app::COLUMN_GAP,
            main_y,
            fondo_width,
            content_height,
        );
        let fx = fondo.x + design::space::MD;
        let fw = (fondo.width - design::space::MD * 2.0).max(60.0);
        let tope = fondo.y + fondo.height - design::space::LG;
        let paso = CHAT_DETAIL_LINE + 4.0;
        let mut y = fondo.y + design::space::XL;

        // Cabecera: punto de acento, rótulo, chevrón para ocultar.
        let punto = Bounds::new(fx, y - 7.0, 7.0, 7.0);
        canvas.fill_rounded_rect(punto, 3.5, Color::from_hex(palette.accent));
        let rotulo = state
            .text_system
            .create_label_buffer("SEGUNDO PLANO", design::type_scale::XS, fw - 40.0);
        state.text_system.draw_buffer(
            canvas,
            &rotulo,
            fx + 14.0,
            y,
            Color::from_hex(palette.text),
        );
        let chev = state
            .text_system
            .create_line_buffer("⌄", design::type_scale::SM, 16.0);
        state.text_system.draw_buffer(
            canvas,
            &chev,
            fx + fw - 12.0,
            y,
            Color::from_hex(palette.text_muted),
        );
        state.add_click_target(
            Bounds::new(fondo.x, y - 14.0, fondo.width, 24.0),
            ClickTargetAction::ToggleBackgroundPanel,
        );
        y += design::space::LG;

        let index_label = state.project_index_label();
        let index_buf = state.text_system.create_code_buffer(&index_label, design::type_scale::XS, fw);
        let index_height: f32 = index_buf.layout_runs().map(|run| run.line_height).sum();
        state.text_system.draw_buffer(canvas, &index_buf, fx, y, Color::from_hex(palette.text_muted));
        y += (index_height + design::space::SM).max(design::space::LG);

        // Estadísticas y barra de presupuesto, de la instantánea real.
        let k = |n: u32| -> String {
            if n >= 1000 {
                format!("{:.1}k", n as f32 / 1000.0)
            } else {
                n.to_string()
            }
        };
        let (stats, fraccion) = match state.background_snapshot {
            Some(sn) => (
                format!(
                    "herramientas {}   {} / {}   par {}/{}",
                    state.last_tool_runs.len(),
                    k(sn.tokens_used),
                    k(sn.token_budget),
                    sn.parallel_current,
                    sn.parallel_cap
                ),
                if sn.token_budget > 0 {
                    (sn.tokens_used as f32 / sn.token_budget as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ),
            None if state.loading => ("chat · pensando".to_string(), 0.0),
            None => ("chat · sin llamadas".to_string(), 0.0),
        };
        let stats_buf = state
            .text_system
            .create_code_buffer(&stats, design::type_scale::XS, fw);
        state.text_system.draw_buffer(
            canvas,
            &stats_buf,
            fx,
            y,
            Color::from_hex(palette.text_muted),
        );
        y += design::space::MD;
        let barra = Bounds::new(fx, y, fw, 3.0);
        canvas.fill_rounded_rect(barra, 1.5, Color::from_hex(palette.border));
        if fraccion > 0.0 {
            canvas.fill_rounded_rect(
                Bounds::new(fx, y, (fw * fraccion).max(3.0), 3.0),
                1.5,
                Color::from_hex(palette.accent),
            );
        }
        y += design::space::XL;

        if state.loading {
            // En vivo: lo que hace ahora mismo, la última línea encendida.
            let rotulo = state.text_system.create_label_buffer("AHORA", design::type_scale::XS, fw);
            state.text_system.draw_buffer(canvas, &rotulo, fx, y, Color::from_hex(palette.text_muted));
            y += design::space::LG;
            let lineas = state.chat_activity_lines();
            let n = lineas.len();
            for (i, linea) in lineas.iter().enumerate().skip(n.saturating_sub(9)) {
                if y > tope {
                    break;
                }
                let texto = truncate_chars(linea, 70);
                let (_, alto) = state.text_system.measure_code(&texto, design::type_scale::XS, fw);
                let buf = state.text_system.create_code_buffer(&texto, design::type_scale::XS, fw);
                let color = if i + 1 == n {
                    Color::from_hex(palette.accent)
                } else if linea.starts_with('✗') {
                    Color::from_hex(palette.error)
                } else {
                    Color::from_hex(palette.text_muted)
                };
                state.text_system.draw_buffer(canvas, &buf, fx, y, color);
                y += paso.max(alto + 2.0);
            }
        } else if state.last_tool_runs.is_empty() {
            // Sin manos usadas no se enseña nada: el vacío ya lo dice.
        } else {
            // Flujo: una línea por herramienta, en el orden en que ocurrieron.
            for run in state.last_tool_runs.iter().take(8) {
                if y > tope {
                    break;
                }
                let linea = match run.name.as_str() {
                    "read_file" => format!("lee {}", run.arg),
                    "search_text" => format!("busca \"{}\" → {}", run.arg, run.summary),
                    "list_files" => format!("lista {}", if run.arg.is_empty() { "." } else { &run.arg }),
                    otro => format!("{otro} {}", run.arg),
                };
                let linea = if run.is_error { format!("✗ {linea}") } else { linea };
                let texto = truncate_chars(&linea, 60);
                // Una ruta larga envuelve en dos líneas: la siguiente entrada
                // empieza donde acaba esta, no a un paso fijo.
                let (_, alto) = state.text_system.measure_code(&texto, design::type_scale::XS, fw);
                let l_buf = state.text_system.create_code_buffer(&texto, design::type_scale::XS, fw);
                state.text_system.draw_buffer(
                    canvas,
                    &l_buf,
                    fx,
                    y,
                    if run.is_error {
                        Color::from_hex(palette.error)
                    } else {
                        Color::from_hex(palette.text_muted)
                    },
                );
                y += paso.max(alto + 2.0);
            }
            y += design::space::LG;

            // LEYENDO AHORA: los archivos que leyó, sin repetir.
            let mut leidos: Vec<&str> = Vec::new();
            for run in state.last_tool_runs.iter() {
                if run.name == "read_file" && !run.is_error && !leidos.contains(&run.arg.as_str()) {
                    leidos.push(run.arg.as_str());
                }
            }
            if !leidos.is_empty() && y < tope {
                let r = state
                    .text_system
                    .create_label_buffer("LEYENDO AHORA", design::type_scale::XS, fw);
                state.text_system.draw_buffer(
                    canvas,
                    &r,
                    fx,
                    y,
                    Color::from_hex(palette.text_muted),
                );
                y += design::space::LG;
                for ruta in leidos.iter().take(6) {
                    if y > tope {
                        break;
                    }
                    let ruta_buf = state.text_system.create_code_buffer(
                        &truncate_chars(ruta, 34),
                        design::type_scale::XS,
                        fw - 44.0,
                    );
                    state.text_system.draw_buffer(
                        canvas,
                        &ruta_buf,
                        fx,
                        y,
                        Color::from_hex(palette.text),
                    );
                    let tag = state
                        .text_system
                        .create_code_buffer("leído", design::type_scale::XS, 40.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &tag,
                        fx + fw - 36.0,
                        y,
                        Color::from_hex(palette.accent_alt),
                    );
                    y += paso;
                }
                y += design::space::LG;
            }

            // HERRAMIENTAS: tarjetas con la duración medida; la última, sobre
            // superficie, como la destacada de la maqueta.
            if y < tope {
                let r = state
                    .text_system
                    .create_label_buffer("HERRAMIENTAS", design::type_scale::XS, fw);
                state.text_system.draw_buffer(
                    canvas,
                    &r,
                    fx,
                    y,
                    Color::from_hex(palette.text_muted),
                );
                y += design::space::LG;
                let n = state.last_tool_runs.len();
                let tarjeta_h = paso * 2.0 + 10.0;
                for (i, run) in state.last_tool_runs.iter().enumerate().rev().take(5) {
                    if y + tarjeta_h > tope {
                        break;
                    }
                    let tarjeta = Bounds::new(fondo.x + 4.0, y - 12.0, fondo.width - 8.0, tarjeta_h);
                    if i + 1 == n {
                        canvas.fill_rounded_rect(
                            tarjeta,
                            design::radius::MD,
                            Color::from_hex(palette.surface),
                        );
                    }
                    let nombre = state
                        .text_system
                        .create_code_buffer(&run.name, design::type_scale::XS, fw - 52.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &nombre,
                        fx,
                        y,
                        Color::from_hex(palette.text),
                    );
                    let dur = format!("{:.1}s", run.took.as_secs_f32());
                    let dur_buf = state
                        .text_system
                        .create_code_buffer(&dur, design::type_scale::XS, 48.0);
                    state.text_system.draw_buffer(
                        canvas,
                        &dur_buf,
                        fx + fw - 40.0,
                        y,
                        if run.is_error {
                            Color::from_hex(palette.error)
                        } else {
                            Color::from_hex(palette.accent_alt)
                        },
                    );
                    let sub_txt = if run.arg.is_empty() {
                        run.summary.clone()
                    } else {
                        format!("{} · {}", truncate_chars(&run.arg, 26), run.summary)
                    };
                    let sub_buf = state.text_system.create_code_buffer(
                        &truncate_chars(&sub_txt, 46),
                        design::type_scale::XS,
                        fw,
                    );
                    state.text_system.draw_buffer(
                        canvas,
                        &sub_buf,
                        fx,
                        y + paso - 2.0,
                        Color::from_hex(palette.text_muted),
                    );
                    y += tarjeta_h;
                }
            }
        }
    }

    // === Pantalla de inicio ===
    //
    // Sin proyecto abierto no hay banco de trabajo que enseñar: la bienvenida
    // ocupa la ventana entera, como el `pantalla: "inicio"` de la maqueta.
    // Antes se dibujaba dentro de la columna del editor —que es donde van los
    // archivos al abrirlos—, y ahí no pintaba nada.
    //
    // Va la última a propósito: `action_at` recorre los objetivos de clic al
    // revés, así que dibujarla al final es lo que hace que «Abrir carpeta…» y
    // los recientes ganen a lo que haya debajo.
    if state.welcome_visible() {
        let pantalla = Bounds::new(0.0, header_height, bounds.width, content_height);
        canvas.fill_rect(pantalla, Color::from_hex(palette.background));
        render_welcome(canvas, state, pantalla, &palette);
    }

    // === Overlay (Quick Open / Command Palette / Symbols) ===
    if let Some(mode) = state.overlay_mode {
        canvas.fill_rect(bounds, overlay_scrim_color(palette));

        if mode == OverlayMode::Agentes {
            render_agents_overlay(canvas, state, &palette, bounds, header_height);
        } else if mode == OverlayMode::Manual {
            render_manual_overlay(canvas, state, &palette, bounds, header_height);
        } else {
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
            OverlayMode::Agentes => "AGENTES",
            OverlayMode::Manual => "MANUAL",
            OverlayMode::Prompt => "NOMBRE",
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
            OverlayMode::Agentes | OverlayMode::Manual => "",
            OverlayMode::Prompt => "Intro confirma | Esc cancela",
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
            OverlayMode::Agentes | OverlayMode::Manual => "",
            OverlayMode::Prompt => "› ",
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
                    OverlayMode::Agentes | OverlayMode::Manual => String::new(),
                    OverlayMode::Prompt => item.title.clone(),
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
    // El menú contextual del explorador va encima de todo lo demás.
    render_context_menu(canvas, state, &palette);
}

/// Menú contextual del explorador: un panel pequeño junto al cursor con el
/// nombre de la ruta arriba y sus operaciones; «—» separa grupos.
fn render_context_menu(canvas: &mut Canvas, state: &mut AppState, palette: &ThemePalette) {
    let Some(menu) = state.context_menu.clone() else {
        return;
    };
    let fila_h = 24.0;
    let sep_h = 7.0;
    let ancho = 250.0;
    let alto: f32 = 30.0
        + menu.items.iter().map(|(t, _)| if t == "—" { sep_h } else { fila_h }).sum::<f32>()
        + 8.0;
    let (lienzo_w, lienzo_h) = (canvas.width() as f32, canvas.height() as f32);
    let x0 = menu.x.min(lienzo_w - ancho - 8.0).max(8.0);
    let y0 = menu.y.min(lienzo_h - alto - 8.0).max(8.0);
    let panel = Bounds::new(x0, y0, ancho, alto);
    canvas.drop_shadow(panel, design::radius::MD, 3.0, 10.0, Color::from_hex(0x2D2B2B).with_alpha(50));
    canvas.fill_rounded_rect(panel, design::radius::MD, Color::from_hex(palette.background));
    canvas.stroke_rounded_rect(panel, design::radius::MD, Color::from_hex(palette.border).with_alpha(220), 1.0);
    let titulo = state.text_system.create_label_buffer(&truncate_chars(&menu.title, 34), design::type_scale::XS, ancho - 24.0);
    state.text_system.draw_buffer(canvas, &titulo, x0 + 12.0, y0 + 19.0, Color::from_hex(palette.text_muted));
    let mut y = y0 + 30.0;
    for (texto, accion) in menu.items {
        if texto == "—" {
            canvas.draw_line(x0 + 10.0, y + 3.0, x0 + ancho - 10.0, y + 3.0, Color::from_hex(palette.border).with_alpha(160), 1.0);
            y += sep_h;
            continue;
        }
        let fila = Bounds::new(x0 + 6.0, y, ancho - 12.0, fila_h);
        let peligro = texto.starts_with("Eliminar") || texto.starts_with("Sí, eliminar");
        let buf = state.text_system.create_line_buffer(&truncate_chars(&texto, 36), design::type_scale::SM, ancho - 24.0);
        state.text_system.draw_buffer(
            canvas,
            &buf,
            x0 + 12.0,
            y + 16.0,
            Color::from_hex(if peligro { palette.error } else { palette.text }),
        );
        state.add_click_target(fila, accion);
        y += fila_h;
    }
}

/// Menú «+» de la barra del chat y lista de modelos: un panel pequeño y
/// redondeado sobre la bandeja, con secciones. Cada entrada hace algo real.
fn render_chat_popover(
    canvas: &mut Canvas,
    state: &mut AppState,
    palette: &ThemePalette,
    input_bounds: Bounds,
    chat_bounds: Bounds,
) {
    let filas: Vec<(String, Option<ClickTargetAction>)> = match state.chat_popover {
        ChatPopover::Models => {
            let actual = state.selected_ai_model().to_string();
            let mut v = vec![("MODELO".to_string(), None)];
            for m in state.ai_model_options().iter().map(|m| m.to_string()).collect::<Vec<_>>() {
                let marca = if m == actual { "● " } else { "   " };
                v.push((format!("{marca}{m}"), Some(ClickTargetAction::ChatModelPick(m))));
            }
            v
        }
        _ => vec![
            ("CONTEXTO".to_string(), None),
            ("Adjuntar archivo…".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::AttachFile))),
            ("Mencionar archivo del proyecto…".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::MentionFile))),
            ("Añadir la selección del editor".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::AddSelection))),
            ("Vaciar conversación".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::ClearConversation))),
            ("Rebobinar la última pregunta".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::Rewind))),
            ("MODELO".to_string(), None),
            (format!("Cambiar modelo… ({})", state.selected_ai_model()), Some(ClickTargetAction::ChatMenu(ChatMenuItem::SwitchModel))),
            (format!("Manos (leer, buscar, listar): {}", if state.chat_tools_enabled { "sí" } else { "no" }), Some(ClickTargetAction::ChatMenu(ChatMenuItem::ToggleTools))),
            (format!("Respuesta: {}", state.response_length.label()), Some(ClickTargetAction::ChatMenu(ChatMenuItem::CycleLength))),
            ("MÁS".to_string(), None),
            ("Agentes…".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::Agents))),
            ("Manual (F1)".to_string(), Some(ClickTargetAction::ChatMenu(ChatMenuItem::Manual))),
        ],
    };
    let fila_h = 24.0;
    let cab_h = 20.0;
    let alto: f32 = filas.iter().map(|(_, a)| if a.is_some() { fila_h } else { cab_h }).sum::<f32>() + 16.0;
    let ancho = (input_bounds.width - 16.0).clamp(220.0, 320.0);
    let x0 = input_bounds.x + 8.0;
    let y0 = (input_bounds.y - 8.0 - alto).max(chat_bounds.y + 8.0);
    let panel = Bounds::new(x0, y0, ancho, alto);
    canvas.drop_shadow(panel, design::radius::MD, 3.0, 10.0, Color::from_hex(0x2D2B2B).with_alpha(41));
    canvas.fill_rounded_rect(panel, design::radius::MD, Color::from_hex(palette.background));
    canvas.stroke_rounded_rect(panel, design::radius::MD, Color::from_hex(palette.border).with_alpha(200), 1.0);
    let mut y = y0 + 8.0;
    for (texto, accion) in filas {
        match accion {
            None => {
                let buf = state.text_system.create_label_buffer(&texto, design::type_scale::XS, ancho - 24.0);
                state.text_system.draw_buffer(canvas, &buf, x0 + 12.0, y + 14.0, Color::from_hex(palette.text_muted));
                y += cab_h;
            }
            Some(accion) => {
                let fila = Bounds::new(x0 + 6.0, y, ancho - 12.0, fila_h);
                let buf = state.text_system.create_line_buffer(&truncate_chars(&texto, 44), design::type_scale::SM, ancho - 24.0);
                state.text_system.draw_buffer(canvas, &buf, x0 + 12.0, y + 16.0, Color::from_hex(palette.text));
                state.add_click_target(fila, accion);
                y += fila_h;
            }
        }
    }
}

/// Paleta Agentes: centrada y con bordes redondeados como Quick Open. Arriba,
/// el proveedor en uso y la salud del cerebro; luego tarjetas en dos columnas
/// —suscripciones, conexiones directas, Ollama y el worker local— con su
/// estado real en este equipo y sus acciones. Configurar una conexión pide los
/// campos de uno en uno en la propia paleta; el worker ofrece sus `.gguf`.
fn render_agents_overlay(
    canvas: &mut Canvas,
    state: &mut AppState,
    palette: &ThemePalette,
    bounds: Bounds,
    header_height: f32,
) {
    let ancho = (bounds.width * 0.6).clamp(560.0, 760.0);
    let x0 = bounds.x + (bounds.width - ancho) * 0.5;
    let y0 = header_height + 36.0;
    let tarjeta_h = 96.0;
    let columna = (ancho - 36.0) * 0.5;
    let probe = state.provider_probe.clone();
    let en_uso = state.ai_provider().replace('-', "_");

    // Altura según lo que se enseña: rejilla, campo o lista del worker.
    let cuerpo_h = if state.agent_field.is_some() {
        120.0
    } else if state.agent_worker_pick {
        let locales = probe.as_ref().map_or(0, |p| p.worker_models.len().max(1));
        let pendientes = probe.as_ref().map_or(0, |p| p.worker_catalog.iter().filter(|c| !c.present).count());
        124.0 + locales as f32 * 26.0 + if pendientes > 0 { 30.0 + pendientes as f32 * 26.0 } else { 0.0 }
    } else {
        3.0 * (tarjeta_h + 10.0)
    };
    let aviso_h = state.provider_notice.as_ref().map_or(0.0, |_| 30.0);
    let alto = 76.0 + cuerpo_h + aviso_h + 44.0;
    let panel = Bounds::new(x0, y0, ancho, alto);
    canvas.fill_rounded_rect(panel, design::radius::LG, Color::from_hex(palette.background));
    canvas.stroke_rounded_rect(panel, design::radius::LG, Color::from_hex(palette.border).with_alpha(200), 1.0);

    // Cabecera: título, proveedor en uso y cierre.
    let titulo = state.text_system.create_heading_buffer("Agentes", 18.0, 200.0);
    state.text_system.draw_buffer(canvas, &titulo, x0 + 20.0, y0 + 30.0, Color::from_hex(palette.text));
    // Con un endpoint compatible el modelo es el configurado (o ninguno);
    // el selector del chat solo manda con Claude.
    let modelo = if en_uso == "openai_compatible" {
        probe.as_ref().and_then(|p| p.compatible_model.clone()).map_or("sin modelo".to_string(), |m| format!("modelo {m}"))
    } else {
        format!("modelo {}", state.selected_ai_model())
    };
    let estado = format!(
        "en uso: {} · {}   ·   cerebro: {}",
        if en_uso.is_empty() { "sin proveedor" } else { en_uso.as_str() },
        modelo,
        state.quiron_connection_health_label()
    );
    let estado_buf = state.text_system.create_code_buffer(&estado, design::type_scale::XS, ancho - 40.0);
    state.text_system.draw_buffer(canvas, &estado_buf, x0 + 20.0, y0 + 52.0, Color::from_hex(palette.text_muted));
    let cerrar = Bounds::new(x0 + ancho - 40.0, y0 + 14.0, 24.0, 24.0);
    let cerrar_buf = state.text_system.create_line_buffer("×", design::type_scale::MD, 20.0);
    state.text_system.draw_buffer(canvas, &cerrar_buf, cerrar.x + 7.0, cerrar.y + 17.0, Color::from_hex(palette.text_muted));
    state.add_click_target(cerrar, ClickTargetAction::AgentClose);

    let mut y = y0 + 76.0;

    if let Some(campo) = state.agent_field.clone() {
        // Entrada de un campo, de uno en uno, con el texto en overlay_query.
        let etiquetas = campo.target.labels();
        let etiqueta = format!(
            "{} · paso {}/{}: {}",
            campo.target.kind().label(),
            campo.step + 1,
            etiquetas.len(),
            etiquetas[campo.step]
        );
        let buf = state.text_system.create_line_buffer(&etiqueta, design::type_scale::SM, ancho - 40.0);
        state.text_system.draw_buffer(canvas, &buf, x0 + 20.0, y + 16.0, Color::from_hex(palette.text));
        let caja = Bounds::new(x0 + 20.0, y + 30.0, ancho - 40.0, 34.0);
        canvas.fill_rounded_rect(caja, design::radius::MD, Color::from_hex(palette.surface));
        canvas.stroke_rounded_rect(caja, design::radius::MD, Color::from_hex(palette.accent).with_alpha(160), 1.0);
        let es_clave = campo.target == AgentTarget::Compatible && campo.step == 2;
        let texto = if es_clave {
            "•".repeat(state.overlay_query.chars().count())
        } else {
            state.overlay_query.clone()
        };
        let mostrado = format!("{}|", truncate_chars(&texto, 80));
        let buf = state.text_system.create_line_buffer(&mostrado, design::type_scale::MD, caja.width - 20.0);
        state.text_system.draw_buffer(canvas, &buf, caja.x + 10.0, caja.y + 22.0, Color::from_hex(palette.text));
        let pista = state.text_system.create_code_buffer(
            "Intro para seguir · Esc para cancelar",
            design::type_scale::XS,
            ancho - 40.0,
        );
        state.text_system.draw_buffer(canvas, &pista, x0 + 20.0, y + 84.0, Color::from_hex(palette.text_muted));
        y += cuerpo_h;
    } else if state.agent_worker_pick {
        // La guía de hardware, primero: qué hay en este equipo y qué le va.
        let hardware = probe.as_ref().map(|p| p.hardware.clone()).unwrap_or_default();
        let equipo = format!("Este equipo: {}", hardware.resumen());
        let buf = state.text_system.create_line_buffer(&truncate_chars(&equipo, 110), design::type_scale::SM, ancho - 40.0);
        state.text_system.draw_buffer(canvas, &buf, x0 + 20.0, y + 16.0, Color::from_hex(palette.text));
        let consejo = format!("Te va: {}", hardware.recomendacion());
        let buf = state.text_system.create_line_buffer(&truncate_chars(&consejo, 110), design::type_scale::SM, ancho - 40.0);
        state.text_system.draw_buffer(canvas, &buf, x0 + 20.0, y + 34.0, Color::from_hex(palette.accent));
        let buf = state.text_system.create_line_buffer(
            "Mejor worker, mejores fichas: cada salto de tamaño entiende mejor el código y tarda más por archivo. El vectorizador no razona: lee y escribe fichas.",
            design::type_scale::XS,
            ancho - 40.0,
        );
        state.text_system.draw_buffer(canvas, &buf, x0 + 20.0, y + 52.0, Color::from_hex(palette.text_muted));
        let rotulo = state.text_system.create_line_buffer(
            "En la carpeta del worker (pulsa uno para usarlo):",
            design::type_scale::SM,
            ancho - 40.0,
        );
        state.text_system.draw_buffer(canvas, &rotulo, x0 + 20.0, y + 80.0, Color::from_hex(palette.text));
        let mut fila_y = y + 94.0;
        let modelos = probe.as_ref().map(|p| p.worker_models.clone()).unwrap_or_default();
        let actual = probe.as_ref().map(|p| p.worker_current.clone()).unwrap_or_default();
        let catalogo = probe.as_ref().map(|p| p.worker_catalog.clone()).unwrap_or_default();
        if modelos.is_empty() {
            let vacio = state.text_system.create_line_buffer(
                "no hay archivos .gguf en la carpeta del worker",
                design::type_scale::XS,
                ancho - 40.0,
            );
            state.text_system.draw_buffer(canvas, &vacio, x0 + 30.0, fila_y + 14.0, Color::from_hex(palette.text_muted));
            fila_y += 26.0;
        }
        for nombre in modelos {
            let fila = Bounds::new(x0 + 20.0, fila_y, ancho - 40.0, 24.0);
            if nombre == actual {
                canvas.fill_rounded_rect(fila, 4.0, Color::from_hex(palette.selection).with_alpha(120));
            }
            let nota = catalogo.iter().find(|c| c.file == nombre).map(|c| c.nota.clone()).unwrap_or_default();
            let texto = if nota.is_empty() { nombre.clone() } else { format!("{nombre} · {nota}") };
            let buf = state.text_system.create_line_buffer(&truncate_chars(&texto, 90), design::type_scale::SM, ancho - 60.0);
            state.text_system.draw_buffer(canvas, &buf, fila.x + 10.0, fila_y + 16.0, Color::from_hex(palette.text));
            state.add_click_target(fila, ClickTargetAction::AgentWorkerModel(nombre));
            fila_y += 26.0;
        }
        // Catálogo: lo que no está, se puede descargar con su suma verificada.
        let pendientes: Vec<_> = catalogo.iter().filter(|c| !c.present).cloned().collect();
        if !pendientes.is_empty() {
            let rotulo = state.text_system.create_line_buffer(
                "Descargar del catálogo (Qwen2.5-Coder y Qwen3, SHA-256 verificado; pulsa uno):",
                design::type_scale::SM,
                ancho - 40.0,
            );
            state.text_system.draw_buffer(canvas, &rotulo, x0 + 20.0, fila_y + 18.0, Color::from_hex(palette.text));
            fila_y += 30.0;
            for entrada in pendientes {
                let fila = Bounds::new(x0 + 20.0, fila_y, ancho - 40.0, 24.0);
                let ajuste = hardware.cabe(&entrada);
                let texto = format!("⤓ {} · {:.1} GB · {}{}", entrada.file, entrada.size_gb, entrada.nota, ajuste.etiqueta());
                let buf = state.text_system.create_line_buffer(&truncate_chars(&texto, 110), design::type_scale::SM, ancho - 60.0);
                let color = match ajuste {
                    llore_ui::app::Ajuste::Bien | llore_ui::app::Ajuste::Justo => Color::from_hex(palette.accent),
                    _ => Color::from_hex(palette.text_muted),
                };
                state.text_system.draw_buffer(canvas, &buf, fila.x + 10.0, fila_y + 16.0, color);
                state.add_click_target(fila, ClickTargetAction::AgentWorkerDownload(entrada.name.clone()));
                fila_y += 26.0;
            }
        }
        let pista = state.text_system.create_code_buffer(
            "Esc para volver · «Añadir .gguf…» copia un modelo que ya tengas",
            design::type_scale::XS,
            ancho - 40.0,
        );
        state.text_system.draw_buffer(canvas, &pista, x0 + 20.0, fila_y + 14.0, Color::from_hex(palette.text_muted));
        y += cuerpo_h;
    } else {
        // Rejilla de tarjetas.
        let tarjetas: [(ProviderKind, Option<AgentTarget>); 5] = [
            (ProviderKind::ClaudeCli, None),
            (ProviderKind::CodexDirect, None),
            (ProviderKind::OpenAiCompatible, Some(AgentTarget::Compatible)),
            (ProviderKind::LocalServer, Some(AgentTarget::LocalServer)),
            (ProviderKind::Ollama, Some(AgentTarget::Ollama)),
        ];
        for (i, (kind, target)) in tarjetas.iter().enumerate() {
            let cx = x0 + 20.0 + (i % 2) as f32 * (columna + 16.0);
            let cy = y + (i / 2) as f32 * (tarjeta_h + 10.0);
            // Un endpoint sin modelo (la instalación limpia trae uno de
            // relleno) no cuenta como configurado ni como «en uso».
            let guardado = probe
                .as_ref()
                .filter(|p| p.compatible_model.is_some())
                .and_then(|p| p.compatible_endpoint.as_deref());
            let activo = en_uso == kind.backend()
                && match kind {
                    ProviderKind::ClaudeCli | ProviderKind::CodexDirect => true,
                    _ => guardado.map_or(false, |e| ProviderKind::from_compatible_endpoint(e) == *kind),
                };
            let (estado, ok) = estado_de_proveedor(*kind, probe.as_ref());
            let usar = (if activo { "En uso" } else { "Usar" }, ClickTargetAction::ProviderUse(*kind));
            let falta_programa = match (kind, probe.as_ref()) {
                (ProviderKind::ClaudeCli, Some(p)) => p.claude_cli.is_none(),
                (ProviderKind::CodexDirect, Some(p)) => p.codex_cli.is_none(),
                (ProviderKind::Ollama, Some(p)) => p.ollama_models.is_none(),
                _ => false,
            };
            let acciones: Vec<(&str, ClickTargetAction)> = match (kind, target) {
                (ProviderKind::ClaudeCli | ProviderKind::CodexDirect, _) if falta_programa => {
                    vec![("Instalar", ClickTargetAction::ProviderInstall(*kind))]
                }
                (ProviderKind::ClaudeCli | ProviderKind::CodexDirect, _) => {
                    vec![("Iniciar sesión", ClickTargetAction::ProviderLogin(*kind)), usar]
                }
                (ProviderKind::Ollama, Some(t)) if falta_programa => vec![
                    ("Instalar", ClickTargetAction::ProviderInstall(*kind)),
                    ("Configurar…", ClickTargetAction::AgentConfigure(*t)),
                ],
                (_, Some(t)) => vec![("Configurar…", ClickTargetAction::AgentConfigure(*t)), usar],
                _ => Vec::new(),
            };
            dibujar_tarjeta_agente(canvas, state, palette, Bounds::new(cx, cy, columna, tarjeta_h), kind.label(), kind.detail(), &estado, ok, activo, acciones);
        }
        // Worker local, sexta tarjeta.
        let cx = x0 + 20.0 + columna + 16.0;
        let cy = y + 2.0 * (tarjeta_h + 10.0);
        let (estado, ok) = match &probe {
            None => ("comprobando…".to_string(), None),
            Some(p) => {
                let marcha = match p.worker_running { Some(true) => "en marcha", Some(false) => "parado", None => "estado desconocido" };
                (format!("{} · {marcha}", truncate_chars(&p.worker_current, 34)), p.worker_running)
            }
        };
        dibujar_tarjeta_agente(
            canvas, state, palette, Bounds::new(cx, cy, columna, tarjeta_h),
            "Vectorizador (worker)", "lee el proyecto, escribe qué hace cada función y vectoriza", &estado, ok, false,
            vec![
                ("Modelos", ClickTargetAction::AgentWorkerPick),
                ("Añadir .gguf…", ClickTargetAction::AgentWorkerAddFile),
            ],
        );
        y += cuerpo_h;
    }

    if let Some(aviso) = state.provider_notice.clone() {
        let buf = state.text_system.create_line_buffer(&truncate_chars(&aviso, 110), design::type_scale::XS, ancho - 40.0);
        state.text_system.draw_buffer(canvas, &buf, x0 + 20.0, y + 14.0, Color::from_hex(palette.accent));
        y += aviso_h;
    }
    let boton = Bounds::new(x0 + 20.0, y + 8.0, 110.0, 26.0);
    canvas.fill_rounded_rect(boton, design::radius::MD, Color::from_hex(palette.surface));
    canvas.stroke_rounded_rect(boton, design::radius::MD, Color::from_hex(palette.border).with_alpha(200), 1.0);
    let buf = state.text_system.create_line_buffer("Comprobar", design::type_scale::SM, 100.0);
    state.text_system.draw_buffer(canvas, &buf, boton.x + 14.0, boton.y + 18.0, Color::from_hex(palette.text_muted));
    state.add_click_target(boton, ClickTargetAction::ProviderRefresh);
    let manual = Bounds::new(boton.x + boton.width + 10.0, boton.y, 100.0, 26.0);
    let buf = state.text_system.create_line_buffer("Manual (F1)", design::type_scale::SM, 100.0);
    state.text_system.draw_buffer(canvas, &buf, manual.x + 10.0, manual.y + 18.0, Color::from_hex(palette.text_muted));
    state.add_click_target(manual, ClickTargetAction::TopMenuExecute(CommandPaletteAction::OpenManual));
    let pista = state.text_system.create_code_buffer("Esc cierra", design::type_scale::XS, 120.0);
    state.text_system.draw_buffer(canvas, &pista, x0 + ancho - 90.0, boton.y + 18.0, Color::from_hex(palette.text_muted));
}

/// Manual dentro de la aplicación: panel centrado con el índice de secciones
/// a la izquierda y el texto de la sección a la derecha, con desplazamiento
/// (rueda, flechas, AvPág). El texto sale de docs/MANUAL.md, embebido.
fn render_manual_overlay(
    canvas: &mut Canvas,
    state: &mut AppState,
    palette: &ThemePalette,
    bounds: Bounds,
    header_height: f32,
) {
    let secciones = manual_sections();
    if secciones.is_empty() {
        return;
    }
    let ancho = (bounds.width * 0.74).clamp(600.0, 940.0);
    let x0 = bounds.x + (bounds.width - ancho) * 0.5;
    let y0 = header_height + 24.0;
    let alto = (bounds.y + bounds.height - 24.0 - y0).max(300.0);
    let panel = Bounds::new(x0, y0, ancho, alto);
    canvas.fill_rounded_rect(panel, design::radius::LG, Color::from_hex(palette.background));
    canvas.stroke_rounded_rect(panel, design::radius::LG, Color::from_hex(palette.border).with_alpha(200), 1.0);

    let seccion = state.manual_section.min(secciones.len() - 1);
    let titulo = state.text_system.create_heading_buffer("Manual", 18.0, 200.0);
    state.text_system.draw_buffer(canvas, &titulo, x0 + 20.0, y0 + 30.0, Color::from_hex(palette.text));
    let cerrar = Bounds::new(x0 + ancho - 40.0, y0 + 14.0, 24.0, 24.0);
    let cerrar_buf = state.text_system.create_line_buffer("×", design::type_scale::MD, 20.0);
    state.text_system.draw_buffer(canvas, &cerrar_buf, cerrar.x + 7.0, cerrar.y + 17.0, Color::from_hex(palette.text_muted));
    state.add_click_target(cerrar, ClickTargetAction::AgentClose);

    // Índice de secciones.
    let indice_w = 200.0;
    let mut iy = y0 + 56.0;
    for (i, (nombre, _)) in secciones.iter().enumerate() {
        let fila = Bounds::new(x0 + 12.0, iy, indice_w - 12.0, 26.0);
        if i == seccion {
            canvas.fill_rounded_rect(fila, 6.0, Color::from_hex(palette.selection).with_alpha(120));
        }
        let etiqueta = format!("{} {}", i + 1, if i == 0 { "Qué es Quirón" } else { nombre.as_str() });
        let buf = state.text_system.create_line_buffer(&truncate_chars(&etiqueta, 28), design::type_scale::SM, fila.width - 16.0);
        state.text_system.draw_buffer(
            canvas,
            &buf,
            fila.x + 8.0,
            iy + 17.0,
            Color::from_hex(if i == seccion { palette.text } else { palette.text_muted }),
        );
        state.add_click_target(fila, ClickTargetAction::ManualSection(i));
        iy += 28.0;
    }

    // Texto de la sección, con recorte y desplazamiento.
    let tx = x0 + indice_w + 24.0;
    let tw = ancho - indice_w - 48.0;
    let top = y0 + 56.0;
    let bottom = y0 + alto - 36.0;
    let (nombre, cuerpo) = &secciones[seccion];
    let mut y = top - state.manual_scroll;
    let pintar = |state: &mut AppState, canvas: &mut Canvas, texto: &str, size: f32, x: f32, y: f32, color: Color, heading: bool| -> f32 {
        let (_, h) = state.text_system.measure(texto, size, tw - (x - tx));
        let buf = if heading {
            state.text_system.create_heading_buffer(texto, size, tw - (x - tx))
        } else {
            state.text_system.create_buffer(texto, size, tw - (x - tx))
        };
        if y + h >= top && y <= bottom {
            state.text_system.draw_buffer_within(canvas, &buf, x, y + size, color, top, bottom);
        }
        h
    };
    let h = pintar(state, canvas, nombre, 15.0, tx, y, Color::from_hex(palette.text), true);
    y += h + 10.0;
    for bloque in manual_blocks(cuerpo) {
        match bloque {
            ManualBlock::Heading(t) => {
                y += 6.0;
                let h = pintar(state, canvas, &t, design::type_scale::MD, tx, y, Color::from_hex(palette.text), true);
                y += h + 6.0;
            }
            ManualBlock::Paragraph(t) => {
                let h = pintar(state, canvas, &t, design::type_scale::SM, tx, y, Color::from_hex(palette.text), false);
                y += h + 10.0;
            }
            ManualBlock::Bullet(t) => {
                pintar(state, canvas, "•", design::type_scale::SM, tx + 4.0, y, Color::from_hex(palette.accent), false);
                let h = pintar(state, canvas, &t, design::type_scale::SM, tx + 18.0, y, Color::from_hex(palette.text), false);
                y += h + 6.0;
            }
        }
    }
    let total = y + state.manual_scroll - top;
    state.manual_scroll_max = (total - (bottom - top)).max(0.0);
    // Si la sección cambió a una más corta, el desplazamiento se acota.
    if state.manual_scroll > state.manual_scroll_max {
        state.manual_scroll = state.manual_scroll_max;
    }
    let pista = state.text_system.create_code_buffer(
        "rueda o ↑↓ desplaza · ←→ o 1-9 sección · Esc cierra",
        design::type_scale::XS,
        ancho - 40.0,
    );
    state.text_system.draw_buffer(canvas, &pista, x0 + 20.0, y0 + alto - 14.0, Color::from_hex(palette.text_muted));
}

/// Estado de un proveedor según el sondeo: texto y si está listo.
fn estado_de_proveedor(kind: ProviderKind, probe: Option<&llore_ui::app::ProviderProbe>) -> (String, Option<bool>) {
    let Some(p) = probe else {
        return ("comprobando…".to_string(), None);
    };
    match kind {
        ProviderKind::ClaudeCli => match (&p.claude_cli, p.claude_logged_in) {
            (None, _) if !p.npm_found => ("CLI no encontrada · hace falta Node.js (npm)".to_string(), Some(false)),
            (None, _) => ("CLI no encontrada · npm install -g @anthropic-ai/claude-code".to_string(), Some(false)),
            (Some(_), Some(true)) => ("CLI encontrada · sesión de claude.ai activa".to_string(), Some(true)),
            (Some(_), Some(false)) => ("CLI encontrada · sin sesión".to_string(), Some(false)),
            (Some(_), None) => ("CLI encontrada · estado desconocido".to_string(), None),
        },
        ProviderKind::CodexDirect => match (&p.codex_cli, &p.codex_session) {
            (None, _) if !p.npm_found => ("Codex no encontrado · hace falta Node.js (npm)".to_string(), Some(false)),
            (None, _) => ("Codex no encontrado · npm install -g @openai/codex".to_string(), Some(false)),
            (Some(_), Some(s)) => (format!("Codex · {s}"), Some(!s.starts_with("sin"))),
            (Some(_), None) => ("Codex encontrado · sin sesión".to_string(), Some(false)),
        },
        ProviderKind::OpenAiCompatible | ProviderKind::LocalServer => match (
            p.compatible_endpoint.as_deref().filter(|e| ProviderKind::from_compatible_endpoint(e) == kind),
            p.compatible_model.as_deref(),
        ) {
            (Some(e), Some(m)) => (format!("{} · {}", truncate_chars(e, 30), m), Some(true)),
            _ => ("sin configurar".to_string(), Some(false)),
        },
        ProviderKind::Ollama => match &p.ollama_models {
            None => ("no responde en 127.0.0.1:11434 · instálalo o arráncalo".to_string(), Some(false)),
            Some(m) if m.is_empty() => ("en marcha · sin modelos: ollama pull qwen2.5-coder:7b".to_string(), Some(false)),
            Some(m) => (format!("en marcha · {}", truncate_chars(&m.join(", "), 40)), Some(true)),
        },
    }
}

/// Tarjeta de la paleta: nombre, detalle, estado coloreado y botones.
#[allow(clippy::too_many_arguments)]
fn dibujar_tarjeta_agente(
    canvas: &mut Canvas,
    state: &mut AppState,
    palette: &ThemePalette,
    caja: Bounds,
    nombre: &str,
    detalle: &str,
    estado: &str,
    ok: Option<bool>,
    activo: bool,
    acciones: Vec<(&str, ClickTargetAction)>,
) {
    canvas.fill_rounded_rect(caja, design::radius::MD, Color::from_hex(palette.surface));
    canvas.stroke_rounded_rect(
        caja,
        design::radius::MD,
        Color::from_hex(if activo { palette.accent } else { palette.border }).with_alpha(if activo { 200 } else { 120 }),
        1.0,
    );
    let buf = state.text_system.create_heading_buffer(nombre, design::type_scale::SM, caja.width - 24.0);
    state.text_system.draw_buffer(canvas, &buf, caja.x + 12.0, caja.y + 20.0, Color::from_hex(palette.text));
    let buf = state.text_system.create_line_buffer(detalle, design::type_scale::XS, caja.width - 24.0);
    state.text_system.draw_buffer(canvas, &buf, caja.x + 12.0, caja.y + 35.0, Color::from_hex(palette.text_muted));
    let color = match ok {
        Some(true) => Color::from_hex(palette.success),
        Some(false) => Color::from_hex(palette.warning),
        None => Color::from_hex(palette.text_muted),
    };
    let buf = state.text_system.create_line_buffer(&truncate_chars(estado, 52), design::type_scale::XS, caja.width - 24.0);
    state.text_system.draw_buffer(canvas, &buf, caja.x + 12.0, caja.y + 52.0, color);
    let mut bx = caja.x + 12.0;
    for (texto, accion) in acciones {
        let ancho = 12.0 + texto.chars().count() as f32 * 6.4 + 12.0;
        let boton = Bounds::new(bx, caja.y + 62.0, ancho, 24.0);
        canvas.fill_rounded_rect(boton, 6.0, Color::from_hex(palette.accent).with_alpha(if texto == "En uso" { 60 } else { 28 }));
        let buf = state.text_system.create_line_buffer(texto, design::type_scale::XS, ancho - 8.0);
        state.text_system.draw_buffer(canvas, &buf, bx + 12.0, caja.y + 78.0, Color::from_hex(palette.text));
        state.add_click_target(boton, accion);
        bx += ancho + 8.0;
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
/// Secciones de la lateral entre «Nuevo chat» y la búsqueda: el historial de
/// conversaciones y el repositorio de trabajo con sus recientes, como pestañas
/// plegables. Devuelve la `y` donde sigue la columna.
fn render_sidebar_sections(
    canvas: &mut Canvas,
    state: &mut AppState,
    palette: &ThemePalette,
    x: f32,
    w: f32,
    mut y: f32,
) -> f32 {
    let fila = 24.0;
    let texto_base = |y: f32, tamano: f32| y + fila * 0.5 + tamano * 0.36;
    let cabecera = |canvas: &mut Canvas,
                    state: &mut AppState,
                    y: f32,
                    texto: &str,
                    abierta: bool,
                    accion: ClickTargetAction| {
        let bounds = Bounds::new(x, y, w, fila);
        let rotulo = format!("{}  {}", if abierta { "⌄" } else { "›" }, texto);
        let buf = state
            .text_system
            .create_line_buffer(&rotulo, design::type_scale::XS, w);
        state.text_system.draw_buffer(
            canvas,
            &buf,
            x + 2.0,
            texto_base(y, design::type_scale::XS),
            Color::from_hex(palette.text_muted),
        );
        state.add_click_target(bounds, accion);
    };

    // --- Conversaciones: el historial, con el hilo activo marcado ---
    let abierta = state.sidebar_conversations_open;
    cabecera(canvas, state, y, "CONVERSACIONES", abierta, ClickTargetAction::ToggleConversations);
    y += fila;
    if abierta {
        let hilos: Vec<(usize, String)> = state
            .chat_threads
            .iter()
            .enumerate()
            .rev()
            .take(8)
            .map(|(i, hilo)| (i, hilo.title()))
            .collect();
        if hilos.is_empty() {
            // Nada que listar: no se pinta un texto de relleno.
        }
        for (i, titulo) in hilos {
            let activa = state.active_thread == Some(i);
            let bounds = Bounds::new(x, y, w, fila);
            if activa {
                canvas.fill_rounded_rect(bounds, 4.0, Color::from_hex(palette.selection).with_alpha(120));
            }
            let buf = state
                .text_system
                .create_line_buffer(&titulo, design::type_scale::SM, w - 20.0);
            state.text_system.draw_buffer(
                canvas,
                &buf,
                x + 14.0,
                texto_base(y, design::type_scale::SM),
                Color::from_hex(if activa { palette.text } else { palette.text_muted }),
            );
            state.add_click_target(bounds, ClickTargetAction::SelectThread(i));
            y += fila;
        }
    }
    y += design::space::SM;

    // --- Abrir carpeta: la acción, como «Nuevo chat» ---
    let boton_h = 34.0;
    let boton = Bounds::new(x, y, w, boton_h);
    canvas.fill_rounded_rect(boton, design::radius::MD, Color::from_hex(palette.accent));
    let icono = state.text_system.create_icon_buffer(icons::FOLDER_OPEN, 13.0);
    state.text_system.draw_buffer(
        canvas,
        &icono,
        boton.x + 14.0,
        y + boton_h * 0.5 + 5.0,
        Color::from_hex(palette.background),
    );
    let etiqueta = state.text_system.create_heading_buffer(
        "Abrir carpeta",
        design::type_scale::SM,
        w - 40.0,
    );
    state.text_system.draw_buffer(
        canvas,
        &etiqueta,
        boton.x + 36.0,
        y + boton_h * 0.5 + design::type_scale::SM * 0.36,
        Color::from_hex(palette.background),
    );
    state.add_click_target(boton, ClickTargetAction::WelcomeOpenFolder);
    y += boton_h + design::space::SM;

    // --- Repositorios: el de trabajo como cajita; pulsarla despliega los demás ---
    let actual = state.workspace_is_open().then(|| state.workspace_root.clone());
    let abierta = state.sidebar_repos_open || actual.is_none();
    cabecera(canvas, state, y, "REPOSITORIOS", abierta, ClickTargetAction::ToggleRepositories);
    y += fila;
    if let Some(raiz) = &actual {
        let chip = Bounds::new(x, y, w, 28.0);
        canvas.fill_rounded_rect(chip, design::radius::MD, Color::from_hex(palette.accent).with_alpha(38));
        canvas.stroke_rect(chip, Color::from_hex(palette.accent).with_alpha(90), 1.0);
        let icono = state.text_system.create_icon_buffer(icons::FOLDER_OPEN, 12.0);
        state.text_system.draw_buffer(canvas, &icono, chip.x + 10.0, y + 19.0, Color::from_hex(palette.accent));
        let nombre = llore_ui::recents::display_name(raiz);
        let buf = state
            .text_system
            .create_line_buffer(&nombre, design::type_scale::SM, w - 40.0);
        state.text_system.draw_buffer(canvas, &buf, chip.x + 30.0, y + 19.0, Color::from_hex(palette.text));
        state.add_click_target(chip, ClickTargetAction::ToggleRepositories);
        y += 28.0 + 4.0;
    }
    if abierta {
        let otros: Vec<_> = state
            .recent_projects
            .iter()
            .filter(|p| actual.as_ref() != Some(*p))
            .take(6)
            .cloned()
            .collect();
        if otros.is_empty() && actual.is_none() {
            // Sin repositorios recientes: vacío, sin texto de relleno.
        }
        for proyecto in otros {
            let bounds = Bounds::new(x, y, w, fila);
            let nombre = llore_ui::recents::display_name(&proyecto);
            let buf = state
                .text_system
                .create_line_buffer(&nombre, design::type_scale::SM, w - 20.0);
            state.text_system.draw_buffer(
                canvas,
                &buf,
                x + 14.0,
                texto_base(y, design::type_scale::SM),
                Color::from_hex(palette.text_muted),
            );
            state.add_click_target(bounds, ClickTargetAction::WelcomeOpenRecent(proyecto));
            y += fila;
        }
    }
    y + design::space::SM
}

fn render_welcome(
    canvas: &mut Canvas,
    state: &mut AppState,
    bounds: Bounds,
    palette: &ThemePalette,
) {
    // Como en la maqueta «Modernist», y limpia: marca, titular, subtítulo y una
    // sola acción. Abrir carpeta y los recientes viven en el explorador, que es
    // donde se buscan una vez dentro del programa.
    let margen = (bounds.width * 0.05).clamp(32.0, 80.0);
    let x = bounds.x + margen;
    let columna = (bounds.width * 0.46).clamp(260.0, 620.0);

    let chip = state.text_system.create_code_buffer(
        concat!("v", env!("CARGO_PKG_VERSION"), " · rust"),
        design::type_scale::XS,
        200.0,
    );
    state.text_system.draw_buffer(
        canvas,
        &chip,
        x,
        bounds.y + 26.0,
        Color::from_hex(palette.text_muted),
    );

    // El holograma, algo a la derecha del centro y en el tercio superior, para
    // repartir la pantalla con el titular, que va abajo a la izquierda.
    let lado = (bounds.width * 0.30)
        .clamp(240.0, 520.0)
        .min(bounds.height * 0.64);
    let cx = bounds.x + bounds.width * 0.55;
    let cy = bounds.y + (bounds.height * 0.40).max(lado * 0.5 + 24.0);
    // Sensible al ratón: el holograma gira hacia el cursor (guiñada por la
    // horizontal, cabeceo por la vertical) con inercia, y los puntos que quedan
    // bajo el cursor se encienden. Sin cursor, vuelve despacio al reposo.
    let objetivo = state.cursor_pos.map_or((0.0, 0.0), |(mx, my)| {
        (
            ((mx - cx) / bounds.width * 1.1).clamp(-0.7, 0.7),
            ((my - cy) / bounds.height * 0.8).clamp(-0.45, 0.45),
        )
    });
    let mirada = state.welcome_gaze;
    let mirada = (
        mirada.0 + (objetivo.0 - mirada.0) * 0.08,
        mirada.1 + (objetivo.1 - mirada.1) * 0.08,
    );
    state.welcome_gaze = mirada;
    draw_brain_hologram(
        canvas,
        cx,
        cy,
        lado,
        state.welcome_clock.elapsed().as_secs_f32(),
        mirada,
        state.cursor_pos,
        Color::from_hex(palette.accent),
        Color::from_hex(palette.text_muted),
    );

    // --- Columna izquierda, de abajo arriba ---
    let fondo = bounds.y + bounds.height - (bounds.height * 0.12).clamp(40.0, 96.0);
    let boton_alto = 48.0;
    let boton = Bounds::new(x, fondo - boton_alto, 148.0, boton_alto);
    canvas.fill_rounded_rect(boton, design::radius::MD, Color::from_hex(palette.accent));
    let etiqueta = state.text_system.create_line_buffer(
        "Empezar  →",
        design::type_scale::MD,
        boton.width - 20.0,
    );
    state.text_system.draw_buffer(
        canvas,
        &etiqueta,
        boton.x + 28.0,
        boton.y + 29.0,
        Color::from_hex(0xffffff),
    );
    state.add_click_target(boton, ClickTargetAction::WelcomeEnter);

    let subtitulo = "Un editor con mente propia: lee tu proyecto entero, piensa en segundo \
                     plano y te dice qué está mirando mientras lo hace.";
    let ancho_sub = columna.min(520.0);
    let (_, alto_sub) = state
        .text_system
        .measure(subtitulo, design::type_scale::MD, ancho_sub);
    let sub_top = boton.y - 36.0 - alto_sub;
    let sub_buf = state
        .text_system
        .create_buffer(subtitulo, design::type_scale::MD, ancho_sub);
    state.text_system.draw_buffer(
        canvas,
        &sub_buf,
        x,
        sub_top + design::type_scale::MD,
        Color::from_hex(palette.text_muted),
    );

    let titular = (bounds.width * 0.042).clamp(34.0, 58.0);
    let texto_titular = "Quirón, une tus archivos.";
    let (_, alto_titular) = state.text_system.measure(texto_titular, titular, columna);
    let titular_top = sub_top - 22.0 - alto_titular;
    let titular_buf = state
        .text_system
        .create_heading_buffer(texto_titular, titular, columna);
    state.text_system.draw_buffer(
        canvas,
        &titular_buf,
        x,
        titular_top + titular,
        Color::from_hex(palette.text),
    );
}

/// Holograma de la bienvenida: nube de puntos en tres dimensiones dentro de
/// una silueta de cerebro —hemisferio, cerebelo y tronco— que gira despacio
/// sobre su eje, con perspectiva y profundidad (lo cercano, más grande y más
/// opaco) y trazos finos entre vecinos. Es determinista (semilla fija): entre
/// fotogramas solo cambia el ángulo. El lienzo no pinta imágenes; esto se dibuja.
#[allow(clippy::too_many_arguments)]
fn draw_brain_hologram(
    canvas: &mut Canvas,
    cx: f32,
    cy: f32,
    lado: f32,
    t: f32,
    mirada: (f32, f32),
    cursor: Option<(f32, f32)>,
    acento: Color,
    apagado: Color,
) {
    let escala = lado / 420.0;
    let dentro = |x: f32, y: f32, z: f32| -> bool {
        let elipsoide = |ox: f32, oy: f32, oz: f32, rx: f32, ry: f32, rz: f32| {
            ((x - ox) / rx).powi(2) + ((y - oy) / ry).powi(2) + ((z - oz) / rz).powi(2) <= 1.0
        };
        // Dos hemisferios separados por la cisura y dos lóbulos de cerebelo:
        // de lado dibujan la silueta de la maqueta y de frente siguen siendo
        // un cerebro, no una bola.
        elipsoide(0.0, -20.0, -64.0, 195.0, 140.0, 86.0)
            || elipsoide(0.0, -20.0, 64.0, 195.0, 140.0, 86.0)
            || elipsoide(75.0, 108.0, -36.0, 68.0, 44.0, 40.0)
            || elipsoide(75.0, 108.0, 36.0, 68.0, 44.0, 40.0)
            || elipsoide(42.0, 165.0, 0.0, 15.0, 42.0, 15.0) // tronco
    };
    let mut semilla: u32 = 0x9E37_79B9;
    let mut azar = move || {
        semilla = semilla.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (semilla >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut puntos: Vec<(f32, f32, f32, bool)> = Vec::with_capacity(1500);
    let mut intentos = 0;
    while puntos.len() < 1300 && intentos < 40_000 {
        intentos += 1;
        let x = azar() * 430.0 - 215.0;
        let y = azar() * 410.0 - 190.0;
        let z = azar() * 320.0 - 160.0;
        if dentro(x, y, z) {
            puntos.push((x, y, z, azar() < 0.4));
        }
    }
    // El cerebelo va más denso, como en la maqueta.
    while puntos.len() < 1500 && intentos < 60_000 {
        intentos += 1;
        let x = 75.0 + (azar() * 2.0 - 1.0) * 68.0;
        let y = 108.0 + (azar() * 2.0 - 1.0) * 44.0;
        let z = (azar() * 2.0 - 1.0) * 70.0;
        if dentro(x, y, z) {
            puntos.push((x, y, z, azar() < 0.55));
        }
    }

    // Giro lento sobre el eje vertical más la guiñada hacia el cursor, y una
    // inclinación hacia la cámara corregida por el cabeceo.
    let (sa, ca) = (t * 0.45 + mirada.0).sin_cos();
    let (st, ct) = (0.26 + mirada.1).sin_cos();
    let proyectar = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
        let xr = x * ca + z * sa;
        let zr = -x * sa + z * ca;
        let yr = y * ct - zr * st;
        let zf = y * st + zr * ct;
        let f = 900.0 / (900.0 + zf);
        (cx + xr * f * escala, cy + yr * f * escala, zf)
    };
    // Cercanía en [0, 1]: 1 lo más próximo a la cámara.
    let cercania = |zf: f32| ((170.0 - zf) / 340.0).clamp(0.0, 1.0);

    let proyectados: Vec<(f32, f32, f32, bool)> = puntos
        .iter()
        .map(|&(x, y, z, fuerte)| {
            let (sx, sy, zf) = proyectar(x, y, z);
            (sx, sy, zf, fuerte)
        })
        .collect();

    // Trazos: cada punto acentuado (uno de cada dos) con su vecino más cercano
    // en el espacio, buscado entre los sesenta siguientes de la lista.
    for (i, &(x, y, z, fuerte)) in puntos.iter().enumerate() {
        if !fuerte || i % 2 != 0 {
            continue;
        }
        let mut mejor: Option<(usize, f32)> = None;
        for (j, &(ox, oy, oz, _)) in puntos.iter().enumerate().skip(i + 1).take(60) {
            let d = (ox - x).powi(2) + (oy - y).powi(2) + (oz - z).powi(2);
            if d < 32.0 * 32.0 && mejor.map_or(true, |(_, md)| d < md) {
                mejor = Some((j, d));
            }
        }
        if let Some((j, _)) = mejor {
            let (ax, ay, az, _) = proyectados[i];
            let (bx, by, bz, _) = proyectados[j];
            let alfa = 14.0 + 44.0 * cercania((az + bz) * 0.5);
            canvas.draw_line(ax, ay, bx, by, apagado.with_alpha(alfa as u8), 1.0);
        }
    }

    // Puntos de atrás hacia delante, para que lo cercano quede encima.
    let mut orden: Vec<usize> = (0..proyectados.len()).collect();
    orden.sort_by(|&a, &b| {
        proyectados[b].2
            .partial_cmp(&proyectados[a].2)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Bajo el cursor los puntos se encienden y crecen, con caída suave al
    // borde del radio; lejos de él todo sigue igual.
    let radio = lado * 0.22;
    for i in orden {
        let (sx, sy, zf, fuerte) = proyectados[i];
        let c = cercania(zf);
        let brillo = cursor.map_or(0.0, |(mx, my)| {
            let d = ((sx - mx).powi(2) + (sy - my).powi(2)).sqrt();
            (1.0 - d / radio).clamp(0.0, 1.0)
        });
        // Diámetro: un círculo se percibe algo más pequeño que un cuadrado
        // del mismo lado, de ahí el factor.
        let d = (if fuerte { 3.0 } else { 2.0 })
            * (0.65 + 0.7 * c)
            * escala.max(0.6)
            * (1.0 + 0.9 * brillo);
        let color = if fuerte || brillo > 0.35 {
            let base = 80.0 + 175.0 * c;
            acento.with_alpha((base + (255.0 - base) * brillo) as u8)
        } else {
            apagado.with_alpha((28.0 + 96.0 * c) as u8)
        };
        // Bolas con volumen para los puntos acentuados y los encendidos; los
        // grises pequeños, discos: a dos píxeles no se distinguen y así el
        // fotograma no se encarece.
        if fuerte || brillo > 0.35 {
            canvas.fill_sphere(sx, sy, d * 0.5, color);
        } else {
            canvas.fill_circle(sx, sy, d * 0.5, color);
        }
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

    // La barra de pestañas va sobre el blanco de la tarjeta, sin tinte: en la
    // maqueta las pestañas y el código comparten fondo.
    let tab_bar_h = EDITOR_TAB_BAR_HEIGHT;

    let tab_h = tab_bar_h - 3.0;
    let mut tab_x = editor_bounds.x + 8.0;
    let max_tab_right = editor_bounds.x + editor_bounds.width - 160.0;
    // La pestaña activa siempre se ve. Con una sesión de muchas pestañas y una
    // columna estrecha, la tira empezaba en la primera y la activa quedaba fuera:
    // la cabecera decía un archivo y el cuerpo enseñaba otro.
    let ancho = |tab: &TabSnapshot| (tab.title.chars().count() as f32 * 7.0 + 24.0).clamp(86.0, 210.0);
    let disponible = max_tab_right - tab_x;
    let activa = tabs.iter().position(|tab| tab.active).unwrap_or(0);
    let mut inicio = activa;
    let mut usado = tabs.get(activa).map(&ancho).unwrap_or(0.0) + 6.0;
    while inicio > 0 && usado + ancho(&tabs[inicio - 1]) + 6.0 <= disponible {
        inicio -= 1;
        usado += ancho(&tabs[inicio]) + 6.0;
    }
    for tab in tabs.iter().skip(inicio).take(10) {
        let width = ancho(tab);
        if tab_x + width > max_tab_right {
            break;
        }

        let tab_bounds = Bounds::new(tab_x, editor_bounds.y + 2.0, width, tab_h);
        // Como en la maqueta: la activa es una píldora sobre la superficie y
        // las demás son solo texto apagado. Sin cajas ni bordes de un píxel,
        // que era lo que hacía que la barra pareciera un formulario.
        if tab.active {
            canvas.fill_rounded_rect(
                tab_bounds,
                design::radius::SM,
                Color::from_hex(palette.surface),
            );
        }

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
