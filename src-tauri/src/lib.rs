mod ocr;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{
    imageops::{resize, FilterType},
    DynamicImage, ImageFormat,
};
use ocr::{
    inspect_engines, recognize_text, sanitize_ocr_settings, tesseract_langs, OcrEngine,
    OcrEngineAvailabilityPayload, OcrLanguage, OcrSettingsPayload,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tauri_plugin_store::StoreExt;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use xcap::Monitor;

const EVENT_STATE_CHANGED: &str = "overlay://state";
const EVENT_PIXEL_FRAME: &str = "overlay://pixel-frame";
const EVENT_OCR_REGION_STATE_CHANGED: &str = "ocr-region://state";
const EVENT_OCR_REGION_DRAW_REQUEST: &str = "ocr-region://draw-request";
const EVENT_OCR_RUNTIME_REQUEST: &str = "ocr-runtime://request";
const EVENT_SUBTITLE_STATUS_CHANGED: &str = "subtitle://status";
const EVENT_SUBTITLE_RESULT_CHANGED: &str = "subtitle://result";
const STORE_PATH: &str = "overlay-settings.json";
const STORE_KEY_OVERLAY: &str = "overlay";
const STORE_KEY_OCR_REGION: &str = "ocrRegion";
const STORE_KEY_SUBTITLE_EXPLAIN: &str = "subtitleExplain";
const STORE_KEY_SUBTITLE_EXPLAIN_WINDOW: &str = "subtitleExplainWindow";
const TRAY_ID: &str = "overlay-tray";
const SETTINGS_WINDOW_LABEL: &str = "settings";
const OCR_REGION_WINDOW_LABEL: &str = "ocr-region";
const OCR_RUNTIME_WINDOW_LABEL: &str = "ocr-runtime";
const SUBTITLE_EXPLAIN_WINDOW_LABEL: &str = "subtitle-explain";
const DEFAULT_WIDTH: f64 = 720.0;
const DEFAULT_HEIGHT: f64 = 132.0;
const MIN_WIDTH: f64 = 280.0;
const MIN_HEIGHT: f64 = 36.0;
const DEFAULT_OCR_REGION_WIDTH: f64 = 720.0;
const DEFAULT_OCR_REGION_HEIGHT: f64 = 156.0;
const MIN_OCR_REGION_WIDTH: f64 = 240.0;
const MIN_OCR_REGION_HEIGHT: f64 = 20.0;
const DEFAULT_SUBTITLE_EXPLAIN_WINDOW_WIDTH: f64 = 520.0;
const DEFAULT_SUBTITLE_EXPLAIN_WINDOW_HEIGHT: f64 = 360.0;
const MIN_SUBTITLE_EXPLAIN_WINDOW_WIDTH: f64 = 440.0;
const MIN_SUBTITLE_EXPLAIN_WINDOW_HEIGHT: f64 = 300.0;
const CTRL_HIDE_POLL_INTERVAL_MS: u64 = 16;
const OCR_CAPTURE_SETTLE_MS: u64 = 320;
const STATIC_SELECTION_CAPTURE_SETTLE_MS: u64 = 160;
const OCR_RUNTIME_READY_TIMEOUT_MS: u64 = 10_000;
const OCR_RUNTIME_REQUEST_TIMEOUT_MS: u64 = 120_000;
const PYTHON_SIDECAR_SCRIPT: &str = "sidecars/subtitle_sidecar.py";
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const DEFAULT_SHORTCUT_TOGGLE_OVERLAY_VISIBILITY: &str = "Ctrl+Alt+H";
const DEFAULT_SHORTCUT_TOGGLE_OVERLAY_LOCK: &str = "Ctrl+Alt+Space";
const DEFAULT_SHORTCUT_EDIT_OCR_REGION: &str = "Ctrl+Shift+E";
const DEFAULT_SHORTCUT_START_STATIC_SCREEN_SELECTION: &str = "Ctrl+Alt+J";
const DEFAULT_SHORTCUT_ANALYZE_CURRENT_SUBTITLE: &str = "Ctrl+Shift+J";

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
const MENU_OPACITY_50: &str = "opacity_50";
const MENU_OPACITY_60: &str = "opacity_60";
const MENU_OPACITY_70: &str = "opacity_70";
const MENU_OPACITY_80: &str = "opacity_80";
const MENU_OPACITY_90: &str = "opacity_90";
const MENU_OPACITY_100: &str = "opacity_100";
const MENU_APPEARANCE_PIXELATE: &str = "appearance_pixelate";
const MENU_APPEARANCE_SOLID: &str = "appearance_solid";
const MENU_COLOR_BLACK: &str = "color_black";
const MENU_COLOR_CHARCOAL: &str = "color_charcoal";
const MENU_COLOR_SILVER: &str = "color_silver";
const MENU_COLOR_WHITE: &str = "color_white";
const MENU_RESET_POSITION: &str = "reset_position";
const MENU_TOGGLE_ON_TOP: &str = "toggle_on_top";
const MENU_EDIT_OCR_REGION: &str = "edit_ocr_region";
const MENU_ANALYZE_CURRENT_SUBTITLE: &str = "analyze_current_subtitle";
const MENU_SHOW_SUBTITLE_EXPLAIN: &str = "show_subtitle_explain";
const MENU_OPEN_MORE_SETTINGS: &str = "open_more_settings";
const MENU_QUIT: &str = "quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ConfiguredShortcutAction {
    ToggleOverlayVisibility,
    ToggleOverlayLock,
    EditOcrRegion,
    StartStaticScreenSelection,
    AnalyzeCurrentSubtitle,
}

