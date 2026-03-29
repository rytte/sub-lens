use super::*;

pub(super) fn effective_overlay_visibility(
    overlay: &OverlayStatePayload,
    ctrl_pressed: bool,
) -> bool {
    overlay.visible && !ctrl_pressed
}

pub(super) fn update_overlay_state<F>(
    app: &AppHandle,
    persist: bool,
    focus_window: bool,
    apply_geometry: bool,
    updater: F,
) -> Result<OverlayStatePayload, String>
where
    F: FnOnce(&mut SharedState),
{
    let (overlay, ctrl_pressed) = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        updater(&mut shared);
        shared.overlay = sanitize_state(shared.overlay.clone());
        (shared.overlay.clone(), shared.ctrl_pressed)
    };

    super::app_window::apply_window_state(app, &overlay, ctrl_pressed, apply_geometry)?;

    if persist {
        save_state(app, &overlay)?;
    }

    super::menu::refresh_tray_menu(app, &overlay)?;
    emit_state(app, &overlay)?;

    if focus_window && overlay.visible && !overlay.locked {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }

    Ok(overlay)
}

pub(super) fn set_ctrl_pressed(app: &AppHandle, ctrl_pressed: bool) -> Result<(), String> {
    let effective_visible = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;

        if shared.ctrl_pressed == ctrl_pressed {
            return Ok(());
        }

        shared.ctrl_pressed = ctrl_pressed;
        effective_overlay_visibility(&shared.overlay, shared.ctrl_pressed)
    };

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;

    super::app_window::apply_window_visibility(&window, effective_visible)
}

pub(super) fn toggle_visibility(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.visible = !shared.overlay.visible;
    })
}

pub(super) fn toggle_lock(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.locked = !shared.overlay.locked;
    })
}

pub(super) fn toggle_click_through(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.click_through_when_locked = !shared.overlay.click_through_when_locked;
    })
}

pub(super) fn reset_overlay_position_inner(
    app: &AppHandle,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, true, true, |shared| {
        shared.overlay.visible = true;
        shared.overlay.locked = false;
        shared.overlay.x = None;
        shared.overlay.y = None;
    })
}

pub(super) fn toggle_on_top(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.always_on_top = !shared.overlay.always_on_top;
    })
}

pub(super) fn toggle_auto_center(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;

    update_overlay_state(app, true, false, true, move |shared| {
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

pub(super) fn set_opacity(app: &AppHandle, opacity: f64) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.opacity = opacity;
    })
}

pub(super) fn set_refresh_rate(
    app: &AppHandle,
    refresh_rate: u32,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.refresh_rate = refresh_rate;
    })
}

pub(super) fn set_pixel_detail(
    app: &AppHandle,
    pixel_detail: PixelDetail,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.pixel_detail = pixel_detail;
    })
}

pub(super) fn set_appearance(
    app: &AppHandle,
    appearance: OverlayAppearance,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.appearance = appearance;
    })
}

pub(super) fn set_color(app: &AppHandle, color: &str) -> Result<OverlayStatePayload, String> {
    let color = color.to_string();
    update_overlay_state(app, true, false, false, move |shared| {
        shared.overlay.color = color;
    })
}

fn capture_pixelated_frame(app: &AppHandle) -> Result<Option<String>, String> {
    let overlay = current_overlay_state(app)?;
    if !overlay.visible || overlay.appearance != OverlayAppearance::Pixelate {
        return Ok(None);
    }

    if overlay_interaction_active(app)? {
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

pub(super) fn start_pixelate_loop(app: AppHandle) {
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

#[cfg(target_os = "windows")]
fn is_virtual_key_pressed(virtual_key: i32) -> bool {
    unsafe { (GetAsyncKeyState(virtual_key) as u16 & 0x8000) != 0 }
}

#[cfg(target_os = "windows")]
fn current_ctrl_hide_active() -> bool {
    let ctrl_pressed =
        is_virtual_key_pressed(VK_LCONTROL as i32) || is_virtual_key_pressed(VK_RCONTROL as i32);
    let alt_pressed = is_virtual_key_pressed(VK_MENU as i32)
        || is_virtual_key_pressed(VK_LMENU as i32)
        || is_virtual_key_pressed(VK_RMENU as i32);
    let shift_pressed = is_virtual_key_pressed(VK_SHIFT as i32)
        || is_virtual_key_pressed(VK_LSHIFT as i32)
        || is_virtual_key_pressed(VK_RSHIFT as i32);
    let win_pressed =
        is_virtual_key_pressed(VK_LWIN as i32) || is_virtual_key_pressed(VK_RWIN as i32);

    ctrl_pressed && !alt_pressed && !shift_pressed && !win_pressed
}

#[cfg(not(target_os = "windows"))]
fn current_ctrl_hide_active() -> bool {
    false
}

#[cfg(target_os = "windows")]
pub(super) fn current_ctrl_pressed() -> bool {
    current_ctrl_hide_active()
}

#[cfg(not(target_os = "windows"))]
pub(super) fn current_ctrl_pressed() -> bool {
    false
}

#[cfg(target_os = "windows")]
pub(super) fn start_ctrl_hold_loop(app: AppHandle) {
    thread::spawn(move || {
        let mut last_ctrl_pressed = current_ctrl_pressed();

        loop {
            thread::sleep(Duration::from_millis(CTRL_HIDE_POLL_INTERVAL_MS));

            let ctrl_pressed = current_ctrl_pressed();
            if ctrl_pressed == last_ctrl_pressed {
                continue;
            }

            last_ctrl_pressed = ctrl_pressed;

            if let Err(err) = set_ctrl_pressed(&app, ctrl_pressed) {
                eprintln!("ctrl hold handler failed: {err}");
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub(super) fn start_ctrl_hold_loop(_app: AppHandle) {}
