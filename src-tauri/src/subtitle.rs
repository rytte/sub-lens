use super::*;

fn resolve_sidecar_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("src-tauri").join(PYTHON_SIDECAR_SCRIPT));
        candidates.push(current_dir.join(PYTHON_SIDECAR_SCRIPT));
    }

    if let Ok(exe_dir) = std::env::current_exe().and_then(|path| {
        path.parent()
            .map(|value| value.to_path_buf())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing exe dir"))
    }) {
        candidates.push(exe_dir.join(PYTHON_SIDECAR_SCRIPT));
        candidates.push(exe_dir.join("..").join(PYTHON_SIDECAR_SCRIPT));
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(PYTHON_SIDECAR_SCRIPT));
    }

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| format!("找不到 Python sidecar 脚本：{PYTHON_SIDECAR_SCRIPT}"))
}

fn python_command_candidates() -> Vec<(String, Vec<String>)> {
    let mut candidates = Vec::new();

    if let Ok(value) = std::env::var("SUB_LENS_PYTHON") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            candidates.push((trimmed.to_string(), Vec::new()));
        }
    }

    candidates.push(("python".into(), Vec::new()));
    candidates.push(("py".into(), vec!["-3".into()]));
    candidates.push(("python3".into(), Vec::new()));
    candidates
}

fn spawn_stderr_logger(stderr: ChildStderr) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        loop {
            let mut buffer = Vec::new();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer);
                    let line = line.trim();
                    if !line.is_empty() {
                        eprintln!("subtitle sidecar: {line}");
                    }
                }
                Err(err) => {
                    eprintln!("subtitle sidecar stderr read failed: {err}");
                    break;
                }
            }
        }
    });
}

fn spawn_python_sidecar(app: &AppHandle) -> Result<PythonSidecarProcess, String> {
    let script_path = resolve_sidecar_script_path(app)?;
    let mut last_error = None;

    for (program, args) in python_command_candidates() {
        let mut command = Command::new(&program);
        command
            .args(&args)
            .arg("-X")
            .arg("utf8")
            .arg("-u")
            .arg(&script_path)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW);

        match command.spawn() {
            Ok(mut child) => {
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| "无法拿到 Python sidecar stdin".to_string())?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| "无法拿到 Python sidecar stdout".to_string())?;

                if let Some(stderr) = child.stderr.take() {
                    spawn_stderr_logger(stderr);
                }

                return Ok(PythonSidecarProcess {
                    child,
                    stdin,
                    stdout: BufReader::new(stdout),
                });
            }
            Err(err) => {
                last_error = Some(format!("{program} 启动失败：{err}"));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "无法启动 Python 解释器".into()))
}

fn read_process_line_lossy<R: Read>(reader: &mut BufReader<R>) -> Result<Option<String>, String> {
    let mut buffer = Vec::new();
    let read_size = reader
        .read_until(b'\n', &mut buffer)
        .map_err(|err| err.to_string())?;

    if read_size == 0 {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&buffer).trim().to_string()))
}

fn with_python_sidecar<R, T>(app: &AppHandle, request: &SidecarRequest<T>) -> Result<R, String>
where
    R: DeserializeOwned,
    T: Serialize,
{
    let sidecar_state = app.state::<SidecarState>();
    let mut process_guard = sidecar_state.0.lock().map_err(|err| err.to_string())?;

    if process_guard.is_none() {
        *process_guard = Some(spawn_python_sidecar(app)?);
    }

    let process = process_guard
        .as_mut()
        .ok_or_else(|| "Python sidecar 尚未初始化".to_string())?;

    let request_json = serde_json::to_string(request).map_err(|err| err.to_string())?;
    process
        .stdin
        .write_all(request_json.as_bytes())
        .and_then(|_| process.stdin.write_all(b"\n"))
        .and_then(|_| process.stdin.flush())
        .map_err(|err| err.to_string())?;

    let Some(response_line) = read_process_line_lossy(&mut process.stdout)? else {
        *process_guard = None;
        return Err("Python sidecar 已退出，请检查 Python 环境与依赖".into());
    };

    if response_line.is_empty() {
        return Err("Python sidecar 返回了空响应".into());
    }

    let response = serde_json::from_str::<SidecarResponse<R>>(&response_line)
        .map_err(|err| format!("sidecar 返回 JSON 解析失败：{err}；原始输出：{response_line}"))?;

    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "sidecar 返回未知错误".into()));
    }

    response
        .result
        .ok_or_else(|| "sidecar 缺少 result 字段".to_string())
}

