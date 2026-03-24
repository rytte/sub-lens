use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::{
    DynamicImage, ImageFormat,
    imageops::{FilterType, resize},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{io::Cursor, sync::Mutex, thread, time::Duration};
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager,
};
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};
use tauri_plugin_store::StoreExt;
use xcap::Monitor;

const EVENT_STATE_CHANGED: &str = "overlay://state";
const EVENT_PIXEL_FRAME: &str = "overlay://pixel-frame";
const STORE_PATH: &str = "overlay-settings.json";
const STORE_KEY: &str = "overlay";
const TRAY_ID: &str = "overlay-tray";
const DEFAULT_WIDTH: f64 = 720.0;
const DEFAULT_HEIGHT: f64 = 132.0;
const MIN_WIDTH: f64 = 280.0;
const MIN_HEIGHT: f64 = 36.0;

const MENU_TOGGLE_VISIBLE: &str = "toggle_visible";
const MENU_TOGGLE_LOCK: &str = "toggle_lock";
const MENU_TOGGLE_CLICK_THROUGH: &str = "toggle_click_through";
const MENU_TOGGLE_AUTO_CENTER: &str = "toggle_auto_center";
const MENU_REFRESH_RATE_5: &str = "refresh_rate_5";
const MENU_REFRESH_RATE_10: &str = "refresh_rate_10";
const MENU_REFRESH_RATE_15: &str = "refresh_rate_15";
const MENU_REFRESH_RATE_20: &str = "refresh_rate_20";
const MENU_PIXEL_DETAIL_COARSE: &str = "pixel_detail_coarse";
const MENU_PIXEL_DETAIL_MEDIUM: &str = "pixel_detail_medium";
const MENU_PIXEL_DETAIL_FINE: &str = "pixel_detail_fine";
const MENU_OPACITY_20: &str = "opacity_20";
const MENU_OPACITY_40: &str = "opacity_40";
const MENU_OPACITY_60: &str = "opacity_60";
const MENU_OPACITY_80: &str = "opacity_80";
const MENU_OPACITY_100: &str = "opacity_100";
const MENU_APPEARANCE_PIXELATE: &str = "appearance_pixelate";
const MENU_APPEARANCE_SOLID: &str = "appearance_solid";
const MENU_COLOR_BLACK: &str = "color_black";
const MENU_COLOR_CHARCOAL: &str = "color_charcoal";
const MENU_COLOR_SILVER: &str = "color_silver";
const MENU_COLOR_WHITE: &str = "color_white";
const MENU_RESET_POSITION: &str = "reset_position";
const MENU_TOGGLE_ON_TOP: &str = "toggle_on_top";
const MENU_QUIT: &str = "quit";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OverlayAppearance {
    #[serde(alias = "glass")]
    Pixelate,
    Solid,
}

