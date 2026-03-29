use super::*;

pub(super) fn sync_overlay_state_from_window(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let Some(window) = app.get_webview_window("main") else {
        return current_overlay_state(app);
    };

    let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;
    let size = window.inner_size().map_err(|err| err.to_string())?;

    let overlay = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.overlay.width = f64::from(size.width) / scale_factor;
        shared.overlay.height = f64::from(size.height) / scale_factor;
        if shared.overlay.auto_center {
            shared.overlay.x = None;
        } else {
            shared.overlay.x = Some(f64::from(position.x) / scale_factor);
        }
        shared.overlay.y = Some(f64::from(position.y) / scale_factor);
        shared.overlay = sanitize_state(shared.overlay.clone());
        shared.overlay.clone()
    };

    save_state(app, &overlay)?;
    Ok(overlay)
}

pub(super) fn sync_subtitle_explain_window_state_from_window(
    app: &AppHandle,
    visible_override: Option<bool>,
) -> Result<SubtitleExplainWindowStatePayload, String> {
    let mut next_state = current_subtitle_explain_window_state(app)?;
    let window_geometry =
        if let Some(window) = app.get_webview_window(SUBTITLE_EXPLAIN_WINDOW_LABEL) {
            let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
            let size = window.inner_size().map_err(|err| err.to_string())?;
            let position = window.outer_position().map_err(|err| err.to_string())?;
            Some((
                f64::from(size.width) / scale_factor,
                f64::from(size.height) / scale_factor,
                f64::from(position.x) / scale_factor,
                f64::from(position.y) / scale_factor,
            ))
        } else {
            None
        };

    if let Some((width, height, x, y)) = window_geometry {
        next_state.width = width;
        next_state.height = height;
        next_state.x = Some(x);
        next_state.y = Some(y);
    }
    if let Some(visible) = visible_override {
        next_state.visible = visible;
    }

    commit_subtitle_explain_window_state(app, next_state, false, true, false)
}

pub(super) fn sync_ocr_region_state_from_window(
    app: &AppHandle,
    visible_override: Option<bool>,
) -> Result<OcrRegionStatePayload, String> {
    let mut next_state = current_ocr_region_state(app)?;
    let window_geometry = if let Some(window) = app.get_webview_window(OCR_REGION_WINDOW_LABEL) {
        let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
        let size = window.inner_size().map_err(|err| err.to_string())?;
        let position = window.outer_position().map_err(|err| err.to_string())?;
        Some((
            f64::from(size.width) / scale_factor,
            f64::from(size.height) / scale_factor,
            f64::from(position.x) / scale_factor,
            f64::from(position.y) / scale_factor,
        ))
    } else {
        None
    };

    if let Some((width, height, x, y)) = window_geometry {
        next_state.width = width;
        next_state.height = height;
        next_state.x = Some(x);
        next_state.y = Some(y);
    }
    if let Some(visible) = visible_override {
        next_state.visible = visible;
    }

    commit_ocr_region_state(app, next_state, false, true, false)
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

fn capture_target_monitor(
    window: &tauri::WebviewWindow,
) -> Result<(Monitor, f64, f64, f64, f64), String> {
    let position = window.outer_position().map_err(|err| err.to_string())?;
    let size = window.inner_size().map_err(|err| err.to_string())?;
    let center_x = position.x + (size.width as i32 / 2);
    let center_y = position.y + (size.height as i32 / 2);

    let monitor = Monitor::from_point(center_x, center_y).map_err(|err| err.to_string())?;
    let scale = f64::from(monitor.scale_factor().map_err(|err| err.to_string())?);
    let viewport_x = f64::from(monitor.x().map_err(|err| err.to_string())?) / scale;
    let viewport_y = f64::from(monitor.y().map_err(|err| err.to_string())?) / scale;
    let viewport_width = f64::from(monitor.width().map_err(|err| err.to_string())?) / scale;
    let viewport_height = f64::from(monitor.height().map_err(|err| err.to_string())?) / scale;

    Ok((
        monitor,
        viewport_x,
        viewport_y,
        viewport_width,
        viewport_height,
    ))
}

fn restore_static_selection_windows_snapshot(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
    ctrl_pressed: bool,
    ocr_region: &OcrRegionStatePayload,
    subtitle_explain: &SubtitleExplainWindowStatePayload,
    settings_was_visible: bool,
) -> Result<(), String> {
    apply_window_state(app, overlay, ctrl_pressed, false)?;
    apply_ocr_region_window_state(app, ocr_region)?;
    apply_subtitle_explain_window_state(app, subtitle_explain, false)?;

    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        if settings_was_visible {
            let _ = window.unminimize();
            window.show().map_err(|err| err.to_string())?;
        } else if window.is_visible().map_err(|err| err.to_string())? {
            window.hide().map_err(|err| err.to_string())?;
        }
    }

    Ok(())
}