impl ConfiguredShortcutAction {
    fn label(self) -> &'static str {
        match self {
            Self::ToggleOverlayVisibility => "显示 / 隐藏遮挡条",
            Self::ToggleOverlayLock => "锁定 / 解锁遮挡条",
            Self::EditOcrRegion => "编辑 OCR 区域",
            Self::StartStaticScreenSelection => "划词翻译",
            Self::AnalyzeCurrentSubtitle => "解析当前字幕",
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SubtitleProvider {
    OpenAiResponses,
    Ollama,
}

impl Default for SubtitleProvider {
    fn default() -> Self {
        Self::OpenAiResponses
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SubtitleExplainMode {
    Phrase,
    Word,
}

impl Default for SubtitleExplainMode {
    fn default() -> Self {
        Self::Word
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum TrayClickAction {
    Overlay,
    SubtitleExplain,
}

impl Default for TrayClickAction {
    fn default() -> Self {
        Self::Overlay
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct ProviderConnectionSettingsPayload {
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct SubtitleProviderProfilesPayload {
    open_ai_responses: ProviderConnectionSettingsPayload,
    ollama: ProviderConnectionSettingsPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct ShortcutSettingsPayload {
    toggle_overlay_visibility: String,
    toggle_overlay_lock: String,
    edit_ocr_region: String,
    start_static_screen_selection: String,
    analyze_current_subtitle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct SubtitleExplainSettingsPayload {
    realtime_enabled: bool,
    provider: SubtitleProvider,
    explain_mode: SubtitleExplainMode,
    tray_click_action: TrayClickAction,
    base_url: String,
    api_key: String,
    model: String,
    provider_profiles: SubtitleProviderProfilesPayload,
    ocr_settings: OcrSettingsPayload,
    ocr_interval_ms: u32,
    stable_frames: u32,
    similarity_threshold: f64,
    blank_clear_ms: u32,
    shortcut_settings: ShortcutSettingsPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct OcrRegionStatePayload {
    visible: bool,
    show_border_when_idle: bool,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OcrRegionDrawMode {
    SetRegion,
    AnalyzeStatic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrRegionDrawRequestPayload {
    mode: OcrRegionDrawMode,
    viewport_x: f64,
    viewport_y: f64,
    viewport_width: f64,
    viewport_height: f64,
    background_image_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OcrSelectionPayload {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct SubtitleExplainWindowStatePayload {
    visible: bool,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SubtitlePhrasePayload {
    source: String,
    explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SubtitleAnalysisResultPayload {
    source_text: String,
    translation: String,
    phrases: Vec<SubtitlePhrasePayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SubtitleStatusPayload {
    analyzing: bool,
    stage: String,
    message: String,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SidecarRequest<'a, T> {
    command: &'a str,
    payload: T,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrSubtitleRequest<'a> {
    image_path: &'a str,
    engine: OcrEngine,
    language: OcrLanguage,
    allow_empty: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExplainSubtitleRequest<'a> {
    source_text: &'a str,
    settings: &'a SubtitleExplainSettingsPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OcrSubtitleResultPayload {
    source_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrRuntimeRequestPayload {
    request_id: String,
    engine: OcrEngine,
    language: OcrLanguage,
    image_data_url: String,
    tesseract: OcrRuntimeTesseractPayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrRuntimeTesseractPayload {
    languages: Vec<String>,
    page_seg_mode: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SidecarResponse<T> {
    ok: bool,
    result: Option<T>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelItem {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelItem>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelItem {
    name: String,
    model: Option<String>,
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

impl Default for SubtitleExplainSettingsPayload {
    fn default() -> Self {
        let provider = SubtitleProvider::default();
        let provider_profiles = SubtitleProviderProfilesPayload::default();
        let active_profile = provider_profile(&provider_profiles, &provider).clone();
        Self {
            realtime_enabled: false,
            base_url: active_profile.base_url,
            api_key: active_profile.api_key,
            model: active_profile.model,
            provider,
            explain_mode: SubtitleExplainMode::default(),
            tray_click_action: TrayClickAction::default(),
            provider_profiles,
            ocr_settings: OcrSettingsPayload::default(),
            ocr_interval_ms: 300,
            stable_frames: 2,
            similarity_threshold: 0.88,
            blank_clear_ms: 600,
            shortcut_settings: ShortcutSettingsPayload::default(),
        }
    }
}

impl Default for ProviderConnectionSettingsPayload {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

impl Default for SubtitleProviderProfilesPayload {
    fn default() -> Self {
        Self {
            open_ai_responses: default_provider_connection_settings(
                &SubtitleProvider::OpenAiResponses,
            ),
            ollama: default_provider_connection_settings(&SubtitleProvider::Ollama),
        }
    }
}

impl Default for ShortcutSettingsPayload {
    fn default() -> Self {
        Self {
            toggle_overlay_visibility: DEFAULT_SHORTCUT_TOGGLE_OVERLAY_VISIBILITY.into(),
            toggle_overlay_lock: DEFAULT_SHORTCUT_TOGGLE_OVERLAY_LOCK.into(),
            edit_ocr_region: DEFAULT_SHORTCUT_EDIT_OCR_REGION.into(),
            start_static_screen_selection: DEFAULT_SHORTCUT_START_STATIC_SCREEN_SELECTION.into(),
            analyze_current_subtitle: DEFAULT_SHORTCUT_ANALYZE_CURRENT_SUBTITLE.into(),
        }
    }
}

impl Default for OcrRegionStatePayload {
    fn default() -> Self {
        Self {
            visible: false,
            show_border_when_idle: false,
            width: DEFAULT_OCR_REGION_WIDTH,
            height: DEFAULT_OCR_REGION_HEIGHT,
            x: None,
            y: None,
        }
    }
}

impl Default for SubtitleExplainWindowStatePayload {
    fn default() -> Self {
        Self {
            visible: false,
            width: DEFAULT_SUBTITLE_EXPLAIN_WINDOW_WIDTH,
            height: DEFAULT_SUBTITLE_EXPLAIN_WINDOW_HEIGHT,
            x: None,
            y: None,
        }
    }
}

impl Default for SubtitleStatusPayload {
    fn default() -> Self {
        Self {
            analyzing: false,
            stage: "idle".into(),
            message: "等待手动解析。".into(),
            error: None,
        }
    }
}

#[derive(Debug)]
struct SharedState {
    overlay: OverlayStatePayload,
    ocr_region: OcrRegionStatePayload,
    ocr_region_drawing: bool,
    ocr_region_draw_request: Option<OcrRegionDrawRequestPayload>,
    static_screen_selection: Option<StaticScreenSelectionSession>,
    subtitle_settings: SubtitleExplainSettingsPayload,
    subtitle_explain_window: SubtitleExplainWindowStatePayload,
    subtitle_status: SubtitleStatusPayload,
    latest_subtitle_result: Option<SubtitleAnalysisResultPayload>,
    subtitle_job_running: bool,
    overlay_interaction_until_ms: u128,
    ctrl_pressed: bool,
}

impl SharedState {
    fn new(
        overlay: OverlayStatePayload,
        ocr_region: OcrRegionStatePayload,
        subtitle_settings: SubtitleExplainSettingsPayload,
        subtitle_explain_window: SubtitleExplainWindowStatePayload,
        ctrl_pressed: bool,
    ) -> Self {
        Self {
            overlay,
            ocr_region,
            ocr_region_drawing: false,
            ocr_region_draw_request: None,
            static_screen_selection: None,
            subtitle_settings,
            subtitle_explain_window,
            subtitle_status: SubtitleStatusPayload::default(),
            latest_subtitle_result: None,
            subtitle_job_running: false,
            overlay_interaction_until_ms: 0,
            ctrl_pressed,
        }
    }
}

struct AppState(Mutex<SharedState>);

struct SubtitleJobGuard {
    app: AppHandle,
}

impl Drop for SubtitleJobGuard {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        let lock_result = state.0.lock();
        if let Ok(mut shared) = lock_result {
            shared.subtitle_job_running = false;
        }
    }
}

struct PythonSidecarProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for PythonSidecarProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct SidecarState(Mutex<Option<PythonSidecarProcess>>);

#[derive(Default)]
struct OcrRuntimeShared {
    ready: bool,
    next_request_id: u64,
    pending: HashMap<String, mpsc::Sender<Result<String, String>>>,
}

struct OcrRuntimeState(Mutex<OcrRuntimeShared>);

#[derive(Debug, Clone)]
struct ParsedShortcutBinding {
    action: ConfiguredShortcutAction,
    shortcut: Shortcut,
    accelerator: String,
}

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn default_base_url(provider: &SubtitleProvider) -> &'static str {
    match provider {
        SubtitleProvider::OpenAiResponses => "https://api.openai.com/v1",
        SubtitleProvider::Ollama => "http://127.0.0.1:11434",
    }
}

fn default_model(provider: &SubtitleProvider) -> &'static str {
    match provider {
        SubtitleProvider::OpenAiResponses => "gpt-4.1-mini",
        SubtitleProvider::Ollama => "qwen2.5:7b",
    }
}

fn default_provider_connection_settings(
    provider: &SubtitleProvider,
) -> ProviderConnectionSettingsPayload {
    ProviderConnectionSettingsPayload {
        base_url: default_base_url(provider).into(),
        api_key: String::new(),
        model: default_model(provider).into(),
    }
}

fn provider_profile<'a>(
    profiles: &'a SubtitleProviderProfilesPayload,
    provider: &SubtitleProvider,
) -> &'a ProviderConnectionSettingsPayload {
    match provider {
        SubtitleProvider::OpenAiResponses => &profiles.open_ai_responses,
        SubtitleProvider::Ollama => &profiles.ollama,
    }
}

fn provider_profile_mut<'a>(
    profiles: &'a mut SubtitleProviderProfilesPayload,
    provider: &SubtitleProvider,
) -> &'a mut ProviderConnectionSettingsPayload {
    match provider {
        SubtitleProvider::OpenAiResponses => &mut profiles.open_ai_responses,
        SubtitleProvider::Ollama => &mut profiles.ollama,
    }
}

fn sanitize_provider_connection_settings(
    mut settings: ProviderConnectionSettingsPayload,
    provider: &SubtitleProvider,
) -> ProviderConnectionSettingsPayload {
    settings.base_url = normalize_base_url(&settings.base_url, provider);
    settings.api_key = settings.api_key.trim().to_string();
    settings.model = if settings.model.trim().is_empty() {
        default_model(provider).into()
    } else {
        settings.model.trim().to_string()
    };
    settings
}

fn sanitize_provider_profiles(
    mut profiles: SubtitleProviderProfilesPayload,
) -> SubtitleProviderProfilesPayload {
    profiles.open_ai_responses = sanitize_provider_connection_settings(
        profiles.open_ai_responses,
        &SubtitleProvider::OpenAiResponses,
    );
    profiles.ollama =
        sanitize_provider_connection_settings(profiles.ollama, &SubtitleProvider::Ollama);
    profiles
}

fn sanitize_shortcut_settings(mut settings: ShortcutSettingsPayload) -> ShortcutSettingsPayload {
    settings.toggle_overlay_visibility = settings.toggle_overlay_visibility.trim().to_string();
    settings.toggle_overlay_lock = settings.toggle_overlay_lock.trim().to_string();
    settings.edit_ocr_region = settings.edit_ocr_region.trim().to_string();
    settings.start_static_screen_selection =
        settings.start_static_screen_selection.trim().to_string();
    settings.analyze_current_subtitle = settings.analyze_current_subtitle.trim().to_string();
    settings
}

fn normalize_base_url(base_url: &str, provider: &SubtitleProvider) -> String {
    let trimmed = base_url.trim();
    let effective = if trimmed.is_empty() {
        default_base_url(provider)
    } else {
        trimmed
    };
    effective.trim_end_matches('/').to_string()
}

fn join_base_url(base_url: &str, suffix: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), suffix)
}

fn dedup_and_sort_models(models: Vec<String>) -> Vec<String> {
    let mut normalized = models
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    normalized.sort_by_cached_key(|value| value.to_ascii_lowercase());
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    normalized
}

fn summarize_http_error(body: &str) -> String {
    let condensed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if condensed.is_empty() {
        return "接口未返回错误详情。".into();
    }

    let mut summary = condensed.chars().take(180).collect::<String>();
    if condensed.chars().count() > 180 {
        summary.push_str("...");
    }
    summary
}

fn build_model_fetch_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("创建模型拉取客户端失败：{err}"))
}

async fn fetch_openai_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    if api_key.trim().is_empty() {
        return Err("OpenAI Responses 拉取模型列表需要填写 API Key。".into());
    }

    let client = build_model_fetch_client()?;
    let response = client
        .get(join_base_url(base_url, "/models"))
        .bearer_auth(api_key.trim())
        .send()
        .await
        .map_err(|err| format!("请求模型列表失败：{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "拉取模型列表失败（HTTP {}）：{}",
            status.as_u16(),
            summarize_http_error(&body)
        ));
    }

    let payload = response
        .json::<OpenAiModelsResponse>()
        .await
        .map_err(|err| format!("解析 OpenAI 模型列表失败：{err}"))?;
    Ok(dedup_and_sort_models(
        payload.data.into_iter().map(|item| item.id).collect(),
    ))
}

async fn fetch_ollama_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let client = build_model_fetch_client()?;
    let mut request = client.get(join_base_url(base_url, "/api/tags"));
    if !api_key.trim().is_empty() {
        request = request.bearer_auth(api_key.trim());
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("请求模型列表失败：{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "拉取模型列表失败（HTTP {}）：{}",
            status.as_u16(),
            summarize_http_error(&body)
        ));
    }

    let payload = response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|err| format!("解析 Ollama 模型列表失败：{err}"))?;
    Ok(dedup_and_sort_models(
        payload
            .models
            .into_iter()
            .map(|item| {
                let model = item.model.unwrap_or_default();
                if item.name.trim().is_empty() {
                    model
                } else {
                    item.name
                }
            })
            .collect(),
    ))
}

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

fn sanitize_subtitle_settings(
    mut settings: SubtitleExplainSettingsPayload,
) -> SubtitleExplainSettingsPayload {
    settings.provider_profiles = sanitize_provider_profiles(settings.provider_profiles);
    settings.ocr_settings = sanitize_ocr_settings(settings.ocr_settings);
    settings.shortcut_settings = sanitize_shortcut_settings(settings.shortcut_settings);
    let active_profile = sanitize_provider_connection_settings(
        ProviderConnectionSettingsPayload {
            base_url: settings.base_url,
            api_key: settings.api_key,
            model: settings.model,
        },
        &settings.provider,
    );
    *provider_profile_mut(&mut settings.provider_profiles, &settings.provider) =
        active_profile.clone();
    settings.base_url = active_profile.base_url;
    settings.api_key = active_profile.api_key;
    settings.model = active_profile.model;
    settings.ocr_interval_ms = settings.ocr_interval_ms.clamp(250, 4_000);
    settings.stable_frames = settings.stable_frames.clamp(1, 10);
    settings.similarity_threshold = settings.similarity_threshold.clamp(0.5, 1.0);
    settings.blank_clear_ms = settings.blank_clear_ms.clamp(300, 5_000);
    settings
}

fn sanitize_ocr_region_state(mut state: OcrRegionStatePayload) -> OcrRegionStatePayload {
    state.width = state.width.max(MIN_OCR_REGION_WIDTH);
    state.height = state.height.max(MIN_OCR_REGION_HEIGHT);
    state
}

fn sanitize_subtitle_explain_window_state(
    mut state: SubtitleExplainWindowStatePayload,
) -> SubtitleExplainWindowStatePayload {
    state.width = state.width.max(MIN_SUBTITLE_EXPLAIN_WINDOW_WIDTH);
    state.height = state.height.max(MIN_SUBTITLE_EXPLAIN_WINDOW_HEIGHT);
    if matches!(state.x, Some(x) if x.abs() < 0.5)
        && matches!(state.y, Some(y) if y.abs() < 0.5)
        && (state.width - DEFAULT_SUBTITLE_EXPLAIN_WINDOW_WIDTH).abs() < 1.0
        && (state.height - DEFAULT_SUBTITLE_EXPLAIN_WINDOW_HEIGHT).abs() < 1.0
    {
        state.x = None;
        state.y = None;
    }
    state
}

fn effective_ocr_region_visibility(region: &OcrRegionStatePayload) -> bool {
    region.visible || region.show_border_when_idle
}

fn is_valid_hex_color(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.len() == 7 && chars[0] == '#' && chars[1..].iter().all(|c| c.is_ascii_hexdigit())
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
        .get(STORE_KEY_OVERLAY)
        .and_then(|value| serde_json::from_value::<OverlayStatePayload>(value).ok())
        .unwrap_or_default();

    store.close_resource();
    sanitize_state(state)
}

fn load_saved_subtitle_settings(app: &AppHandle) -> SubtitleExplainSettingsPayload {
    let store = match app.store(STORE_PATH) {
        Ok(store) => store,
        Err(_) => return SubtitleExplainSettingsPayload::default(),
    };

    let settings = store
        .get(STORE_KEY_SUBTITLE_EXPLAIN)
        .and_then(|value| serde_json::from_value::<SubtitleExplainSettingsPayload>(value).ok())
        .unwrap_or_default();

    store.close_resource();
    sanitize_subtitle_settings(settings)
}

fn load_saved_subtitle_explain_window_state(app: &AppHandle) -> SubtitleExplainWindowStatePayload {
    let store = match app.store(STORE_PATH) {
        Ok(store) => store,
        Err(_) => return SubtitleExplainWindowStatePayload::default(),
    };

    let state = store
        .get(STORE_KEY_SUBTITLE_EXPLAIN_WINDOW)
        .and_then(|value| serde_json::from_value::<SubtitleExplainWindowStatePayload>(value).ok())
        .unwrap_or_default();

    store.close_resource();
    sanitize_subtitle_explain_window_state(state)
}

fn load_saved_ocr_region(app: &AppHandle) -> OcrRegionStatePayload {
    let store = match app.store(STORE_PATH) {
        Ok(store) => store,
        Err(_) => return OcrRegionStatePayload::default(),
    };

    let region = store
        .get(STORE_KEY_OCR_REGION)
        .and_then(|value| serde_json::from_value::<OcrRegionStatePayload>(value).ok())
        .unwrap_or_default();

    store.close_resource();
    sanitize_ocr_region_state(region)
}

fn save_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|err| err.to_string())?;
    store.set(STORE_KEY_OVERLAY, json!(overlay));
    store.save().map_err(|err| err.to_string())?;
    store.close_resource();
    Ok(())
}

fn save_subtitle_settings(
    app: &AppHandle,
    settings: &SubtitleExplainSettingsPayload,
) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|err| err.to_string())?;
    store.set(STORE_KEY_SUBTITLE_EXPLAIN, json!(settings));
    store.save().map_err(|err| err.to_string())?;
    store.close_resource();
    Ok(())
}

fn save_subtitle_explain_window_state(
    app: &AppHandle,
    state: &SubtitleExplainWindowStatePayload,
) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|err| err.to_string())?;
    store.set(STORE_KEY_SUBTITLE_EXPLAIN_WINDOW, json!(state));
    store.save().map_err(|err| err.to_string())?;
    store.close_resource();
    Ok(())
}

fn save_ocr_region(app: &AppHandle, region: &OcrRegionStatePayload) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|err| err.to_string())?;
    let mut persisted = region.clone();
    persisted.visible = false;
    store.set(STORE_KEY_OCR_REGION, json!(persisted));
    store.save().map_err(|err| err.to_string())?;
    store.close_resource();
    Ok(())
}

fn configured_shortcut_entries(
    shortcuts: &ShortcutSettingsPayload,
) -> [(ConfiguredShortcutAction, &str); 5] {
    [
        (
            ConfiguredShortcutAction::ToggleOverlayVisibility,
            shortcuts.toggle_overlay_visibility.as_str(),
        ),
        (
            ConfiguredShortcutAction::ToggleOverlayLock,
            shortcuts.toggle_overlay_lock.as_str(),
        ),
        (
            ConfiguredShortcutAction::EditOcrRegion,
            shortcuts.edit_ocr_region.as_str(),
        ),
        (
            ConfiguredShortcutAction::StartStaticScreenSelection,
            shortcuts.start_static_screen_selection.as_str(),
        ),
        (
            ConfiguredShortcutAction::AnalyzeCurrentSubtitle,
            shortcuts.analyze_current_subtitle.as_str(),
        ),
    ]
}

fn shortcut_accelerator(shortcut: &str) -> Option<&str> {
    let trimmed = shortcut.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn collect_shortcut_bindings(
    shortcuts: &ShortcutSettingsPayload,
) -> Result<Vec<ParsedShortcutBinding>, String> {
    let mut seen: HashMap<u32, &'static str> = HashMap::new();
    let mut bindings = Vec::new();

    for (action, accelerator) in configured_shortcut_entries(shortcuts) {
        let Some(accelerator) = shortcut_accelerator(accelerator) else {
            continue;
        };
        let shortcut = Shortcut::try_from(accelerator)
            .map_err(|err| format!("{} 快捷键无效：{err}", action.label()))?;
        if let Some(previous_action) = seen.insert(shortcut.id(), action.label()) {
            return Err(format!(
                "快捷键重复：{} 与 {} 不能使用同一组合。",
                previous_action,
                action.label()
            ));
        }
        bindings.push(ParsedShortcutBinding {
            action,
            shortcut,
            accelerator: accelerator.to_string(),
        });
    }

    Ok(bindings)
}

fn run_configured_shortcut_action(
    app: &AppHandle,
    action: ConfiguredShortcutAction,
    event: ShortcutEvent,
) {
    if event.state() != ShortcutState::Pressed {
        return;
    }

    let result = match action {
        ConfiguredShortcutAction::ToggleOverlayVisibility => toggle_visibility(app),
        ConfiguredShortcutAction::ToggleOverlayLock => toggle_lock(app),
        ConfiguredShortcutAction::EditOcrRegion => {
            toggle_ocr_region_editor(app).map(|_| current_overlay_state(app).unwrap_or_default())
        }
        ConfiguredShortcutAction::StartStaticScreenSelection => start_static_screen_selection_inner(app)
            .map(|_| current_overlay_state(app).unwrap_or_default()),
        ConfiguredShortcutAction::AnalyzeCurrentSubtitle => {
            start_manual_subtitle_analysis(app.clone());
            return;
        }
    };

    if let Err(err) = result {
        eprintln!("shortcut handler failed ({}): {err}", action.label());
    }
}

fn register_shortcut_binding(app: &AppHandle, binding: &ParsedShortcutBinding) -> Result<(), String> {
    let action = binding.action;
    app.global_shortcut()
        .on_shortcut(binding.shortcut, move |app, _shortcut, event| {
            run_configured_shortcut_action(app, action, event);
        })
        .map_err(|err| {
            format!(
                "注册 {} 快捷键 {} 失败：{}",
                binding.action.label(),
                binding.accelerator,
                err
            )
        })
}

fn register_shortcut_bindings(
    app: &AppHandle,
    bindings: &[ParsedShortcutBinding],
) -> Result<(), String> {
    for binding in bindings {
        register_shortcut_binding(app, binding)?;
    }

    Ok(())
}

fn replace_configured_shortcuts(
    app: &AppHandle,
    previous_shortcuts: &ShortcutSettingsPayload,
    next_shortcuts: &ShortcutSettingsPayload,
) -> Result<(), String> {
    let next_bindings = collect_shortcut_bindings(next_shortcuts)?;
    let previous_bindings = collect_shortcut_bindings(previous_shortcuts).unwrap_or_default();
    let shortcut_manager = app.global_shortcut();

    shortcut_manager
        .unregister_all()
        .map_err(|err| format!("清空旧快捷键失败：{err}"))?;

    if let Err(err) = register_shortcut_bindings(app, &next_bindings) {
        let _ = shortcut_manager.unregister_all();
        if let Err(restore_err) = register_shortcut_bindings(app, &previous_bindings) {
            return Err(format!("{err}；回滚旧快捷键失败：{restore_err}"));
        }
        return Err(err);
    }

    Ok(())
}

fn current_overlay_state(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.overlay.clone())
}

fn current_ocr_region_state(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.ocr_region.clone())
}

fn current_overlay_restore_state(app: &AppHandle) -> Result<(OverlayStatePayload, bool), String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok((shared.overlay.clone(), shared.ctrl_pressed))
}