impl Default for OverlayAppearance {
    fn default() -> Self {
        Self::Pixelate
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum PixelDetail {
    Coarse,
    Medium,
    Fine,
}

impl Default for PixelDetail {
    fn default() -> Self {
        Self::Medium
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PixelFramePayload {
    data_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct OverlayStatePayload {
    visible: bool,
    locked: bool,
    click_through_when_locked: bool,
    always_on_top: bool,
    auto_center: bool,
    refresh_rate: u32,
    pixel_detail: PixelDetail,
    opacity: f64,
    appearance: OverlayAppearance,
    color: String,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}

impl Default for OverlayStatePayload {
    fn default() -> Self {
        Self {
            visible: true,
            locked: false,
            click_through_when_locked: false,
            always_on_top: true,
            auto_center: false,
            refresh_rate: 5,
            pixel_detail: PixelDetail::Medium,
            opacity: 0.82,
            appearance: OverlayAppearance::Pixelate,
            color: "#0d1117".into(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            x: None,
            y: None,
        }
    }
}

#[derive(Debug)]
struct SharedState {
    overlay: OverlayStatePayload,
}

impl SharedState {
    fn new(overlay: OverlayStatePayload) -> Self {
        Self { overlay }
    }
}

struct AppState(Mutex<SharedState>);

fn sanitize_state(mut state: OverlayStatePayload) -> OverlayStatePayload {
    state.opacity = state.opacity.clamp(0.2, 1.0);
    state.width = state.width.max(MIN_WIDTH);
    state.height = state.height.max(MIN_HEIGHT);
    state.refresh_rate = match state.refresh_rate {
        5 | 10 | 15 | 20 => state.refresh_rate,
        _ => OverlayStatePayload::default().refresh_rate,
    };
    if !is_valid_hex_color(&state.color) {
        state.color = OverlayStatePayload::default().color;
    }
    state
}

fn is_valid_hex_color(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.len() == 7
        && chars[0] == '#'
        && chars[1..].iter().all(|c| c.is_ascii_hexdigit())
}

fn is_opacity_selected(current: f64, candidate: f64) -> bool {
    (current - candidate).abs() < 0.01
}

fn is_refresh_rate_selected(current: u32, candidate: u32) -> bool {
    current == candidate
}

fn is_pixel_detail_selected(current: &PixelDetail, candidate: &PixelDetail) -> bool {
    current == candidate
}

fn is_color_selected(current: &str, candidate: &str) -> bool {
    current.eq_ignore_ascii_case(candidate)
}

fn load_saved_state(app: &AppHandle) -> OverlayStatePayload {
    let store = match app.store(STORE_PATH) {
        Ok(store) => store,
        Err(_) => return OverlayStatePayload::default(),
    };

    let state = store
        .get(STORE_KEY)
        .and_then(|value| serde_json::from_value::<OverlayStatePayload>(value).ok())
        .unwrap_or_default();

    store.close_resource();
    sanitize_state(state)
}

fn save_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|err| err.to_string())?;
    store.set(STORE_KEY, json!(overlay));
    store.save().map_err(|err| err.to_string())?;
    store.close_resource();
    Ok(())
}

fn current_overlay_state(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.overlay.clone())
}

fn emit_pixel_frame(app: &AppHandle, data_url: String) -> Result<(), String> {
    app.emit(EVENT_PIXEL_FRAME, PixelFramePayload { data_url })
        .map_err(|err| err.to_string())
}

fn refresh_tray_menu(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let menu = build_overlay_menu(app, overlay).map_err(|err| err.to_string())?;
    tray.set_menu(Some(menu)).map_err(|err| err.to_string())
}

fn emit_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    app.emit(EVENT_STATE_CHANGED, overlay.clone())
        .map_err(|err| err.to_string())
}

fn centered_horizontal_position(
    window: &tauri::WebviewWindow,
    overlay: &OverlayStatePayload,
) -> Result<Option<LogicalPosition<f64>>, String> {
    let monitor = window
        .current_monitor()
        .map_err(|err| err.to_string())?
        .or(window.primary_monitor().map_err(|err| err.to_string())?);

    let Some(monitor) = monitor else {
        return Ok(None);
    };

    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let work_x = f64::from(work_area.position.x) / scale;
    let work_y = f64::from(work_area.position.y) / scale;
    let work_width = f64::from(work_area.size.width) / scale;
    let work_height = f64::from(work_area.size.height) / scale;
    let centered_x = work_x + ((work_width - overlay.width) / 2.0);
    let y = overlay
        .y
        .unwrap_or_else(|| work_y + ((work_height - overlay.height) / 2.0));

    Ok(Some(LogicalPosition::new(centered_x, y)))
}

fn apply_window_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;

    window
        .set_size(LogicalSize::new(overlay.width, overlay.height))
        .map_err(|err| err.to_string())?;

    if overlay.auto_center {
        if let Some(position) = centered_horizontal_position(&window, overlay)? {
            window
                .set_position(position)
                .map_err(|err| err.to_string())?;
        } else {
            window.center().map_err(|err| err.to_string())?;
        }
    } else if let (Some(x), Some(y)) = (overlay.x, overlay.y) {
        window
            .set_position(LogicalPosition::new(x, y))
            .map_err(|err| err.to_string())?;
    } else {
        window.center().map_err(|err| err.to_string())?;
    }

    window
        .set_always_on_top(overlay.always_on_top)
        .map_err(|err| err.to_string())?;
    window
        .set_ignore_cursor_events(overlay.locked && overlay.click_through_when_locked)
        .map_err(|err| err.to_string())?;
    window
        .set_content_protected(matches!(
            overlay.appearance,
            OverlayAppearance::Pixelate
        ))
        .map_err(|err| err.to_string())?;
    window.set_effects(None::<tauri::utils::config::WindowEffectsConfig>)
        .map_err(|err| err.to_string())?;

    if overlay.visible {
        window.show().map_err(|err| err.to_string())?;
    } else {
        window.hide().map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn update_overlay_state<F>(
    app: &AppHandle,
    persist: bool,
    focus_window: bool,
    updater: F,
) -> Result<OverlayStatePayload, String>
where
    F: FnOnce(&mut SharedState),
{
    let overlay = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        updater(&mut shared);
        shared.overlay = sanitize_state(shared.overlay.clone());
        shared.overlay.clone()
    };

    apply_window_state(app, &overlay)?;

    if persist {
        save_state(app, &overlay)?;
    }

    refresh_tray_menu(app, &overlay)?;
    emit_state(app, &overlay)?;

    if focus_window && overlay.visible && !overlay.locked {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }

    Ok(overlay)
}

fn toggle_visibility(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.visible = !shared.overlay.visible;
    })
}