pub(super) fn restore_static_screen_selection_windows(
    app: &AppHandle,
    session: &StaticScreenSelectionSession,
) -> Result<(), String> {
    restore_static_selection_windows_snapshot(
        app,
        &session.overlay,
        session.ctrl_pressed,
        &session.ocr_region,
        &session.subtitle_explain,
        session.settings_was_visible,
    )
}

fn present_ocr_region_draw_request(
    app: &AppHandle,
    draw_request: OcrRegionDrawRequestPayload,
    static_screen_selection: Option<StaticScreenSelectionSession>,
) -> Result<(), String> {
    let window = ensure_ocr_region_window(app)?;
    if window.is_visible().map_err(|err| err.to_string())? {
        window.hide().map_err(|err| err.to_string())?;
    }

    start_ocr_region_drawing_session(app, draw_request.clone(), static_screen_selection)?;
    let result = (|| -> Result<(), String> {
        window
            .set_ignore_cursor_events(false)
            .map_err(|err| err.to_string())?;
        window
            .set_size(LogicalSize::new(
                draw_request.viewport_width,
                draw_request.viewport_height,
            ))
            .map_err(|err| err.to_string())?;
        window
            .set_position(LogicalPosition::new(
                draw_request.viewport_x,
                draw_request.viewport_y,
            ))
            .map_err(|err| err.to_string())?;
        window
            .emit(EVENT_OCR_REGION_DRAW_REQUEST, draw_request)
            .map_err(|err| err.to_string())?;
        let _ = window.unminimize();
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = finish_ocr_region_drawing_session(app);
        let _ = window.hide();
        return Err(err);
    }

    Ok(())
}

pub(super) fn apply_window_visibility(
    window: &tauri::WebviewWindow,
    visible: bool,
) -> Result<(), String> {
    if visible {
        window.show().map_err(|err| err.to_string())?;
    } else {
        window.hide().map_err(|err| err.to_string())?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn disable_window_transitions(window: &tauri::WebviewWindow) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    let transitions_disabled: i32 = 1;
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd.0 as _,
            DWMWA_TRANSITIONS_FORCEDISABLED as u32,
            &transitions_disabled as *const _ as _,
            std::mem::size_of_val(&transitions_disabled) as u32,
        )
    };

    if result < 0 {
        return Err(format!(
            "failed to disable window transitions: HRESULT {result}"
        ));
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn disable_window_transitions(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

pub(super) fn apply_window_state(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
    ctrl_pressed: bool,
    apply_geometry: bool,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;

    if apply_geometry {
        window
            .set_size(LogicalSize::new(overlay.width, overlay.height))
            .map_err(|err| err.to_string())?;

        if overlay.auto_center {
            if let Some(position) = centered_horizontal_position(&window, overlay)? {
                window.set_position(position).map_err(|err| err.to_string())?;
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
    }

    window
        .set_always_on_top(overlay.always_on_top)
        .map_err(|err| err.to_string())?;
    window
        .set_ignore_cursor_events(overlay.locked && overlay.click_through_when_locked)
        .map_err(|err| err.to_string())?;
    window
        .set_content_protected(matches!(overlay.appearance, OverlayAppearance::Pixelate))
        .map_err(|err| err.to_string())?;
    window
        .set_effects(None::<tauri::utils::config::WindowEffectsConfig>)
        .map_err(|err| err.to_string())?;

    apply_window_visibility(
        &window,
        super::overlay::effective_overlay_visibility(overlay, ctrl_pressed),
    )
}

pub(super) fn ensure_ocr_region_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(OCR_REGION_WINDOW_LABEL) {
        window
            .set_min_size(Some(LogicalSize::new(
                MIN_OCR_REGION_WIDTH,
                MIN_OCR_REGION_HEIGHT,
            )))
            .map_err(|err| err.to_string())?;
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(
        app,
        OCR_REGION_WINDOW_LABEL,
        WebviewUrl::App("pages/ocr-region.html".into()),
    )
    .title("OCR 区域")
    .inner_size(DEFAULT_OCR_REGION_WIDTH, DEFAULT_OCR_REGION_HEIGHT)
    .min_inner_size(MIN_OCR_REGION_WIDTH, MIN_OCR_REGION_HEIGHT)
    .resizable(true)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .center()
    .visible(false)
    .build()
    .map_err(|err| err.to_string())?;

    disable_window_transitions(&window)?;
    window
        .set_min_size(Some(LogicalSize::new(
            MIN_OCR_REGION_WIDTH,
            MIN_OCR_REGION_HEIGHT,
        )))
        .map_err(|err| err.to_string())?;
    Ok(window)
}

pub(super) fn ensure_main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_min_size(Some(LogicalSize::new(MIN_WIDTH, MIN_HEIGHT)))
            .map_err(|err| err.to_string())?;
        return Ok(window);
    }

    let window =
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App("pages/overlay.html".into()))
            .title("字幕遮挡")
            .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
            .resizable(true)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .center()
            .visible(false)
            .build()
            .map_err(|err| err.to_string())?;

    disable_window_transitions(&window)?;
    window
        .set_min_size(Some(LogicalSize::new(MIN_WIDTH, MIN_HEIGHT)))
        .map_err(|err| err.to_string())?;
    Ok(window)
}

pub(super) fn ensure_ocr_runtime_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(OCR_RUNTIME_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(
        app,
        OCR_RUNTIME_WINDOW_LABEL,
        WebviewUrl::App("pages/ocr-runtime.html".into()),
    )
    .title("OCR Runtime")
    .inner_size(1.0, 1.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|err| err.to_string())?;

    disable_window_transitions(&window)?;
    Ok(window)
}

fn ensure_subtitle_explain_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(SUBTITLE_EXPLAIN_WINDOW_LABEL) {
        window
            .set_min_size(Some(LogicalSize::new(
                MIN_SUBTITLE_EXPLAIN_WINDOW_WIDTH,
                MIN_SUBTITLE_EXPLAIN_WINDOW_HEIGHT,
            )))
            .map_err(|err| err.to_string())?;
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(
        app,
        SUBTITLE_EXPLAIN_WINDOW_LABEL,
        WebviewUrl::App("pages/subtitle-explain.html".into()),
    )
    .title("字幕解释")
    .inner_size(
        DEFAULT_SUBTITLE_EXPLAIN_WINDOW_WIDTH,
        DEFAULT_SUBTITLE_EXPLAIN_WINDOW_HEIGHT,
    )
    .min_inner_size(
        MIN_SUBTITLE_EXPLAIN_WINDOW_WIDTH,
        MIN_SUBTITLE_EXPLAIN_WINDOW_HEIGHT,
    )
    .resizable(true)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .center()
    .visible(false)
    .build()
    .map_err(|err| err.to_string())?;

    disable_window_transitions(&window)?;
    window
        .set_min_size(Some(LogicalSize::new(
            MIN_SUBTITLE_EXPLAIN_WINDOW_WIDTH,
            MIN_SUBTITLE_EXPLAIN_WINDOW_HEIGHT,
        )))
        .map_err(|err| err.to_string())?;
    Ok(window)
}

pub(super) fn apply_subtitle_explain_window_state(
    app: &AppHandle,
    state: &SubtitleExplainWindowStatePayload,
    focus_window: bool,
) -> Result<(), String> {
    let window = ensure_subtitle_explain_window(app)?;

    window
        .set_size(LogicalSize::new(state.width, state.height))
        .map_err(|err| err.to_string())?;

    if let (Some(x), Some(y)) = (state.x, state.y) {
        window
            .set_position(LogicalPosition::new(x, y))
            .map_err(|err| err.to_string())?;
    } else {
        window.center().map_err(|err| err.to_string())?;
    }

    if state.visible {
        let _ = window.unminimize();
        window.show().map_err(|err| err.to_string())?;
        if focus_window {
            window.set_focus().map_err(|err| err.to_string())?;
        }
    } else {
        window.hide().map_err(|err| err.to_string())?;
    }

    Ok(())
}

pub(super) fn commit_subtitle_explain_window_state(
    app: &AppHandle,
    next_state: SubtitleExplainWindowStatePayload,
    apply_window: bool,
    persist: bool,
    focus_window: bool,
) -> Result<SubtitleExplainWindowStatePayload, String> {
    let state = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.subtitle_explain_window = sanitize_subtitle_explain_window_state(next_state);
        shared.subtitle_explain_window.clone()
    };

    if apply_window {
        apply_subtitle_explain_window_state(app, &state, focus_window)?;
    }

    if persist {
        save_subtitle_explain_window_state(app, &state)?;
    }

    Ok(state)
}

pub(super) fn apply_ocr_region_window_state(
    app: &AppHandle,
    region: &OcrRegionStatePayload,
) -> Result<(), String> {
    let window = ensure_ocr_region_window(app)?;

    window
        .set_size(LogicalSize::new(region.width, region.height))
        .map_err(|err| err.to_string())?;

    if let (Some(x), Some(y)) = (region.x, region.y) {
        window
            .set_position(LogicalPosition::new(x, y))
            .map_err(|err| err.to_string())?;
    } else {
        window.center().map_err(|err| err.to_string())?;
    }

    window
        .set_ignore_cursor_events(!region.visible && region.show_border_when_idle)
        .map_err(|err| err.to_string())?;
    apply_window_visibility(&window, effective_ocr_region_visibility(region))
}

pub(super) fn commit_ocr_region_state(
    app: &AppHandle,
    next_state: OcrRegionStatePayload,
    apply_window: bool,
    persist: bool,
    focus_window: bool,
) -> Result<OcrRegionStatePayload, String> {
    let region = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.ocr_region = sanitize_ocr_region_state(next_state);
        shared.ocr_region.clone()
    };

    if apply_window {
        apply_ocr_region_window_state(app, &region)?;
    }

    if persist {
        save_ocr_region(app, &region)?;
    }

    emit_ocr_region_state(app, &region)?;

    if focus_window && region.visible {
        if let Some(window) = app.get_webview_window(OCR_REGION_WINDOW_LABEL) {
            let _ = window.set_focus();
        }
    }

    Ok(region)
}