fn is_ocr_region_drawing(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.ocr_region_drawing)
}

fn current_ocr_region_draw_request(
    app: &AppHandle,
) -> Result<Option<OcrRegionDrawRequestPayload>, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.ocr_region_draw_request.clone())
}

fn take_static_screen_selection_session(
    app: &AppHandle,
) -> Result<Option<StaticScreenSelectionSession>, String> {
    let state = app.state::<AppState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.static_screen_selection.take())
}

fn start_ocr_region_drawing_session(
    app: &AppHandle,
    draw_request: OcrRegionDrawRequestPayload,
    static_screen_selection: Option<StaticScreenSelectionSession>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    shared.ocr_region_drawing = true;
    shared.ocr_region_draw_request = Some(draw_request);
    shared.static_screen_selection = static_screen_selection;
    Ok(())
}

fn finish_ocr_region_drawing_session(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    shared.ocr_region_drawing = false;
    shared.ocr_region_draw_request = None;
    shared.static_screen_selection = None;
    Ok(())
}

fn current_subtitle_settings(app: &AppHandle) -> Result<SubtitleExplainSettingsPayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.subtitle_settings.clone())
}

fn current_subtitle_explain_window_state(
    app: &AppHandle,
) -> Result<SubtitleExplainWindowStatePayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.subtitle_explain_window.clone())
}

