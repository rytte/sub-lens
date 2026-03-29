mod app_window;
mod menu;
mod ocr;
mod overlay;
mod subtitle;

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
            Self::StartStaticScreenSelection => "截图翻译",
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
    static_screen_selection: Option<app_window::StaticScreenSelectionSession>,
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
        ConfiguredShortcutAction::ToggleOverlayVisibility => overlay::toggle_visibility(app),
        ConfiguredShortcutAction::ToggleOverlayLock => overlay::toggle_lock(app),
        ConfiguredShortcutAction::EditOcrRegion => {
            app_window::toggle_ocr_region_editor(app)
                .map(|_| current_overlay_state(app).unwrap_or_default())
        }
        ConfiguredShortcutAction::StartStaticScreenSelection => {
            app_window::start_static_screen_selection_inner(app)
                .map(|_| current_overlay_state(app).unwrap_or_default())
        }
        ConfiguredShortcutAction::AnalyzeCurrentSubtitle => {
            subtitle::start_manual_subtitle_analysis(app.clone());
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
) -> Result<Option<app_window::StaticScreenSelectionSession>, String> {
    let state = app.state::<AppState>();
    let mut shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.static_screen_selection.take())
}

fn start_ocr_region_drawing_session(
    app: &AppHandle,
    draw_request: OcrRegionDrawRequestPayload,
    static_screen_selection: Option<app_window::StaticScreenSelectionSession>,
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

fn emit_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    app.emit(EVENT_STATE_CHANGED, overlay.clone())
        .map_err(|err| err.to_string())
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

#[tauri::command]
fn show_overlay_menu(app: AppHandle, x: f64, y: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let overlay = current_overlay_state(&app)?;
    let settings = current_subtitle_settings(&app)?;
    let menu = menu::build_overlay_menu(&app, &overlay, &settings).map_err(|err| err.to_string())?;

    window
        .popup_menu_at(&menu, LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn open_more_settings(app: AppHandle) -> Result<(), String> {
    app_window::open_more_settings_window(&app)
}

#[tauri::command]
fn open_subtitle_explain(app: AppHandle) -> Result<(), String> {
    app_window::open_subtitle_explain_window(&app)
}

#[tauri::command]
fn hide_subtitle_explain(app: AppHandle) -> Result<(), String> {
    let state = app_window::sync_subtitle_explain_window_state_from_window(&app, Some(false))?;
    app_window::commit_subtitle_explain_window_state(&app, state, true, true, false)?;
    Ok(())
}

#[tauri::command]
fn show_ocr_region(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    app_window::show_ocr_region_editor(&app)
}

#[tauri::command]
fn hide_ocr_region(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    app_window::hide_ocr_region_editor(&app)
}

#[tauri::command]
fn start_ocr_region_drawing(app: AppHandle) -> Result<(), String> {
    app_window::start_ocr_region_drawing_inner(&app)
}

#[tauri::command]
fn start_static_screen_selection(app: AppHandle) -> Result<(), String> {
    app_window::start_static_screen_selection_inner(&app)
}

#[tauri::command]
fn cancel_ocr_region_drawing(app: AppHandle) -> Result<OcrRegionStatePayload, String> {
    app_window::cancel_ocr_region_drawing_inner(&app)
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
    app_window::commit_ocr_region_state(&app, next_state, true, true, false)
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

    app_window::commit_ocr_region_state(&app, state, false, true, false)
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

    app_window::commit_subtitle_explain_window_state(
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
    menu::refresh_tray_menu(&app, &current_overlay_state(&app)?)?;
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
    menu::refresh_tray_menu(&app, &current_overlay_state(&app)?)?;
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
    subtitle::submit_ocr_runtime_response(&app, &request_id, Ok(text))
}

#[tauri::command]
fn submit_ocr_runtime_error(
    app: AppHandle,
    request_id: String,
    error: String,
) -> Result<(), String> {
    subtitle::submit_ocr_runtime_response(&app, &request_id, Err(error))
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
    tauri::async_runtime::spawn_blocking(move || subtitle::run_subtitle_analysis_blocking(&app))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn submit_static_screen_selection(
    app: AppHandle,
    selection: OcrSelectionPayload,
) -> Result<SubtitleAnalysisResultPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        subtitle::run_static_screen_selection_analysis_blocking(&app, selection)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn apply_overlay_state(
    app: AppHandle,
    next_state: OverlayStatePayload,
) -> Result<OverlayStatePayload, String> {
    overlay::update_overlay_state(&app, true, false, true, move |shared| {
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
    overlay::toggle_visibility(&app)
}

#[tauri::command]
fn toggle_overlay_lock(app: AppHandle) -> Result<OverlayStatePayload, String> {
    overlay::toggle_lock(&app)
}

#[tauri::command]
fn toggle_overlay_on_top(app: AppHandle) -> Result<OverlayStatePayload, String> {
    overlay::toggle_on_top(&app)
}

#[tauri::command]
fn reset_overlay_position(app: AppHandle) -> Result<OverlayStatePayload, String> {
    overlay::reset_overlay_position_inner(&app)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app_window::sync_persisted_window_states(&app);
    app.exit(0);
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
            menu::handle_menu_action(app, event.id.as_ref());
        })
        .setup(|app| {
            let overlay = load_saved_state(app.handle());
            let ocr_region = load_saved_ocr_region(app.handle());
            let subtitle_settings = load_saved_subtitle_settings(app.handle());
            let subtitle_explain_window = load_saved_subtitle_explain_window_state(app.handle());
            let ctrl_pressed = overlay::current_ctrl_pressed();
            app.manage(AppState(Mutex::new(SharedState::new(
                overlay.clone(),
                ocr_region.clone(),
                subtitle_settings,
                subtitle_explain_window.clone(),
                ctrl_pressed,
            ))));
            app.manage(SidecarState(Mutex::new(None)));
            app.manage(OcrRuntimeState(Mutex::new(OcrRuntimeShared::default())));

            app_window::ensure_main_window(app.handle())?;
            app_window::ensure_ocr_region_window(app.handle())?;
            app_window::ensure_ocr_runtime_window(app.handle())?;
            app_window::apply_window_state(app.handle(), &overlay, ctrl_pressed, true)?;
            app_window::apply_ocr_region_window_state(app.handle(), &ocr_region)?;
            if subtitle_explain_window.visible {
                app_window::apply_subtitle_explain_window_state(
                    app.handle(),
                    &subtitle_explain_window,
                    false,
                )?;
            }
            emit_state(app.handle(), &overlay)?;
            emit_ocr_region_state(app.handle(), &ocr_region)?;
            emit_subtitle_status(app.handle(), &SubtitleStatusPayload::default())?;
            menu::setup_tray(app.handle())?;
            overlay::start_pixelate_loop(app.handle().clone());
            overlay::start_ctrl_hold_loop(app.handle().clone());
            subtitle::start_realtime_subtitle_loop(app.handle().clone());

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