pub(super) fn show_ocr_region_editor(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
    if is_ocr_region_drawing(app)? {
        let _ = cancel_ocr_region_drawing_inner(app);
    }
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let scale_factor = main_window.scale_factor().map_err(|err| err.to_string())?;
    let position = main_window.outer_position().map_err(|err| err.to_string())?;
    let size = main_window.inner_size().map_err(|err| err.to_string())?;

    let mut next_state = current_ocr_region_state(app)?;
    let should_initialize_geometry = next_state.x.is_none() || next_state.y.is_none();
    next_state.visible = true;
    if should_initialize_geometry {
        next_state.x = Some(f64::from(position.x) / scale_factor);
        next_state.y = Some(f64::from(position.y) / scale_factor);
        next_state.width = (f64::from(size.width) / scale_factor).max(MIN_OCR_REGION_WIDTH);
        next_state.height = (f64::from(size.height) / scale_factor).max(MIN_OCR_REGION_HEIGHT);
    }

    let region = commit_ocr_region_state(app, next_state, true, false, true)?;

    if let Some(window) = app.get_webview_window(OCR_REGION_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    Ok(region)
}

pub(super) fn hide_ocr_region_editor(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
    if is_ocr_region_drawing(app)? {
        finish_ocr_region_drawing_session(app)?;
        let mut state = current_ocr_region_state(app)?;
        state.visible = false;
        return commit_ocr_region_state(app, state, true, true, false);
    }

    let state = sync_ocr_region_state_from_window(app, Some(false))?;
    apply_ocr_region_window_state(app, &state)?;
    Ok(state)
}

pub(super) fn toggle_ocr_region_editor(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
    let visible = current_ocr_region_state(app)?.visible;
    if visible {
        hide_ocr_region_editor(app)
    } else {
        show_ocr_region_editor(app)
    }
}

#[derive(Debug)]
pub(super) struct StaticScreenSelectionSession {
    pub(super) screenshot_path: PathBuf,
    pub(super) viewport_width: f64,
    pub(super) viewport_height: f64,
    pub(super) overlay: OverlayStatePayload,
    pub(super) ctrl_pressed: bool,
    pub(super) ocr_region: OcrRegionStatePayload,
    pub(super) subtitle_explain: SubtitleExplainWindowStatePayload,
    pub(super) settings_was_visible: bool,
}

impl Drop for StaticScreenSelectionSession {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.screenshot_path);
    }
}