fn sync_overlay_state_from_window(app: &AppHandle) -> Result<OverlayStatePayload, String> {
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

fn sync_subtitle_explain_window_state_from_window(
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

fn sync_ocr_region_state_from_window(
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

fn current_subtitle_status(app: &AppHandle) -> Result<SubtitleStatusPayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.subtitle_status.clone())
}

fn current_subtitle_result(
    app: &AppHandle,
) -> Result<Option<SubtitleAnalysisResultPayload>, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.latest_subtitle_result.clone())
}

fn try_acquire_subtitle_job(app: &AppHandle) -> Result<Option<SubtitleJobGuard>, String> {
    let state = app.state::<AppState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    if shared.subtitle_job_running {
        return Ok(None);
    }

    shared.subtitle_job_running = true;
    Ok(Some(SubtitleJobGuard { app: app.clone() }))
}

fn emit_pixel_frame(app: &AppHandle, data_url: String) -> Result<(), String> {
    app.emit(EVENT_PIXEL_FRAME, PixelFramePayload { data_url })
        .map_err(|err| err.to_string())
}

fn overlay_interaction_active(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.overlay_interaction_until_ms > current_timestamp_ms())
}

fn emit_ocr_region_state(app: &AppHandle, region: &OcrRegionStatePayload) -> Result<(), String> {
    app.emit(EVENT_OCR_REGION_STATE_CHANGED, region.clone())
        .map_err(|err| err.to_string())
}