fn toggle_lock(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.locked = !shared.overlay.locked;
    })
}

fn toggle_click_through(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.click_through_when_locked = !shared.overlay.click_through_when_locked;
    })
}

fn reset_overlay_position_inner(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, true, |shared| {
        shared.overlay.visible = true;
        shared.overlay.locked = false;
        shared.overlay.x = None;
        shared.overlay.y = None;
    })
}

fn toggle_on_top(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.always_on_top = !shared.overlay.always_on_top;
    })
}

fn toggle_auto_center(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;

    update_overlay_state(app, true, false, move |shared| {
        let next_auto_center = !shared.overlay.auto_center;
        shared.overlay.auto_center = next_auto_center;

        if next_auto_center {
            shared.overlay.x = None;
        } else {
            shared.overlay.x = Some(f64::from(position.x) / scale_factor);
        }

        shared.overlay.y = Some(f64::from(position.y) / scale_factor);
    })
}

fn set_opacity(app: &AppHandle, opacity: f64) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.opacity = opacity;
    })
}

fn set_refresh_rate(app: &AppHandle, refresh_rate: u32) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.refresh_rate = refresh_rate;
    })
}

fn set_pixel_detail(app: &AppHandle, pixel_detail: PixelDetail) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.pixel_detail = pixel_detail;
    })
}

fn set_appearance(app: &AppHandle, appearance: OverlayAppearance) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.appearance = appearance;
    })
}

fn set_color(app: &AppHandle, color: &str) -> Result<OverlayStatePayload, String> {
    let color = color.to_string();
    update_overlay_state(app, true, false, move |shared| {
        shared.overlay.color = color;
    })
}