pub(super) fn start_ocr_region_drawing_inner(app: &AppHandle) -> Result<(), String> {
    if is_ocr_region_drawing(app)? {
        let _ = cancel_ocr_region_drawing_inner(app);
    }

    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let _ = sync_ocr_region_state_from_window(app, None);
    let (_, viewport_x, viewport_y, viewport_width, viewport_height) =
        capture_target_monitor(&main_window)?;
    let draw_request = OcrRegionDrawRequestPayload {
        mode: OcrRegionDrawMode::SetRegion,
        viewport_x,
        viewport_y,
        viewport_width,
        viewport_height,
        background_image_path: None,
    };
    present_ocr_region_draw_request(app, draw_request, None)
}

pub(super) fn start_static_screen_selection_inner(app: &AppHandle) -> Result<(), String> {
    if is_ocr_region_drawing(app)? {
        let _ = cancel_ocr_region_drawing_inner(app);
    }

    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let _ = sync_ocr_region_state_from_window(app, None);

    let subtitle_explain = if let Some(window) = app.get_webview_window(SUBTITLE_EXPLAIN_WINDOW_LABEL)
    {
        let visible = window.is_visible().map_err(|err| err.to_string())?;
        sync_subtitle_explain_window_state_from_window(app, Some(visible))?
    } else {
        current_subtitle_explain_window_state(app)?
    };
    let ocr_region = current_ocr_region_state(app)?;
    let (overlay, ctrl_pressed) = current_overlay_restore_state(app)?;
    let settings_was_visible = if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL)
    {
        window.is_visible().map_err(|err| err.to_string())?
    } else {
        false
    };
    let (monitor, viewport_x, viewport_y, viewport_width, viewport_height) =
        capture_target_monitor(&main_window)?;

    let capture_result = (|| -> Result<(PathBuf, String), String> {
        if let Some(window) = app.get_webview_window(OCR_REGION_WINDOW_LABEL) {
            if window.is_visible().map_err(|err| err.to_string())? {
                window.hide().map_err(|err| err.to_string())?;
            }
        }
        if let Some(window) = app.get_webview_window(SUBTITLE_EXPLAIN_WINDOW_LABEL) {
            if window.is_visible().map_err(|err| err.to_string())? {
                window.hide().map_err(|err| err.to_string())?;
            }
        }
        if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
            if window.is_visible().map_err(|err| err.to_string())? {
                window.hide().map_err(|err| err.to_string())?;
            }
        }
        if main_window.is_visible().map_err(|err| err.to_string())? {
            main_window.hide().map_err(|err| err.to_string())?;
        }

        thread::sleep(Duration::from_millis(
            STATIC_SELECTION_CAPTURE_SETTLE_MS,
        ));

        let image = super::subtitle::capture_monitor_image(&monitor)?;
        let screenshot_path = super::subtitle::save_static_screen_capture(image)?;
        let background_image_path = screenshot_path.to_string_lossy().to_string();
        Ok((screenshot_path, background_image_path))
    })();

    let (screenshot_path, background_image_path) = match capture_result {
        Ok(result) => result,
        Err(err) => {
            let _ = restore_static_selection_windows_snapshot(
                app,
                &overlay,
                ctrl_pressed,
                &ocr_region,
                &subtitle_explain,
                settings_was_visible,
            );
            return Err(err);
        }
    };

    let overlay_restore = overlay.clone();
    let ocr_region_restore = ocr_region.clone();
    let subtitle_explain_restore = subtitle_explain.clone();
    let session = StaticScreenSelectionSession {
        screenshot_path,
        viewport_width,
        viewport_height,
        overlay,
        ctrl_pressed,
        ocr_region,
        subtitle_explain,
        settings_was_visible,
    };
    let draw_request = OcrRegionDrawRequestPayload {
        mode: OcrRegionDrawMode::AnalyzeStatic,
        viewport_x,
        viewport_y,
        viewport_width,
        viewport_height,
        background_image_path: Some(background_image_path),
    };

    if let Err(err) = present_ocr_region_draw_request(app, draw_request, Some(session)) {
        let _ = restore_static_selection_windows_snapshot(
            app,
            &overlay_restore,
            ctrl_pressed,
            &ocr_region_restore,
            &subtitle_explain_restore,
            settings_was_visible,
        );
        return Err(err);
    }

    Ok(())
}