fn emit_subtitle_status(app: &AppHandle, status: &SubtitleStatusPayload) -> Result<(), String> {
    app.emit(EVENT_SUBTITLE_STATUS_CHANGED, status.clone())
        .map_err(|err| err.to_string())
}

fn emit_subtitle_result(
    app: &AppHandle,
    result: &Option<SubtitleAnalysisResultPayload>,
) -> Result<(), String> {
    app.emit(EVENT_SUBTITLE_RESULT_CHANGED, result.clone())
        .map_err(|err| err.to_string())
}

fn refresh_tray_menu(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let settings = current_subtitle_settings(app)?;
    let menu = build_tray_menu(app, overlay, &settings).map_err(|err| err.to_string())?;
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

fn restore_static_screen_selection_windows(
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

fn effective_overlay_visibility(overlay: &OverlayStatePayload, ctrl_pressed: bool) -> bool {
    overlay.visible && !ctrl_pressed
}

fn apply_window_visibility(window: &tauri::WebviewWindow, visible: bool) -> Result<(), String> {
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

fn apply_window_state(
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

    apply_window_visibility(&window, effective_overlay_visibility(overlay, ctrl_pressed))
}

fn ensure_ocr_region_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
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

fn ensure_main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
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

fn ensure_ocr_runtime_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
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

fn apply_subtitle_explain_window_state(
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

fn commit_subtitle_explain_window_state(
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

fn apply_ocr_region_window_state(
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

fn commit_ocr_region_state(
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

fn set_subtitle_status(
    app: &AppHandle,
    analyzing: bool,
    stage: &str,
    message: &str,
    error: Option<String>,
) -> Result<SubtitleStatusPayload, String> {
    let (status, changed) = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        let next_status = SubtitleStatusPayload {
            analyzing,
            stage: stage.into(),
            message: message.into(),
            error,
        };
        if shared.subtitle_status == next_status {
            (shared.subtitle_status.clone(), false)
        } else {
            shared.subtitle_status = next_status;
            (shared.subtitle_status.clone(), true)
        }
    };

    if changed {
        emit_subtitle_status(app, &status)?;
    }
    Ok(status)
}

fn set_subtitle_result(
    app: &AppHandle,
    result: SubtitleAnalysisResultPayload,
) -> Result<SubtitleAnalysisResultPayload, String> {
    let changed = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        let changed = shared.latest_subtitle_result.as_ref() != Some(&result);
        shared.latest_subtitle_result = Some(result.clone());
        changed
    };

    if changed {
        emit_subtitle_result(app, &Some(result.clone()))?;
    }
    Ok(result)
}

fn clear_subtitle_result(app: &AppHandle) -> Result<(), String> {
    let changed = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        let changed = shared.latest_subtitle_result.is_some();
        shared.latest_subtitle_result = None;
        changed
    };

    if changed {
        emit_subtitle_result(app, &None)?;
    }
    Ok(())
}

fn save_runtime_subtitle_settings(
    app: &AppHandle,
    settings: SubtitleExplainSettingsPayload,
) -> Result<SubtitleExplainSettingsPayload, String> {
    let settings = sanitize_subtitle_settings(settings);
    let previous_settings = {
        let state = app.state::<AppState>();
        let shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.subtitle_settings.clone()
    };

    if previous_settings.shortcut_settings != settings.shortcut_settings {
        replace_configured_shortcuts(
            app,
            &previous_settings.shortcut_settings,
            &settings.shortcut_settings,
        )?;
    }

    {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        shared.subtitle_settings = settings.clone();
    }
    save_subtitle_settings(app, &settings)?;
    Ok(settings)
}

fn update_overlay_state<F>(
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

    apply_window_state(app, &overlay, ctrl_pressed, apply_geometry)?;

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

fn set_ctrl_pressed(app: &AppHandle, ctrl_pressed: bool) -> Result<(), String> {
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

    apply_window_visibility(&window, effective_visible)
}

fn toggle_visibility(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.visible = !shared.overlay.visible;
    })
}

fn toggle_lock(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.locked = !shared.overlay.locked;
    })
}

fn toggle_click_through(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.click_through_when_locked = !shared.overlay.click_through_when_locked;
    })
}

fn reset_overlay_position_inner(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, true, true, |shared| {
        shared.overlay.visible = true;
        shared.overlay.locked = false;
        shared.overlay.x = None;
        shared.overlay.y = None;
    })
}

fn toggle_on_top(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.always_on_top = !shared.overlay.always_on_top;
    })
}

fn toggle_auto_center(app: &AppHandle) -> Result<OverlayStatePayload, String> {
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

fn set_opacity(app: &AppHandle, opacity: f64) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.opacity = opacity;
    })
}

fn set_refresh_rate(app: &AppHandle, refresh_rate: u32) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.refresh_rate = refresh_rate;
    })
}

fn set_pixel_detail(
    app: &AppHandle,
    pixel_detail: PixelDetail,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.pixel_detail = pixel_detail;
    })
}

fn set_appearance(
    app: &AppHandle,
    appearance: OverlayAppearance,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, false, |shared| {
        shared.overlay.appearance = appearance;
    })
}

fn set_color(app: &AppHandle, color: &str) -> Result<OverlayStatePayload, String> {
    let color = color.to_string();
    update_overlay_state(app, true, false, false, move |shared| {
        shared.overlay.color = color;
    })
}

fn show_ocr_region_editor(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
    if is_ocr_region_drawing(app)? {
        let _ = cancel_ocr_region_drawing_inner(app);
    }
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let scale_factor = main_window.scale_factor().map_err(|err| err.to_string())?;
    let position = main_window
        .outer_position()
        .map_err(|err| err.to_string())?;
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

fn hide_ocr_region_editor(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
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

fn toggle_ocr_region_editor(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
    let visible = current_ocr_region_state(app)?.visible;
    if visible {
        hide_ocr_region_editor(app)
    } else {
        show_ocr_region_editor(app)
    }
}

#[derive(Debug)]
struct StaticScreenSelectionSession {
    screenshot_path: PathBuf,
    viewport_width: f64,
    viewport_height: f64,
    overlay: OverlayStatePayload,
    ctrl_pressed: bool,
    ocr_region: OcrRegionStatePayload,
    subtitle_explain: SubtitleExplainWindowStatePayload,
    settings_was_visible: bool,
}

impl Drop for StaticScreenSelectionSession {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.screenshot_path);
    }
}