fn image_path_to_data_url(image_path: &Path) -> Result<String, String> {
    let bytes = fs::read(image_path).map_err(|err| err.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

fn wait_for_ocr_runtime_ready(app: &AppHandle) -> Result<(), String> {
    super::app_window::ensure_ocr_runtime_window(app)?;

    let start = Instant::now();
    loop {
        let ready = {
            let state = app.state::<OcrRuntimeState>();
            let shared = state.0.lock().map_err(|err| err.to_string())?;
            shared.ready
        };

        if ready {
            return Ok(());
        }

        if start.elapsed() >= Duration::from_millis(OCR_RUNTIME_READY_TIMEOUT_MS) {
            return Err("OCR runtime 初始化超时，请稍后重试。".into());
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn remove_pending_ocr_runtime_request(app: &AppHandle, request_id: &str) -> Result<(), String> {
    let state = app.state::<OcrRuntimeState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    shared.pending.remove(request_id);
    Ok(())
}

fn run_tesseract_js_ocr(
    app: &AppHandle,
    image_path: &Path,
    settings: &OcrSettingsPayload,
) -> Result<String, String> {
    wait_for_ocr_runtime_ready(app)?;

    let request_id = {
        let state = app.state::<OcrRuntimeState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.next_request_id = shared.next_request_id.saturating_add(1);
        format!("ocr-runtime-{}", shared.next_request_id)
    };

    let image_data_url = image_path_to_data_url(image_path)?;
    let runtime_window = super::app_window::ensure_ocr_runtime_window(app)?;
    let (sender, receiver) = mpsc::channel();

    {
        let state = app.state::<OcrRuntimeState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.pending.insert(request_id.clone(), sender);
    }

    let payload = OcrRuntimeRequestPayload {
        request_id: request_id.clone(),
        engine: OcrEngine::Tesseract,
        language: settings.language,
        image_data_url,
        tesseract: OcrRuntimeTesseractPayload {
            languages: tesseract_langs(&settings.language)
                .into_iter()
                .map(str::to_string)
                .collect(),
            page_seg_mode: settings.profiles.tesseract.page_seg_mode,
        },
    };

    if let Err(err) = runtime_window.emit(EVENT_OCR_RUNTIME_REQUEST, payload) {
        let _ = remove_pending_ocr_runtime_request(app, &request_id);
        return Err(err.to_string());
    }

    match receiver.recv_timeout(Duration::from_millis(OCR_RUNTIME_REQUEST_TIMEOUT_MS)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = remove_pending_ocr_runtime_request(app, &request_id);
            Err("Tesseract.js 识别超时，请稍后重试或切换 OCR 引擎。".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = remove_pending_ocr_runtime_request(app, &request_id);
            Err("OCR runtime 连接已断开，请重试。".into())
        }
    }
}

pub(super) fn submit_ocr_runtime_response(
    app: &AppHandle,
    request_id: &str,
    response: Result<String, String>,
) -> Result<(), String> {
    let sender = {
        let state = app.state::<OcrRuntimeState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.pending.remove(request_id)
    };

    if let Some(sender) = sender {
        let _ = sender.send(response);
    }

    Ok(())
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn temp_capture_path_with_extension(extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sub-lens-capture-{}.{}",
        timestamp_millis(),
        extension.trim_start_matches('.')
    ))
}

fn temp_capture_path() -> PathBuf {
    temp_capture_path_with_extension("png")
}

fn temp_static_screen_capture_path() -> PathBuf {
    temp_capture_path_with_extension("bmp")
}

pub(super) fn capture_monitor_image(monitor: &Monitor) -> Result<image::RgbaImage, String> {
    let width = monitor.width().map_err(|err| err.to_string())?;
    let height = monitor.height().map_err(|err| err.to_string())?;
    monitor
        .capture_region(0, 0, width, height)
        .map_err(|err| err.to_string())
}

pub(super) fn save_static_screen_capture(image: image::RgbaImage) -> Result<PathBuf, String> {
    let path = temp_static_screen_capture_path();
    DynamicImage::ImageRgba8(image)
        .save_with_format(&path, ImageFormat::Bmp)
        .map_err(|err| err.to_string())?;
    Ok(path)
}

fn save_processed_ocr_capture(image: image::RgbaImage, engine: OcrEngine) -> Result<PathBuf, String> {
    let processed = preprocess_ocr_image(image, engine);
    let path = temp_capture_path();
    DynamicImage::ImageRgba8(processed)
        .save(&path)
        .map_err(|err| err.to_string())?;
    Ok(path)
}

fn crop_static_selection_image(
    session: &super::app_window::StaticScreenSelectionSession,
    selection: &OcrSelectionPayload,
    engine: OcrEngine,
) -> Result<PathBuf, String> {
    let screenshot = image::open(&session.screenshot_path)
        .map_err(|err| format!("读取静态屏幕截图失败：{err}"))?;
    let image_width = screenshot.width();
    let image_height = screenshot.height();
    if image_width == 0 || image_height == 0 {
        return Err("静态屏幕截图为空，无法裁剪。".into());
    }

    let scale_x = f64::from(image_width) / session.viewport_width.max(1.0);
    let scale_y = f64::from(image_height) / session.viewport_height.max(1.0);
    let left = (selection.x.max(0.0) * scale_x).floor() as u32;
    let top = (selection.y.max(0.0) * scale_y).floor() as u32;
    let right = ((selection.x + selection.width).max(0.0) * scale_x).ceil() as u32;
    let bottom = ((selection.y + selection.height).max(0.0) * scale_y).ceil() as u32;

    let clamped_left = left.min(image_width.saturating_sub(1));
    let clamped_top = top.min(image_height.saturating_sub(1));
    let clamped_right = right.max(clamped_left + 1).min(image_width);
    let clamped_bottom = bottom.max(clamped_top + 1).min(image_height);
    let cropped = screenshot
        .crop_imm(
            clamped_left,
            clamped_top,
            clamped_right.saturating_sub(clamped_left).max(1),
            clamped_bottom.saturating_sub(clamped_top).max(1),
        )
        .to_rgba8();

    save_processed_ocr_capture(cropped, engine)
}

fn capture_ocr_region_image(app: &AppHandle, engine: OcrEngine) -> Result<PathBuf, String> {
    let region = current_ocr_region_state(app)?;
    let window = super::app_window::ensure_ocr_region_window(app)?;

    let was_effectively_visible = effective_ocr_region_visibility(&region);
    if was_effectively_visible {
        window.hide().map_err(|err| err.to_string())?;
        thread::sleep(Duration::from_millis(OCR_CAPTURE_SETTLE_MS));
    }

    let result = (|| -> Result<PathBuf, String> {
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
        save_processed_ocr_capture(image, engine)
    })();

    if region.visible {
        let _ = super::app_window::hide_ocr_region_editor(app);
    } else if was_effectively_visible {
        let _ = super::app_window::apply_ocr_region_window_state(app, &region);
    }

    result
}

fn preprocess_ocr_image(image: image::RgbaImage, engine: OcrEngine) -> image::RgbaImage {
    match engine {
        OcrEngine::WindowsOcr => preprocess_windows_ocr_image(image),
        OcrEngine::Tesseract
        | OcrEngine::EasyOcr
        | OcrEngine::RapidOcr
        | OcrEngine::PaddleOcr => preprocess_default_ocr_image(image),
    }
}

fn preprocess_default_ocr_image(image: image::RgbaImage) -> image::RgbaImage {
    let grayscale = DynamicImage::ImageRgba8(image).grayscale();
    grayscale.adjust_contrast(24.0).to_rgba8()
}

fn preprocess_windows_ocr_image(image: image::RgbaImage) -> image::RgbaImage {
    let source = DynamicImage::ImageRgba8(image);
    let width = source.width().max(1);
    let height = source.height().max(1);

    let scale = if height < 180 { 3 } else { 2 };
    let target_width = width.saturating_mul(scale);
    let target_height = height.saturating_mul(scale);
    let resized = resize(
        &source.to_rgba8(),
        target_width,
        target_height,
        FilterType::CatmullRom,
    );

    DynamicImage::ImageRgba8(resized)
        .adjust_contrast(8.0)
        .brighten(6)
        .to_rgba8()
}

#[derive(Debug, Default)]
struct RealtimeSubtitleTracker {
    candidate_text: Option<String>,
    candidate_source_text: Option<String>,
    stable_count: u32,
    last_emitted_text: Option<String>,
    blank_since_ms: Option<u128>,
}

impl RealtimeSubtitleTracker {
    fn reset_candidate(&mut self) {
        self.candidate_text = None;
        self.candidate_source_text = None;
        self.stable_count = 0;
    }

    fn reset_all(&mut self) {
        self.reset_candidate();
        self.last_emitted_text = None;
        self.blank_since_ms = None;
    }
}

fn normalize_subtitle_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn subtitle_similarity(left: &str, right: &str) -> f64 {
    if left == right {
        return 1.0;
    }

    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    if left_chars.is_empty() || right_chars.is_empty() {
        return 0.0;
    }

    let right_len = right_chars.len();
    let max_len = left_chars.len().max(right_len);
    let mut previous: Vec<usize> = (0..=right_len).collect();
    let mut current = vec![0; right_len + 1];

    for (left_index, left_char) in left_chars.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    let distance = previous[right_len];
    1.0 - distance as f64 / max_len as f64
}

fn run_python_sidecar_ocr_on_image(
    app: &AppHandle,
    image_path: &Path,
    settings: &OcrSettingsPayload,
    allow_empty: bool,
) -> Result<String, String> {
    let image_path_string = image_path.to_string_lossy().to_string();
    let request = SidecarRequest {
        command: "ocr_subtitle",
        payload: OcrSubtitleRequest {
            image_path: &image_path_string,
            engine: settings.engine,
            language: settings.language,
            allow_empty,
        },
    };

    let response = with_python_sidecar::<OcrSubtitleResultPayload, _>(app, &request)?;
    Ok(response.source_text)
}

fn explain_subtitle_text(
    app: &AppHandle,
    settings: &SubtitleExplainSettingsPayload,
    source_text: &str,
) -> Result<SubtitleAnalysisResultPayload, String> {
    let request = SidecarRequest {
        command: "explain_subtitle",
        payload: ExplainSubtitleRequest {
            source_text,
            settings,
        },
    };

    with_python_sidecar::<SubtitleAnalysisResultPayload, _>(app, &request)
}

fn recognize_subtitle_source_text_from_image_path(
    app: &AppHandle,
    settings: &SubtitleExplainSettingsPayload,
    image_path: &Path,
    allow_empty: bool,
) -> Result<String, String> {
    recognize_text(
        image_path,
        allow_empty,
        &settings.ocr_settings,
        |engine, image_path, allow_empty| match engine {
            OcrEngine::Tesseract => run_tesseract_js_ocr(app, image_path, &settings.ocr_settings),
            OcrEngine::EasyOcr | OcrEngine::RapidOcr | OcrEngine::PaddleOcr => {
                run_python_sidecar_ocr_on_image(
                    app,
                    image_path,
                    &settings.ocr_settings,
                    allow_empty,
                )
            }
            OcrEngine::WindowsOcr => Err("当前 OCR 引擎不应走 JS/sidecar 分支。".into()),
        },
    )
}

fn capture_subtitle_source_text(
    app: &AppHandle,
    settings: &SubtitleExplainSettingsPayload,
    allow_empty: bool,
) -> Result<String, String> {
    let image_path = capture_ocr_region_image(app, settings.ocr_settings.engine)?;
    let result =
        recognize_subtitle_source_text_from_image_path(app, settings, &image_path, allow_empty);
    if result.is_ok() {
        let _ = fs::remove_file(&image_path);
    }
    result
}

fn handle_realtime_blank(
    app: &AppHandle,
    settings: &SubtitleExplainSettingsPayload,
    tracker: &mut RealtimeSubtitleTracker,
) -> Result<(), String> {
    tracker.reset_candidate();

    let blank_since_ms = tracker.blank_since_ms.get_or_insert_with(timestamp_millis);
    let blank_elapsed_ms = timestamp_millis().saturating_sub(*blank_since_ms);
    if blank_elapsed_ms >= u128::from(settings.blank_clear_ms) {
        tracker.last_emitted_text = None;
        clear_subtitle_result(app)?;
        set_subtitle_status(
            app,
            false,
            "realtime_waiting",
            "未检测到字幕，等待新字幕。",
            None,
        )?;
    } else {
        set_subtitle_status(
            app,
            false,
            "realtime_hold",
            "未检测到字幕，暂时保留上一条结果。",
            None,
        )?;
    }

    Ok(())
}

fn run_realtime_subtitle_iteration(
    app: &AppHandle,
    settings: &SubtitleExplainSettingsPayload,
    tracker: &mut RealtimeSubtitleTracker,
) -> Result<(), String> {
    let source_text = capture_subtitle_source_text(app, settings, true)?;
    if source_text.is_empty() {
        return handle_realtime_blank(app, settings, tracker);
    }

    tracker.blank_since_ms = None;
    let normalized_text = normalize_subtitle_text(&source_text);
    if normalized_text.is_empty() {
        return handle_realtime_blank(app, settings, tracker);
    }

    let is_same_candidate = tracker
        .candidate_text
        .as_deref()
        .map(|candidate| {
            subtitle_similarity(candidate, &normalized_text) >= settings.similarity_threshold
        })
        .unwrap_or(false);

    if is_same_candidate {
        tracker.stable_count = tracker.stable_count.saturating_add(1);
        tracker.candidate_source_text = Some(source_text);
    } else {
        tracker.candidate_text = Some(normalized_text.clone());
        tracker.candidate_source_text = Some(source_text);
        tracker.stable_count = 1;
    }

    let stable_target = settings.stable_frames.max(1);
    if tracker.stable_count < stable_target {
        let message = format!(
            "实时识别中，等待字幕稳定（{}/{}）",
            tracker.stable_count, stable_target
        );
        set_subtitle_status(app, false, "realtime_pending", &message, None)?;
        return Ok(());
    }

    if tracker.last_emitted_text.as_deref() == Some(normalized_text.as_str()) {
        set_subtitle_status(
            app,
            false,
            "realtime_listening",
            "实时监听中，等待新字幕。",
            None,
        )?;
        return Ok(());
    }

    if let Some(existing_result) = current_subtitle_result(app)? {
        let existing_text = normalize_subtitle_text(&existing_result.source_text);
        if !existing_text.is_empty()
            && subtitle_similarity(&existing_text, &normalized_text)
                >= settings.similarity_threshold
        {
            tracker.last_emitted_text = Some(normalized_text);
            set_subtitle_status(
                app,
                false,
                "realtime_listening",
                "实时监听中，等待新字幕。",
                None,
            )?;
            return Ok(());
        }
    }

    let Some(stable_source_text) = tracker.candidate_source_text.as_deref() else {
        set_subtitle_status(
            app,
            false,
            "realtime_listening",
            "实时监听中，等待新字幕。",
            None,
        )?;
        return Ok(());
    };

    super::app_window::open_subtitle_explain_window(app)?;
    set_subtitle_status(app, true, "llm", "检测到稳定字幕，正在解析...", None)?;
    let result = explain_subtitle_text(app, settings, stable_source_text)?;
    let result = set_subtitle_result(app, result)?;
    tracker.last_emitted_text = Some(normalize_subtitle_text(&result.source_text));
    set_subtitle_status(app, false, "done", "实时字幕解析完成。", None)?;
    Ok(())
}

pub(super) fn run_subtitle_analysis_blocking(
    app: &AppHandle,
) -> Result<SubtitleAnalysisResultPayload, String> {
    let _job_guard = try_acquire_subtitle_job(app)?
        .ok_or_else(|| "已有一条字幕正在解析中，请稍后再试。".to_string())?;
    let settings = current_subtitle_settings(app)?;

    super::app_window::open_subtitle_explain_window(app)?;
    if current_ocr_region_state(app)?.visible {
        set_subtitle_status(
            app,
            true,
            "capture",
            "检测到 OCR 编辑框仍在显示，已自动关闭后再截图...",
            None,
        )?;
    } else {
        set_subtitle_status(app, true, "capture", "正在抓取 OCR 区域并识别文本...", None)?;
    }

    let source_text = match capture_subtitle_source_text(app, &settings, false) {
        Ok(source_text) => source_text,
        Err(err) => {
            let _ = set_subtitle_status(app, false, "error", "获取字幕失败。", Some(err.clone()));
            return Err(err);
        }
    };

    set_subtitle_status(app, true, "llm", "正在解析字幕...", None)?;

    match explain_subtitle_text(app, &settings, &source_text) {
        Ok(result) => {
            let result = set_subtitle_result(app, result)?;
            set_subtitle_status(app, false, "done", "字幕解析完成。", None)?;
            Ok(result)
        }
        Err(err) => {
            let _ = set_subtitle_status(app, false, "error", "字幕解析失败。", Some(err.clone()));
            Err(err)
        }
    }
}

pub(super) fn run_static_screen_selection_analysis_blocking(
    app: &AppHandle,
    selection: OcrSelectionPayload,
) -> Result<SubtitleAnalysisResultPayload, String> {
    let static_session = take_static_screen_selection_session(app)?
        .ok_or_else(|| "当前没有待处理的截图翻译会话。".to_string())?;
    finish_ocr_region_drawing_session(app)?;
    super::app_window::restore_static_screen_selection_windows(app, &static_session)?;

    let _job_guard = match try_acquire_subtitle_job(app)? {
        Some(guard) => guard,
        None => {
            let message = "已有一条字幕正在解析中，请稍后再试。".to_string();
            let _ = set_subtitle_status(
                app,
                false,
                "error",
                "截图翻译提交失败。",
                Some(message.clone()),
            );
            return Err(message);
        }
    };
    let settings = current_subtitle_settings(app)?;

    super::app_window::open_subtitle_explain_window(app)?;
    set_subtitle_status(app, true, "capture", "正在识别截图区域文本...", None)?;

    let image_path =
        match crop_static_selection_image(&static_session, &selection, settings.ocr_settings.engine)
        {
            Ok(image_path) => image_path,
            Err(err) => {
                let _ = set_subtitle_status(
                    app,
                    false,
                    "error",
                    "裁剪静态选区失败。",
                    Some(err.clone()),
                );
                return Err(err);
            }
        };
    let source_text =
        match recognize_subtitle_source_text_from_image_path(app, &settings, &image_path, false) {
            Ok(source_text) => {
                let _ = fs::remove_file(&image_path);
                source_text
            }
            Err(err) => {
                let _ = fs::remove_file(&image_path);
                let _ = set_subtitle_status(
                    app,
                    false,
                    "error",
                    "识别截图区域失败。",
                    Some(err.clone()),
                );
                return Err(err);
            }
        };

    set_subtitle_status(app, true, "llm", "正在解析截图内容...", None)?;

    match explain_subtitle_text(app, &settings, &source_text) {
        Ok(result) => {
            let result = set_subtitle_result(app, result)?;
            set_subtitle_status(app, false, "done", "截图翻译完成。", None)?;
            Ok(result)
        }
        Err(err) => {
            let _ = set_subtitle_status(app, false, "error", "截图翻译失败。", Some(err.clone()));
            Err(err)
        }
    }
}

pub(super) fn start_manual_subtitle_analysis(app: AppHandle) {
    thread::spawn(move || {
        if let Err(err) = run_subtitle_analysis_blocking(&app) {
            eprintln!("subtitle analysis failed: {err}");
        }
    });
}

pub(super) fn start_realtime_subtitle_loop(app: AppHandle) {
    thread::spawn(move || {
        let mut tracker = RealtimeSubtitleTracker::default();

        loop {
            let settings = match current_subtitle_settings(&app) {
                Ok(settings) => settings,
                Err(err) => {
                    eprintln!("load subtitle settings failed: {err}");
                    thread::sleep(Duration::from_millis(
                        SubtitleExplainSettingsPayload::default().ocr_interval_ms as u64,
                    ));
                    continue;
                }
            };
            let interval_ms = settings.ocr_interval_ms.max(250) as u64;

            if !settings.realtime_enabled {
                tracker.reset_all();
                if let Ok(status) = current_subtitle_status(&app) {
                    if status.stage.starts_with("realtime") {
                        let _ = set_subtitle_status(&app, false, "idle", "等待手动解析。", None);
                    }
                }
                thread::sleep(Duration::from_millis(interval_ms));
                continue;
            }

            match current_ocr_region_state(&app) {
                Ok(region) if region.visible => {
                    tracker.reset_candidate();
                    tracker.blank_since_ms = None;
                    let _ = set_subtitle_status(
                        &app,
                        false,
                        "realtime_paused",
                        "OCR 区域编辑中，实时解析已暂停。",
                        None,
                    );
                    thread::sleep(Duration::from_millis(interval_ms));
                    continue;
                }
                Ok(_) => {}
                Err(err) => {
                    eprintln!("load ocr region failed: {err}");
                    thread::sleep(Duration::from_millis(interval_ms));
                    continue;
                }
            }

            let _job_guard = match try_acquire_subtitle_job(&app) {
                Ok(Some(guard)) => guard,
                Ok(None) => {
                    thread::sleep(Duration::from_millis(interval_ms));
                    continue;
                }
                Err(err) => {
                    eprintln!("acquire subtitle job failed: {err}");
                    thread::sleep(Duration::from_millis(interval_ms));
                    continue;
                }
            };

            if let Err(err) = run_realtime_subtitle_iteration(&app, &settings, &mut tracker) {
                tracker.reset_candidate();
                tracker.blank_since_ms = None;
                let _ = set_subtitle_status(
                    &app,
                    false,
                    "error",
                    "实时字幕解析失败。",
                    Some(err.clone()),
                );
                eprintln!("realtime subtitle analysis failed: {err}");
            }

            thread::sleep(Duration::from_millis(interval_ms));
        }
    });
}