fn build_overlay_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let visibility = MenuItem::with_id(
        app,
        MENU_TOGGLE_VISIBLE,
        if overlay.visible {
            "隐藏遮挡条"
        } else {
            "显示遮挡条"
        },
        true,
        Some("Ctrl+Alt+H"),
    )?;
    let lock = MenuItem::with_id(
        app,
        MENU_TOGGLE_LOCK,
        if overlay.locked {
            "解锁遮挡条"
        } else {
            "锁定遮挡条"
        },
        true,
        Some("Ctrl+Alt+Space"),
    )?;
    let click_through = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_CLICK_THROUGH,
        "锁定时点击穿透",
        true,
        overlay.click_through_when_locked,
        None::<&str>,
    )?;
    let auto_center = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_AUTO_CENTER,
        "自动水平居中",
        true,
        overlay.auto_center,
        None::<&str>,
    )?;
    let reset_position =
        MenuItem::with_id(app, MENU_RESET_POSITION, "重置位置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let always_on_top = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_ON_TOP,
        "始终置顶",
        true,
        overlay.always_on_top,
        None::<&str>,
    )?;

    let opacity_20 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_20,
        "20%",
        true,
        is_opacity_selected(overlay.opacity, 0.2),
        None::<&str>,
    )?;
    let opacity_40 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_40,
        "40%",
        true,
        is_opacity_selected(overlay.opacity, 0.4),
        None::<&str>,
    )?;
    let opacity_60 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_60,
        "60%",
        true,
        is_opacity_selected(overlay.opacity, 0.6),
        None::<&str>,
    )?;
    let opacity_80 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_80,
        "80%",
        true,
        is_opacity_selected(overlay.opacity, 0.8),
        None::<&str>,
    )?;
    let opacity_100 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_100,
        "100%",
        true,
        is_opacity_selected(overlay.opacity, 1.0),
        None::<&str>,
    )?;
    let opacity_items: [&dyn IsMenuItem<tauri::Wry>; 5] = [
        &opacity_20,
        &opacity_40,
        &opacity_60,
        &opacity_80,
        &opacity_100,
    ];
    let opacity_menu = Submenu::with_items(app, "不透明度", true, &opacity_items)?;

    let refresh_rate_5 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_5,
        "5 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 5),
        None::<&str>,
    )?;
    let refresh_rate_10 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_10,
        "10 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 10),
        None::<&str>,
    )?;
    let refresh_rate_15 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_15,
        "15 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 15),
        None::<&str>,
    )?;
    let refresh_rate_20 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_20,
        "20 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 20),
        None::<&str>,
    )?;
    let refresh_rate_items: [&dyn IsMenuItem<tauri::Wry>; 4] = [
        &refresh_rate_5,
        &refresh_rate_10,
        &refresh_rate_15,
        &refresh_rate_20,
    ];
    let refresh_rate_menu = Submenu::with_items(
        app,
        "刷新率",
        overlay.appearance == OverlayAppearance::Pixelate,
        &refresh_rate_items,
    )?;

    let pixel_detail_coarse = CheckMenuItem::with_id(
        app,
        MENU_PIXEL_DETAIL_COARSE,
        "粗",
        true,
        is_pixel_detail_selected(&overlay.pixel_detail, &PixelDetail::Coarse),
        None::<&str>,
    )?;
    let pixel_detail_medium = CheckMenuItem::with_id(
        app,
        MENU_PIXEL_DETAIL_MEDIUM,
        "中",
        true,
        is_pixel_detail_selected(&overlay.pixel_detail, &PixelDetail::Medium),
        None::<&str>,
    )?;
    let pixel_detail_fine = CheckMenuItem::with_id(
        app,
        MENU_PIXEL_DETAIL_FINE,
        "细",
        true,
        is_pixel_detail_selected(&overlay.pixel_detail, &PixelDetail::Fine),
        None::<&str>,
    )?;
    let pixel_detail_items: [&dyn IsMenuItem<tauri::Wry>; 3] = [
        &pixel_detail_coarse,
        &pixel_detail_medium,
        &pixel_detail_fine,
    ];
    let pixel_detail_menu = Submenu::with_items(
        app,
        "像素块",
        overlay.appearance == OverlayAppearance::Pixelate,
        &pixel_detail_items,
    )?;

    let appearance_pixelate = CheckMenuItem::with_id(
        app,
        MENU_APPEARANCE_PIXELATE,
        "像素化",
        true,
        overlay.appearance == OverlayAppearance::Pixelate,
        None::<&str>,
    )?;
    let appearance_solid = CheckMenuItem::with_id(
        app,
        MENU_APPEARANCE_SOLID,
        "纯色",
        true,
        overlay.appearance == OverlayAppearance::Solid,
        None::<&str>,
    )?;
    let appearance_items: [&dyn IsMenuItem<tauri::Wry>; 2] =
        [&appearance_pixelate, &appearance_solid];
    let appearance_menu = Submenu::with_items(app, "遮挡样式", true, &appearance_items)?;

    let color_black = CheckMenuItem::with_id(
        app,
        MENU_COLOR_BLACK,
        "黑色",
        true,
        is_color_selected(&overlay.color, "#0b0d12"),
        None::<&str>,
    )?;
    let color_charcoal = CheckMenuItem::with_id(
        app,
        MENU_COLOR_CHARCOAL,
        "深灰",
        true,
        is_color_selected(&overlay.color, "#2b3038"),
        None::<&str>,
    )?;
    let color_silver = CheckMenuItem::with_id(
        app,
        MENU_COLOR_SILVER,
        "浅灰",
        true,
        is_color_selected(&overlay.color, "#d0d5dd"),
        None::<&str>,
    )?;
    let color_white = CheckMenuItem::with_id(
        app,
        MENU_COLOR_WHITE,
        "白色",
        true,
        is_color_selected(&overlay.color, "#f8fafc"),
        None::<&str>,
    )?;
    let color_items: [&dyn IsMenuItem<tauri::Wry>; 4] =
        [&color_black, &color_charcoal, &color_silver, &color_white];
    let color_menu = Submenu::with_items(
        app,
        "颜色",
        overlay.appearance == OverlayAppearance::Solid,
        &color_items,
    )?;

    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;

    let items: [&dyn IsMenuItem<tauri::Wry>; 13] = [
        &visibility,
        &lock,
        &click_through,
        &auto_center,
        &separator_a,
        &refresh_rate_menu,
        &pixel_detail_menu,
        &opacity_menu,
        &appearance_menu,
        &color_menu,
        &separator_b,
        &always_on_top,
        &reset_position,
    ];
    let menu = Menu::with_items(app, &items)?;
    menu.append(&separator_c)?;
    menu.append(&quit)?;
    Ok(menu)
}