fn start_ocr_region_drawing_inner(app: &AppHandle) -> Result<(), String> {
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

fn start_static_screen_selection_inner(app: &AppHandle) -> Result<(), String> {
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

        let image = capture_monitor_image(&monitor)?;
        let screenshot_path = save_static_screen_capture(image)?;
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

fn cancel_ocr_region_drawing_inner(app: &AppHandle) -> Result<OcrRegionStatePayload, String> {
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

fn open_subtitle_explain_window(app: &AppHandle) -> Result<(), String> {
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

fn toggle_subtitle_explain_window(app: &AppHandle) -> Result<(), String> {
    if current_subtitle_explain_window_state(app)?.visible {
        let state = sync_subtitle_explain_window_state_from_window(app, Some(false))?;
        commit_subtitle_explain_window_state(app, state, true, true, false)?;
        Ok(())
    } else {
        open_subtitle_explain_window(app)
    }
}

fn sync_persisted_window_states(app: &AppHandle) {
    if let Err(err) = sync_overlay_state_from_window(app) {
        eprintln!("sync overlay state before exit failed: {err}");
    }

    if let Err(err) = sync_subtitle_explain_window_state_from_window(app, None) {
        eprintln!("sync subtitle explain window state before exit failed: {err}");
    }
}

fn open_more_settings_window(app: &AppHandle) -> Result<(), String> {
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
    .min_inner_size(540.0, 680.0)
    .resizable(true)
    .always_on_top(true)
    .center()
    .build()
    .map_err(|err| err.to_string())?;

    disable_window_transitions(&window)?;
    window.set_focus().map_err(|err| err.to_string())?;
    Ok(())
}

fn open_more_settings_menu(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    open_more_settings_window(app)?;
    current_overlay_state(app)
}

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
    ensure_ocr_runtime_window(app)?;

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
    let runtime_window = ensure_ocr_runtime_window(app)?;
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

fn submit_ocr_runtime_response(
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

fn capture_monitor_image(monitor: &Monitor) -> Result<image::RgbaImage, String> {
    let width = monitor.width().map_err(|err| err.to_string())?;
    let height = monitor.height().map_err(|err| err.to_string())?;
    monitor
        .capture_region(0, 0, width, height)
        .map_err(|err| err.to_string())
}

fn save_static_screen_capture(image: image::RgbaImage) -> Result<PathBuf, String> {
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
    session: &StaticScreenSelectionSession,
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
    let window = ensure_ocr_region_window(app)?;

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
        let _ = hide_ocr_region_editor(app);
    } else if was_effectively_visible {
        let _ = apply_ocr_region_window_state(app, &region);
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

    // Windows OCR 对小字比较敏感，字幕场景先放大再识别更稳。
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
    let result = recognize_subtitle_source_text_from_image_path(app, settings, &image_path, allow_empty);
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

    open_subtitle_explain_window(app)?;
    set_subtitle_status(app, true, "llm", "检测到稳定字幕，正在解析...", None)?;
    let result = explain_subtitle_text(app, settings, stable_source_text)?;
    let result = set_subtitle_result(app, result)?;
    tracker.last_emitted_text = Some(normalize_subtitle_text(&result.source_text));
    set_subtitle_status(app, false, "done", "实时字幕解析完成。", None)?;
    Ok(())
}

fn run_subtitle_analysis_blocking(
    app: &AppHandle,
) -> Result<SubtitleAnalysisResultPayload, String> {
    let _job_guard = try_acquire_subtitle_job(app)?
        .ok_or_else(|| "已有一条字幕正在解析中，请稍后再试。".to_string())?;
    let settings = current_subtitle_settings(app)?;

    open_subtitle_explain_window(app)?;
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

fn run_static_screen_selection_analysis_blocking(
    app: &AppHandle,
    selection: OcrSelectionPayload,
) -> Result<SubtitleAnalysisResultPayload, String> {
    let static_session = take_static_screen_selection_session(app)?
        .ok_or_else(|| "当前没有待处理的静态划词会话。".to_string())?;
    finish_ocr_region_drawing_session(app)?;
    restore_static_screen_selection_windows(app, &static_session)?;

    let _job_guard = match try_acquire_subtitle_job(app)? {
        Some(guard) => guard,
        None => {
            let message = "已有一条字幕正在解析中，请稍后再试。".to_string();
            let _ = set_subtitle_status(app, false, "error", "静态划词提交失败。", Some(message.clone()));
            return Err(message);
        }
    };
    let settings = current_subtitle_settings(app)?;

    open_subtitle_explain_window(app)?;
    set_subtitle_status(app, true, "capture", "正在识别划词区域文本...", None)?;

    let image_path = match crop_static_selection_image(
        &static_session,
        &selection,
        settings.ocr_settings.engine,
    ) {
        Ok(image_path) => image_path,
        Err(err) => {
            let _ = set_subtitle_status(app, false, "error", "裁剪静态选区失败。", Some(err.clone()));
            return Err(err);
        }
    };
    let source_text = match recognize_subtitle_source_text_from_image_path(app, &settings, &image_path, false) {
        Ok(source_text) => {
            let _ = fs::remove_file(&image_path);
            source_text
        }
        Err(err) => {
            let _ = fs::remove_file(&image_path);
            let _ = set_subtitle_status(app, false, "error", "识别划词区域失败。", Some(err.clone()));
            return Err(err);
        }
    };

    set_subtitle_status(app, true, "llm", "正在解析划词内容...", None)?;

    match explain_subtitle_text(app, &settings, &source_text) {
        Ok(result) => {
            let result = set_subtitle_result(app, result)?;
            set_subtitle_status(app, false, "done", "划词翻译完成。", None)?;
            Ok(result)
        }
        Err(err) => {
            let _ = set_subtitle_status(app, false, "error", "划词翻译失败。", Some(err.clone()));
            Err(err)
        }
    }
}

fn start_manual_subtitle_analysis(app: AppHandle) {
    thread::spawn(move || {
        if let Err(err) = run_subtitle_analysis_blocking(&app) {
            eprintln!("subtitle analysis failed: {err}");
        }
    });
}

fn start_realtime_subtitle_loop(app: AppHandle) {
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

fn build_overlay_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
    settings: &SubtitleExplainSettingsPayload,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let visibility = MenuItem::with_id(
        app,
        MENU_TOGGLE_VISIBLE,
        if overlay.visible { "隐藏" } else { "显示" },
        true,
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_visibility),
    )?;
    let _lock_legacy = MenuItem::with_id(
        app,
        "toggle_lock_legacy_unused",
        if overlay.locked { "解锁" } else { "锁定" },
        true,
        None::<&str>,
    )?;
    let lock = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_LOCK,
        "锁定",
        true,
        overlay.locked,
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_lock),
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
    let always_on_top = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_ON_TOP,
        "始终置顶",
        true,
        overlay.always_on_top,
        None::<&str>,
    )?;

    let opacity_50 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_50,
        "50%",
        true,
        is_opacity_selected(overlay.opacity, 0.5),
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
    let opacity_70 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_70,
        "70%",
        true,
        is_opacity_selected(overlay.opacity, 0.7),
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
    let opacity_90 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_90,
        "90%",
        true,
        is_opacity_selected(overlay.opacity, 0.9),
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
    let opacity_items: [&dyn IsMenuItem<tauri::Wry>; 6] = [
        &opacity_50,
        &opacity_60,
        &opacity_70,
        &opacity_80,
        &opacity_90,
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
    let _separator_d = PredefinedMenuItem::separator(app)?;

    let items: [&dyn IsMenuItem<tauri::Wry>; 13] = [
        &visibility,
        &lock,
        &click_through,
        &auto_center,
        &separator_a,
        &appearance_menu,
        &refresh_rate_menu,
        &pixel_detail_menu,
        &opacity_menu,
        &color_menu,
        &separator_b,
        &always_on_top,
        &reset_position,
    ];
    let menu = Menu::with_items(app, &items)?;
    Ok(menu)
}

fn build_tray_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
    settings: &SubtitleExplainSettingsPayload,
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
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_visibility),
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
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_lock),
    )?;
    let reset_position = MenuItem::with_id(
        app,
        MENU_RESET_POSITION,
        "重置遮挡条位置",
        true,
        None::<&str>,
    )?;
    let show_subtitle_explain = MenuItem::with_id(
        app,
        MENU_SHOW_SUBTITLE_EXPLAIN,
        "打开字幕助手",
        true,
        None::<&str>,
    )?;
    let more_settings =
        MenuItem::with_id(app, MENU_OPEN_MORE_SETTINGS, "更多设置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;

    let items: [&dyn IsMenuItem<tauri::Wry>; 8] = [
        &visibility,
        &lock,
        &reset_position,
        &separator_a,
        &show_subtitle_explain,
        &separator_b,
        &more_settings,
        &separator_c,
    ];
    let menu = Menu::with_items(app, &items)?;
    menu.append(&quit)?;
    Ok(menu)
}

fn handle_menu_action(app: &AppHandle, id: &str) {
    let result = match id {
        MENU_TOGGLE_VISIBLE => toggle_visibility(app),
        MENU_ANALYZE_CURRENT_SUBTITLE => {
            start_manual_subtitle_analysis(app.clone());
            Ok(current_overlay_state(app).unwrap_or_default())
        }
        MENU_EDIT_OCR_REGION => {
            let _ = toggle_ocr_region_editor(app);
            Ok(current_overlay_state(app).unwrap_or_default())
        }
        MENU_SHOW_SUBTITLE_EXPLAIN => {
            let _ = open_subtitle_explain_window(app);
            Ok(current_overlay_state(app).unwrap_or_default())
        }
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
        MENU_OPACITY_50 => set_opacity(app, 0.5),
        MENU_OPACITY_60 => set_opacity(app, 0.6),
        MENU_OPACITY_70 => set_opacity(app, 0.7),
        MENU_OPACITY_80 => set_opacity(app, 0.8),
        MENU_OPACITY_90 => set_opacity(app, 0.9),
        MENU_OPACITY_100 => set_opacity(app, 1.0),
        MENU_APPEARANCE_PIXELATE => set_appearance(app, OverlayAppearance::Pixelate),
        MENU_APPEARANCE_SOLID => set_appearance(app, OverlayAppearance::Solid),
        MENU_COLOR_BLACK => set_color(app, "#0b0d12"),
        MENU_COLOR_CHARCOAL => set_color(app, "#2b3038"),
        MENU_COLOR_SILVER => set_color(app, "#d0d5dd"),
        MENU_COLOR_WHITE => set_color(app, "#f8fafc"),
        MENU_RESET_POSITION => reset_overlay_position_inner(app),
        MENU_TOGGLE_ON_TOP => toggle_on_top(app),
        MENU_OPEN_MORE_SETTINGS => open_more_settings_menu(app),
        MENU_QUIT => {
            sync_persisted_window_states(app);
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
    let settings = current_subtitle_settings(&app)?;
    let menu = build_overlay_menu(&app, &overlay, &settings).map_err(|err| err.to_string())?;

    window
        .popup_menu_at(&menu, LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn open_more_settings(app: AppHandle) -> Result<(), String> {
    open_more_settings_window(&app)
}

#[tauri::command]
fn open_subtitle_explain(app: AppHandle) -> Result<(), String> {
    open_subtitle_explain_window(&app)
}

#[tauri::command]
fn hide_subtitle_explain(app: AppHandle) -> Result<(), String> {
    let state = sync_subtitle_explain_window_state_from_window(&app, Some(false))?;
    commit_subtitle_explain_window_state(&app, state, true, true, false)?;
    Ok(())
}

#[tauri::command]
fn show_ocr_region(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    show_ocr_region_editor(&app)
}

#[tauri::command]
fn hide_ocr_region(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    hide_ocr_region_editor(&app)
}

#[tauri::command]
fn start_ocr_region_drawing(app: AppHandle) -> Result<(), String> {
    start_ocr_region_drawing_inner(&app)
}

#[tauri::command]
fn start_static_screen_selection(app: AppHandle) -> Result<(), String> {
    start_static_screen_selection_inner(&app)
}

#[tauri::command]
fn cancel_ocr_region_drawing(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    cancel_ocr_region_drawing_inner(&app)
}

#[tauri::command]
fn load_ocr_region_state(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    current_ocr_region_state(&app)
}

#[tauri::command]
fn load_ocr_region_draw_request(
    app: AppHandle,
) -> Result<Option<OcrRegionDrawRequestPayload>, String> {
    current_ocr_region_draw_request(&app)
}

#[tauri::command]
fn apply_ocr_region_state(
    app: AppHandle,
    next_state: OcrRegionStatePayload,
) -> Result<OcrRegionStatePayload, String> {
    finish_ocr_region_drawing_session(&app)?;
    commit_ocr_region_state(&app, next_state, true, true, false)
}

#[tauri::command]
fn save_ocr_region_window_state_command(
    app: AppHandle,
    next_state: OcrRegionStatePayload,
) -> Result<OcrRegionStatePayload, String> {
    if is_ocr_region_drawing(&app)? {
        return current_ocr_region_state(&app);
    }

    let mut state = current_ocr_region_state(&app)?;
    state.width = next_state.width;
    state.height = next_state.height;
    state.x = next_state.x;
    state.y = next_state.y;
    state.visible = next_state.visible;
    state.show_border_when_idle = next_state.show_border_when_idle;

    commit_ocr_region_state(&app, state, false, true, false)
}

#[tauri::command]
fn load_overlay_state(app: AppHandle) -> Result<OverlayStatePayload, String> {
    current_overlay_state(&app)
}

#[tauri::command]
fn load_subtitle_explain_settings(
    app: AppHandle,
) -> Result<SubtitleExplainSettingsPayload, String> {
    current_subtitle_settings(&app)
}

#[tauri::command]
fn save_subtitle_explain_window_state_command(
    app: AppHandle,
    next_state: SubtitleExplainWindowStatePayload,
) -> Result<SubtitleExplainWindowStatePayload, String> {
    let state = current_subtitle_explain_window_state(&app)?;
    let next_state = SubtitleExplainWindowStatePayload {
        visible: next_state.visible,
        width: next_state.width,
        height: next_state.height,
        x: next_state.x,
        y: next_state.y,
    };

    commit_subtitle_explain_window_state(
        &app,
        SubtitleExplainWindowStatePayload {
            visible: next_state.visible,
            width: next_state.width,
            height: next_state.height,
            x: next_state.x.or(state.x),
            y: next_state.y.or(state.y),
        },
        false,
        true,
        false,
    )
}

#[tauri::command]
fn load_subtitle_explain_mode(app: AppHandle) -> Result<SubtitleExplainMode, String> {
    Ok(current_subtitle_settings(&app)?.explain_mode)
}

#[tauri::command]
fn save_subtitle_explain_settings(
    app: AppHandle,
    next_settings: SubtitleExplainSettingsPayload,
) -> Result<SubtitleExplainSettingsPayload, String> {
    let next_settings = save_runtime_subtitle_settings(&app, next_settings)?;
    refresh_tray_menu(&app, &current_overlay_state(&app)?)?;
    Ok(next_settings)
}

#[tauri::command]
fn save_subtitle_explain_mode(
    app: AppHandle,
    explain_mode: SubtitleExplainMode,
) -> Result<SubtitleExplainMode, String> {
    let mut settings = current_subtitle_settings(&app)?;
    settings.explain_mode = explain_mode;
    let settings = save_runtime_subtitle_settings(&app, settings)?;
    Ok(settings.explain_mode)
}

#[tauri::command]
fn reset_subtitle_explain_settings(
    app: AppHandle,
) -> Result<SubtitleExplainSettingsPayload, String> {
    let settings = save_runtime_subtitle_settings(&app, SubtitleExplainSettingsPayload::default())?;
    refresh_tray_menu(&app, &current_overlay_state(&app)?)?;
    Ok(settings)
}

#[tauri::command]
fn inspect_ocr_engines(
    ocr_settings: OcrSettingsPayload,
) -> Result<Vec<OcrEngineAvailabilityPayload>, String> {
    let ocr_settings = sanitize_ocr_settings(ocr_settings);
    Ok(inspect_engines(&ocr_settings))
}

#[tauri::command]
fn notify_ocr_runtime_ready(app: AppHandle) -> Result<(), String> {
    let state = app.state::<OcrRuntimeState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    shared.ready = true;
    Ok(())
}

#[tauri::command]
fn submit_ocr_runtime_result(
    app: AppHandle,
    request_id: String,
    text: String,
) -> Result<(), String> {
    submit_ocr_runtime_response(&app, &request_id, Ok(text))
}

#[tauri::command]
fn submit_ocr_runtime_error(
    app: AppHandle,
    request_id: String,
    error: String,
) -> Result<(), String> {
    submit_ocr_runtime_response(&app, &request_id, Err(error))
}

#[tauri::command]
async fn fetch_available_subtitle_models(
    provider: SubtitleProvider,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    let base_url = normalize_base_url(&base_url, &provider);
    let api_key = api_key.trim().to_string();

    let models = match provider {
        SubtitleProvider::OpenAiResponses => fetch_openai_models(&base_url, &api_key).await?,
        SubtitleProvider::Ollama => fetch_ollama_models(&base_url, &api_key).await?,
    };

    if models.is_empty() {
        return Err("当前接口未返回任何可用模型。".into());
    }

    Ok(models)
}

#[tauri::command]
fn load_subtitle_status(app: AppHandle) -> Result<SubtitleStatusPayload, String> {
    current_subtitle_status(&app)
}

#[tauri::command]
fn load_latest_subtitle_result(
    app: AppHandle,
) -> Result<Option<SubtitleAnalysisResultPayload>, String> {
    current_subtitle_result(&app)
}

#[tauri::command]
async fn analyze_current_subtitle(app: AppHandle) -> Result<SubtitleAnalysisResultPayload, String> {
    tauri::async_runtime::spawn_blocking(move || run_subtitle_analysis_blocking(&app))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn submit_static_screen_selection(
    app: AppHandle,
    selection: OcrSelectionPayload,
) -> Result<SubtitleAnalysisResultPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_static_screen_selection_analysis_blocking(&app, selection)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn apply_overlay_state(
    app: AppHandle,
    next_state: OverlayStatePayload,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(&app, true, false, true, move |shared| {
        shared.overlay = next_state;
    })
}

#[tauri::command]
fn mark_overlay_interacting(app: AppHandle, duration_ms: Option<u64>) -> Result<(), String> {
    let duration_ms = u128::from(duration_ms.unwrap_or(320).clamp(120, 1200));
    let state = app.state::<AppState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    let next_deadline = current_timestamp_ms() + duration_ms;

    if next_deadline > shared.overlay_interaction_until_ms {
        shared.overlay_interaction_until_ms = next_deadline;
    }

    Ok(())
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
    sync_persisted_window_states(&app);
    app.exit(0);
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
fn current_ctrl_pressed() -> bool {
    current_ctrl_hide_active()
}

#[cfg(not(target_os = "windows"))]
fn current_ctrl_pressed() -> bool {
    false
}

#[cfg(target_os = "windows")]
fn start_ctrl_hold_loop(app: AppHandle) {
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
fn start_ctrl_hold_loop(_app: AppHandle) {}

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let overlay = current_overlay_state(app).map_err(|err| err.to_string())?;
    let settings = current_subtitle_settings(app).map_err(|err| err.to_string())?;
    let menu = build_tray_menu(app, &overlay, &settings)?;

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
                let result = match current_subtitle_settings(&app)
                    .map(|settings| settings.tray_click_action)
                    .unwrap_or_default()
                {
                    TrayClickAction::Overlay => toggle_visibility(&app).map(|_| ()),
                    TrayClickAction::SubtitleExplain => toggle_subtitle_explain_window(&app),
                };

                if let Err(err) = result {
                    eprintln!("tray left click failed: {err}");
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn setup_shortcuts(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    app.plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
    let settings = current_subtitle_settings(app).map_err(|err| err.to_string())?;

    match collect_shortcut_bindings(&settings.shortcut_settings) {
        Ok(bindings) => {
            for binding in &bindings {
                if let Err(err) = register_shortcut_binding(app, binding) {
                    eprintln!("{err}");
                }
            }
        }
        Err(err) => {
            eprintln!("load configured shortcuts failed: {err}");
        }
    }

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
            let ocr_region = load_saved_ocr_region(app.handle());
            let subtitle_settings = load_saved_subtitle_settings(app.handle());
            let subtitle_explain_window = load_saved_subtitle_explain_window_state(app.handle());
            let ctrl_pressed = current_ctrl_pressed();
            app.manage(AppState(Mutex::new(SharedState::new(
                overlay.clone(),
                ocr_region.clone(),
                subtitle_settings,
                subtitle_explain_window.clone(),
                ctrl_pressed,
            ))));
            app.manage(SidecarState(Mutex::new(None)));
            app.manage(OcrRuntimeState(Mutex::new(OcrRuntimeShared::default())));

            ensure_main_window(app.handle())?;
            ensure_ocr_region_window(app.handle())?;
            ensure_ocr_runtime_window(app.handle())?;
            apply_window_state(app.handle(), &overlay, ctrl_pressed, true)?;
            apply_ocr_region_window_state(app.handle(), &ocr_region)?;
            if subtitle_explain_window.visible {
                apply_subtitle_explain_window_state(app.handle(), &subtitle_explain_window, false)?;
            }
            emit_state(app.handle(), &overlay)?;
            emit_ocr_region_state(app.handle(), &ocr_region)?;
            emit_subtitle_status(app.handle(), &SubtitleStatusPayload::default())?;
            setup_tray(app.handle())?;
            start_pixelate_loop(app.handle().clone());
            start_ctrl_hold_loop(app.handle().clone());
            start_realtime_subtitle_loop(app.handle().clone());

            #[cfg(desktop)]
            setup_shortcuts(app.handle())?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_overlay_state,
            load_ocr_region_state,
            load_ocr_region_draw_request,
            apply_ocr_region_state,
            save_ocr_region_window_state_command,
            show_ocr_region,
            hide_ocr_region,
            start_ocr_region_drawing,
            start_static_screen_selection,
            cancel_ocr_region_drawing,
            load_subtitle_explain_settings,
            save_subtitle_explain_window_state_command,
            load_subtitle_explain_mode,
            save_subtitle_explain_settings,
            save_subtitle_explain_mode,
            reset_subtitle_explain_settings,
            inspect_ocr_engines,
            notify_ocr_runtime_ready,
            submit_ocr_runtime_result,
            submit_ocr_runtime_error,
            fetch_available_subtitle_models,
            load_subtitle_status,
            load_latest_subtitle_result,
            apply_overlay_state,
            mark_overlay_interacting,
            toggle_overlay_visibility,
            toggle_overlay_lock,
            toggle_overlay_on_top,
            reset_overlay_position,
            show_overlay_menu,
            open_more_settings,
            open_subtitle_explain,
            hide_subtitle_explain,
            analyze_current_subtitle,
            submit_static_screen_selection,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