pub(super) fn cancel_ocr_region_drawing_inner(
    app: &AppHandle,
) -> Result<OcrRegionStatePayload, String> {
    let state = current_ocr_region_state(app)?;
    let static_session = take_static_screen_selection_session(app)?;
    finish_ocr_region_drawing_session(app)?;
    if let Some(session) = static_session.as_ref() {
        restore_static_screen_selection_windows(app, session)?;
        if session.subtitle_explain.visible {
            if let Some(window) = app.get_webview_window(SUBTITLE_EXPLAIN_WINDOW_LABEL) {
                let _ = window.set_focus();
            }
        }
    } else {
        apply_ocr_region_window_state(app, &state)?;
    }
    if state.visible && static_session.is_none() {
        if let Some(window) = app.get_webview_window(OCR_REGION_WINDOW_LABEL) {
            let _ = window.set_focus();
        }
    }
    Ok(state)
}

pub(super) fn open_subtitle_explain_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SUBTITLE_EXPLAIN_WINDOW_LABEL) {
        let _ = window.unminimize();

        if window.is_visible().map_err(|err| err.to_string())? {
            let _ = sync_subtitle_explain_window_state_from_window(app, Some(true));
            window.show().map_err(|err| err.to_string())?;
            window.set_focus().map_err(|err| err.to_string())?;
            return Ok(());
        }
    }

    let mut state = current_subtitle_explain_window_state(app)?;
    state.visible = true;
    commit_subtitle_explain_window_state(app, state, true, true, true)?;
    Ok(())
}