fn handle_menu_action(app: &AppHandle, id: &str) {
    let result = match id {
        MENU_TOGGLE_VISIBLE => toggle_visibility(app),
        MENU_TOGGLE_LOCK => toggle_lock(app),
        MENU_TOGGLE_CLICK_THROUGH => toggle_click_through(app),
        MENU_TOGGLE_AUTO_CENTER => toggle_auto_center(app),
        MENU_REFRESH_RATE_5 => set_refresh_rate(app, 5),
        MENU_REFRESH_RATE_10 => set_refresh_rate(app, 10),
        MENU_REFRESH_RATE_15 => set_refresh_rate(app, 15),
        MENU_REFRESH_RATE_20 => set_refresh_rate(app, 20),
        MENU_PIXEL_DETAIL_COARSE => set_pixel_detail(app, PixelDetail::Coarse),
        MENU_PIXEL_DETAIL_MEDIUM => set_pixel_detail(app, PixelDetail::Medium),
        MENU_PIXEL_DETAIL_FINE => set_pixel_detail(app, PixelDetail::Fine),
        MENU_OPACITY_20 => set_opacity(app, 0.2),
        MENU_OPACITY_40 => set_opacity(app, 0.4),
        MENU_OPACITY_60 => set_opacity(app, 0.6),
        MENU_OPACITY_80 => set_opacity(app, 0.8),
        MENU_OPACITY_100 => set_opacity(app, 1.0),
        MENU_APPEARANCE_PIXELATE => set_appearance(app, OverlayAppearance::Pixelate),
        MENU_APPEARANCE_SOLID => set_appearance(app, OverlayAppearance::Solid),
        MENU_COLOR_BLACK => set_color(app, "#0b0d12"),
        MENU_COLOR_CHARCOAL => set_color(app, "#2b3038"),
        MENU_COLOR_SILVER => set_color(app, "#d0d5dd"),
        MENU_COLOR_WHITE => set_color(app, "#f8fafc"),
        MENU_RESET_POSITION => reset_overlay_position_inner(app),
        MENU_TOGGLE_ON_TOP => toggle_on_top(app),
        MENU_QUIT => {
            app.exit(0);
            return;
        }
        _ => Ok(current_overlay_state(app).unwrap_or_default()),
    };

    if let Err(err) = result {
        eprintln!("menu action failed: {err}");
    }
}

#[tauri::command]
fn show_overlay_menu(app: AppHandle, x: f64, y: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let overlay = current_overlay_state(&app)?;
    let menu = build_overlay_menu(&app, &overlay).map_err(|err| err.to_string())?;

    window
        .popup_menu_at(&menu, LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn load_overlay_state(app: AppHandle) -> Result<OverlayStatePayload, String> {
    current_overlay_state(&app)
}

#[tauri::command]
fn apply_overlay_state(
    app: AppHandle,
    next_state: OverlayStatePayload,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(&app, true, false, move |shared| {
        shared.overlay = next_state;
    })
}

#[tauri::command]
fn toggle_overlay_visibility(app: AppHandle) -> Result<OverlayStatePayload, String> {
    toggle_visibility(&app)
}

#[tauri::command]
fn toggle_overlay_lock(app: AppHandle) -> Result<OverlayStatePayload, String> {
    toggle_lock(&app)
}

#[tauri::command]
fn toggle_overlay_on_top(app: AppHandle) -> Result<OverlayStatePayload, String> {
    toggle_on_top(&app)
}

#[tauri::command]
fn reset_overlay_position(app: AppHandle) -> Result<OverlayStatePayload, String> {
    reset_overlay_position_inner(&app)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn capture_pixelated_frame(app: &AppHandle) -> Result<Option<String>, String> {
    let overlay = current_overlay_state(app)?;
    if !overlay.visible || overlay.appearance != OverlayAppearance::Pixelate {
        return Ok(None);
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;
    let size = window.inner_size().map_err(|err| err.to_string())?;
    let center_x = position.x + (size.width as i32 / 2);
    let center_y = position.y + (size.height as i32 / 2);

    let monitor = Monitor::from_point(center_x, center_y).map_err(|err| err.to_string())?;
    let monitor_x = monitor.x().map_err(|err| err.to_string())?;
    let monitor_y = monitor.y().map_err(|err| err.to_string())?;
    let capture_x = (position.x - monitor_x).max(0) as u32;
    let capture_y = (position.y - monitor_y).max(0) as u32;
    let capture_width = size.width.max(1);
    let capture_height = size.height.max(1);

    let image = monitor
        .capture_region(capture_x, capture_y, capture_width, capture_height)
        .map_err(|err| err.to_string())?;
    let pixelated = pixelate_image(image, &overlay.pixel_detail);
    let data_url = encode_image_data_url(pixelated)?;
    Ok(Some(data_url))
}

fn pixelate_image(image: image::RgbaImage, pixel_detail: &PixelDetail) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let short_side = width.min(height);
    let block = match pixel_detail {
        PixelDetail::Coarse => (short_side / 8).clamp(12, 36),
        PixelDetail::Medium => (short_side / 12).clamp(8, 28),
        PixelDetail::Fine => (short_side / 16).clamp(6, 20),
    };
    let sample_width = (width / block).max(1);
    let sample_height = (height / block).max(1);
    resize(&image, sample_width, sample_height, FilterType::Triangle)
}

fn encode_image_data_url(image: image::RgbaImage) -> Result<String, String> {
    let mut buffer = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut buffer, ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(buffer.into_inner())
    ))
}

fn start_pixelate_loop(app: AppHandle) {
    thread::spawn(move || loop {
        let refresh_rate = current_overlay_state(&app)
            .map(|state| state.refresh_rate.max(1))
            .unwrap_or(OverlayStatePayload::default().refresh_rate);
        let frame_interval_ms = (1000 / refresh_rate).max(1) as u64;
        thread::sleep(Duration::from_millis(frame_interval_ms));

        match capture_pixelated_frame(&app) {
            Ok(Some(data_url)) => {
                if let Err(err) = emit_pixel_frame(&app, data_url) {
                    eprintln!("pixel frame emit failed: {err}");
                }
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("pixel capture failed: {err}");
            }
        }
    });
}

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let overlay = current_overlay_state(app).map_err(|err| err.to_string())?;
    let menu = build_overlay_menu(app, &overlay)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .ok_or("missing default window icon")?
                .clone(),
        )
        .tooltip("字幕遮挡")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Err(err) = toggle_visibility(&app) {
                    eprintln!("tray left click failed: {err}");
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn setup_shortcuts(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let toggle_visibility_shortcut = Shortcut::try_from("ctrl+alt+h")?;
    let toggle_lock_shortcut = Shortcut::try_from("ctrl+alt+space")?;

    let toggle_visibility_for_handler = toggle_visibility_shortcut;
    let toggle_lock_for_handler = toggle_lock_shortcut;

    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_shortcuts([toggle_visibility_shortcut, toggle_lock_shortcut])?
            .with_handler(move |app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }

                let result = if shortcut == &toggle_visibility_for_handler {
                    toggle_visibility(app)
                } else if shortcut == &toggle_lock_for_handler {
                    toggle_lock(app)
                } else {
                    return;
                };

                if let Err(err) = result {
                    eprintln!("shortcut handler failed: {err}");
                }
            })
            .build(),
    )?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .on_menu_event(|app, event| {
            handle_menu_action(app, event.id.as_ref());
        })
        .setup(|app| {
            let overlay = load_saved_state(app.handle());
            app.manage(AppState(Mutex::new(SharedState::new(overlay.clone()))));

            apply_window_state(app.handle(), &overlay)?;
            emit_state(app.handle(), &overlay)?;
            setup_tray(app.handle())?;
            start_pixelate_loop(app.handle().clone());

            #[cfg(desktop)]
            setup_shortcuts(app.handle())?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_overlay_state,
            apply_overlay_state,
            toggle_overlay_visibility,
            toggle_overlay_lock,
            toggle_overlay_on_top,
            reset_overlay_position,
            show_overlay_menu,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