pub(super) fn toggle_subtitle_explain_window(app: &AppHandle) -> Result<(), String> {
    if current_subtitle_explain_window_state(app)?.visible {
        let state = sync_subtitle_explain_window_state_from_window(app, Some(false))?;
        commit_subtitle_explain_window_state(app, state, true, true, false)?;
        Ok(())
    } else {
        open_subtitle_explain_window(app)
    }
}

pub(super) fn sync_persisted_window_states(app: &AppHandle) {
    if let Err(err) = sync_overlay_state_from_window(app) {
        eprintln!("sync overlay state before exit failed: {err}");
    }

    if let Err(err) = sync_subtitle_explain_window_state_from_window(app, None) {
        eprintln!("sync subtitle explain window state before exit failed: {err}");
    }
}

pub(super) fn open_more_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        let _ = window.unminimize();
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("pages/settings.html".into()),
    )
    .title("更多设置")
    .inner_size(640.0, 760.0)
    .min_inner_size(600.0, 680.0)
    .resizable(true)
    .always_on_top(true)
    .center()
    .build()
    .map_err(|err| err.to_string())?;

    disable_window_transitions(&window)?;
    window.set_focus().map_err(|err| err.to_string())?;
    Ok(())
}

pub(super) fn open_more_settings_menu(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    open_more_settings_window(app)?;
    current_overlay_state(app)
}
