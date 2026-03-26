import { invoke } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const FONT_SCALE_VALUES = ["1.0", "1.1", "1.2", "1.3", "1.4", "1.5"] as const;
const DEFAULT_FONT_SCALE = FONT_SCALE_VALUES[0];
const LEGACY_FONT_SCALE_MAP = {
  normal: "1.0",
  large: "1.1",
  xlarge: "1.2",
} as const;

type ResizeDirection = "East" | "West";
type StyleMode = "default" | "contrast" | "contrastLight";
type FontScale = (typeof FONT_SCALE_VALUES)[number];
type ExplainMode = "phrase" | "word";
type SubtitleProvider = "openAiResponses" | "ollama";

type ProviderConnectionSettings = {
  baseUrl: string;
  apiKey: string;
  model: string;
};

type SubtitleProviderProfiles = {
  openAiResponses: ProviderConnectionSettings;
  ollama: ProviderConnectionSettings;
};

type SubtitleExplainSettings = {
  realtimeEnabled: boolean;
  provider: SubtitleProvider;
  explainMode: ExplainMode;
  baseUrl: string;
  apiKey: string;
  model: string;
  providerProfiles: SubtitleProviderProfiles;
  ocrIntervalMs: number;
  stableFrames: number;
  similarityThreshold: number;
  blankClearMs: number;
};

type OcrRegionState = {
  visible: boolean;
  showBorderWhenIdle: boolean;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
};

type SubtitlePhrase = {
  source: string;
  explanation: string;
};

type SubtitleResult = {
  sourceText: string;
  translation: string;
  phrases: SubtitlePhrase[];
};

type SubtitleStatus = {
  analyzing: boolean;
  stage: string;
  message: string;
  error: string | null;
};

const EVENT_OCR_REGION_STATE_CHANGED = "ocr-region://state";
const EVENT_SUBTITLE_STATUS_CHANGED = "subtitle://status";
const EVENT_SUBTITLE_RESULT_CHANGED = "subtitle://result";
const STYLE_MODE_STORAGE_KEY = "subtitle-explain-style-mode";
const CONTRAST_OUTLINE_STORAGE_KEY = "subtitle-explain-contrast-outline-enabled";
const FONT_SCALE_STORAGE_KEY = "subtitle-explain-font-scale";
const AUTO_HEIGHT_STORAGE_KEY = "subtitle-explain-auto-height-enabled";
const WINDOW_MIN_WIDTH = 360;
const WINDOW_MIN_HEIGHT = 140;
const AUTO_HEIGHT_MAX_WINDOW_HEIGHT = 880;
const RESIZE_SETTLE_MS = 140;
const appWindow = getCurrentWindow();

let status: SubtitleStatus | null = null;
let result: SubtitleResult | null = null;
let styleMode: StyleMode = loadStyleMode();
let contrastOutlineEnabled = loadContrastOutlineEnabled();
let fontScale: FontScale = loadFontScale();
let autoHeightEnabled = loadAutoHeightEnabled();
let explainMode: ExplainMode = "word";
let subtitleSettings: SubtitleExplainSettings | null = null;
let ocrRegionState: OcrRegionState | null = null;
let windowHovered = false;
let settingsOpen = false;
let collapsed = false;
let sizeSyncFrame = 0;
let windowResizeObserver: ResizeObserver | null = null;
let sizeSyncTimeout = 0;
let realtimeTogglePending = false;

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

const appRoot = app;

app.innerHTML = `
  <main class="subtitle-window">
      <button class="resize-handle east" data-direction="East" aria-label="向右拉伸"></button>
      <button class="resize-handle west" data-direction="West" aria-label="向左拉伸"></button>

      <div class="status-row">
        <div class="status-meta" data-tauri-drag-region>
          <div class="drag-meta" data-tauri-drag-region>
            <span class="drag-pill"></span>
            <span class="status-label">字幕助手</span>
          </div>
          <span id="statusText" class="status-text" data-state="idle">等待手动解析。</span>
        </div>
        <div class="action-row">
          <button id="editOcrButton" class="action-button" type="button">
            编辑 OCR 区域
          </button>
          <button
            id="realtimeToggleButton"
            class="action-button action-button-toggle-live"
            type="button"
            aria-label="切换实时解析"
            aria-pressed="false"
          >
            实时解析关
          </button>
          <button id="analyzeButton" class="action-button action-button-primary" type="button">
            解析当前字幕
          </button>
          <div class="settings-menu">
            <button
              id="settingsMenuButton"
              class="action-button action-button-menu"
              type="button"
              aria-haspopup="dialog"
              aria-expanded="false"
            >
              设置
              <span class="action-caret" aria-hidden="true">▾</span>
            </button>

            <div id="settingsPopover" class="settings-popover" role="dialog" aria-label="设置">
              <section class="settings-group">
                <p class="settings-group-title">解释</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="explainMode" data-value="word">
                    逐词
                  </button>
                  <button class="settings-option" type="button" data-setting="explainMode" data-value="phrase">
                    短语
                  </button>
                </div>
              </section>

              <section class="settings-group">
                <p class="settings-group-title">样式</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="styleMode" data-value="default">
                    默认
                  </button>
                  <button class="settings-option" type="button" data-setting="styleMode" data-value="contrast">
                    黑字
                  </button>
                  <button class="settings-option" type="button" data-setting="styleMode" data-value="contrastLight">
                    白字
                  </button>
                </div>
              </section>

              <section class="settings-group">
                <p class="settings-group-title">\u5b57\u4f53\u63cf\u8fb9</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="contrastOutline" data-value="disabled">
                    \u5173\u95ed
                  </button>
                  <button class="settings-option" type="button" data-setting="contrastOutline" data-value="enabled">
                    \u5f00\u542f
                  </button>
                </div>
              </section>

              <section class="settings-group">
                <p class="settings-group-title">窗口高度</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="autoHeight" data-value="disabled">
                    固定
                  </button>
                  <button class="settings-option" type="button" data-setting="autoHeight" data-value="enabled">
                    自适应
                  </button>
                </div>
              </section>

              <section class="settings-group">
                <div class="settings-slider-header">
                  <p class="settings-group-title">字号</p>
                  <output id="fontScaleValue" class="settings-slider-value" for="fontScaleSlider">
                    100%
                  </output>
                </div>
                <div class="settings-slider">
                  <input
                    id="fontScaleSlider"
                    class="settings-slider-input"
                    type="range"
                    min="0"
                    max="5"
                    step="1"
                    value="0"
                    aria-label="调节内容字号"
                  />
                  <div class="settings-slider-labels" aria-hidden="true">
                    <span>100%</span>
                    <span>110%</span>
                    <span>120%</span>
                    <span>130%</span>
                    <span>140%</span>
                    <span>150%</span>
                  </div>
                </div>
              </section>

              <section class="settings-group">
                <p class="settings-group-title">OCR 边框</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="ocrBorderMode" data-value="editOnly">
                    仅编辑时
                  </button>
                  <button class="settings-option" type="button" data-setting="ocrBorderMode" data-value="alwaysVisible">
                    常显
                  </button>
                </div>
              </section>
            </div>
          </div>
          <button
            id="collapseButton"
            class="action-button action-button-icon action-button-toggle"
            type="button"
            aria-label="收起内容"
            title="收起内容"
          >
            ▴
          </button>
          <button
            id="hideButton"
            class="action-button action-button-icon"
            type="button"
            aria-label="隐藏窗口"
            title="隐藏窗口"
          >
            ×
          </button>
          <div id="collapsedDragRegion" class="collapsed-drag-region" aria-hidden="true"></div>
        </div>
      </div>

      <div class="content-stack">
        <section class="block block-summary">
          <div class="summary-row">
            <span class="summary-label">原文</span>
            <p id="sourceText" class="summary-text summary-text-source empty">等待 OCR 结果...</p>
          </div>
          <div class="summary-divider" aria-hidden="true"></div>
          <div class="summary-row">
            <span class="summary-label">翻译</span>
            <p id="translationText" class="summary-text empty">等待 LLM 输出...</p>
          </div>
        </section>

        <section class="block block-phrases">
          <div id="phrasesTitle" class="block-title">短语解释</div>
          <div id="phrasesList" class="phrase-list empty">暂无逐词解释。</div>
        </section>
      </div>
  </main>
`;

function requireElement<T extends HTMLElement>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`Missing element: ${selector}`);
  }

  return element;
}

const statusText = requireElement<HTMLParagraphElement>("#statusText");
const statusMeta = requireElement<HTMLDivElement>(".status-meta");
const windowContent = requireElement<HTMLElement>(".subtitle-window");
const statusRow = requireElement<HTMLDivElement>(".status-row");
const contentStack = requireElement<HTMLDivElement>(".content-stack");
const editOcrButton = requireElement<HTMLButtonElement>("#editOcrButton");
const realtimeToggleButton = requireElement<HTMLButtonElement>("#realtimeToggleButton");
const settingsMenu = requireElement<HTMLDivElement>(".settings-menu");
const settingsMenuButton = requireElement<HTMLButtonElement>("#settingsMenuButton");
const settingsPopover = requireElement<HTMLDivElement>("#settingsPopover");
const fontScaleSlider = requireElement<HTMLInputElement>("#fontScaleSlider");
const fontScaleValue = requireElement<HTMLOutputElement>("#fontScaleValue");
const analyzeButton = requireElement<HTMLButtonElement>("#analyzeButton");
const collapseButton = requireElement<HTMLButtonElement>("#collapseButton");
const hideButton = requireElement<HTMLButtonElement>("#hideButton");
const collapsedDragRegion = requireElement<HTMLDivElement>("#collapsedDragRegion");
const sourceText = requireElement<HTMLParagraphElement>("#sourceText");
const translationText = requireElement<HTMLParagraphElement>("#translationText");
const phrasesTitle = requireElement<HTMLDivElement>("#phrasesTitle");
const phrasesList = requireElement<HTMLDivElement>("#phrasesList");
const settingsOptionButtons = Array.from(
  document.querySelectorAll<HTMLButtonElement>(".settings-option[data-setting][data-value]"),
);
const resizeHandles = Array.from(
  document.querySelectorAll<HTMLButtonElement>("[data-direction]"),
);

async function init() {
  bindUi();
  bindEvents();
  applyDisplaySettings();
  await syncWindowSizeConstraints();

  try {
    const [
      initialStatus,
      initialResult,
      initialExplainMode,
      initialSettings,
      initialOcrRegionState,
    ] = await Promise.all([
      invoke<SubtitleStatus>("load_subtitle_status"),
      invoke<SubtitleResult | null>("load_latest_subtitle_result"),
      invoke<ExplainMode>("load_subtitle_explain_mode"),
      invoke<SubtitleExplainSettings>("load_subtitle_explain_settings"),
      invoke<OcrRegionState>("load_ocr_region_state"),
    ]);
    status = initialStatus;
    result = initialResult;
    explainMode = initialExplainMode;
    subtitleSettings = initialSettings;
    ocrRegionState = initialOcrRegionState;
  } catch (error) {
    status = {
      analyzing: false,
      stage: "error",
      message: "字幕助手初始化失败。",
      error: String(error),
    };
  }

  bindWindowAutoSizing();
  render();
}

function bindUi() {
  bindDragRegion(statusMeta);
  bindDragRegion(collapsedDragRegion);

  resizeHandles.forEach((handle) => {
    handle.addEventListener("pointerdown", async (event) => {
      if (event.button !== 0) {
        return;
      }

      const direction = handle.dataset.direction as ResizeDirection;
      await appWindow.startResizeDragging(direction);
    });
  });

  analyzeButton.addEventListener("click", () => {
    void analyzeCurrentSubtitle();
  });

  collapseButton.addEventListener("click", () => {
    setCollapsed(!collapsed);
  });

  editOcrButton.addEventListener("click", () => {
    void invoke("show_ocr_region");
  });

  realtimeToggleButton.addEventListener("click", () => {
    void toggleRealtimeParsing();
  });

  settingsMenuButton.addEventListener("click", (event) => {
    event.stopPropagation();
    setSettingsOpen(!settingsOpen);
  });

  settingsPopover.addEventListener("pointerdown", (event) => {
    event.stopPropagation();
  });

  settingsOptionButtons.forEach((button) => {
    button.addEventListener("click", () => {
      void applySetting(button.dataset.setting, button.dataset.value);
    });
  });

  fontScaleSlider.addEventListener("input", () => {
    applyFontScaleSliderValue(fontScaleSlider.value);
  });

  hideButton.addEventListener("click", () => {
    void hideWindow();
  });

  windowContent.addEventListener("pointerenter", () => {
    setWindowHovered(true);
  });

  windowContent.addEventListener("pointerleave", () => {
    setWindowHovered(false);
  });

  document.addEventListener("pointerdown", (event) => {
    if (!(event.target instanceof Node) || settingsMenu.contains(event.target)) {
      return;
    }

    setSettingsOpen(false);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      setSettingsOpen(false);
    }
  });
}

function bindDragRegion(element: HTMLElement) {
  element.addEventListener("pointerdown", async (event) => {
    if (event.button !== 0) {
      return;
    }

    await appWindow.startDragging();
  });
}

function bindEvents() {
  void listen<SubtitleStatus>(EVENT_SUBTITLE_STATUS_CHANGED, (event) => {
    status = event.payload;
    renderStatus();
  });

  void listen<OcrRegionState>(EVENT_OCR_REGION_STATE_CHANGED, (event) => {
    ocrRegionState = event.payload;
    applyDisplaySettings();
  });

  void listen<SubtitleResult | null>(EVENT_SUBTITLE_RESULT_CHANGED, (event) => {
    result = event.payload;
    render();
  });
}

function bindWindowAutoSizing() {
  windowResizeObserver?.disconnect();
  windowResizeObserver = new ResizeObserver(() => {
    scheduleWindowSizeSync(RESIZE_SETTLE_MS);
  });
  windowResizeObserver.observe(windowContent);
}

function scheduleWindowSizeSync(delay = 0) {
  if (!autoHeightEnabled) {
    cancelWindowSizeSync();
    return;
  }

  if (sizeSyncTimeout) {
    window.clearTimeout(sizeSyncTimeout);
    sizeSyncTimeout = 0;
  }

  if (delay > 0) {
    sizeSyncTimeout = window.setTimeout(() => {
      sizeSyncTimeout = 0;
      scheduleWindowSizeSync();
    }, delay);
    return;
  }

  if (sizeSyncFrame) {
    return;
  }

  sizeSyncFrame = window.requestAnimationFrame(() => {
    sizeSyncFrame = 0;
    void syncWindowSize();
  });
}

function cancelWindowSizeSync() {
  if (sizeSyncTimeout) {
    window.clearTimeout(sizeSyncTimeout);
    sizeSyncTimeout = 0;
  }

  if (sizeSyncFrame) {
    window.cancelAnimationFrame(sizeSyncFrame);
    sizeSyncFrame = 0;
  }
}

async function syncWindowSizeConstraints() {
  try {
    await appWindow.setSizeConstraints({
      minWidth: WINDOW_MIN_WIDTH,
      minHeight: WINDOW_MIN_HEIGHT,
      maxHeight: autoHeightEnabled ? AUTO_HEIGHT_MAX_WINDOW_HEIGHT : undefined,
    });
  } catch (error) {
    console.error("set subtitle window constraints failed:", error);
  }
}

function loadStyleMode(): StyleMode {
  try {
    const value = window.localStorage.getItem(STYLE_MODE_STORAGE_KEY);

    if (value === "contrast" || value === "contrastLight") {
      return value;
    }
  } catch {
    return "default";
  }

  return "default";
}

function loadFontScale(): FontScale {
  try {
    const value = window.localStorage.getItem(FONT_SCALE_STORAGE_KEY);

    if (value && isFontScale(value)) {
      return value;
    }

    if (value === "normal" || value === "large" || value === "xlarge") {
      const migratedValue = LEGACY_FONT_SCALE_MAP[value];
      window.localStorage.setItem(FONT_SCALE_STORAGE_KEY, migratedValue);
      return migratedValue;
    }
  } catch {
    return DEFAULT_FONT_SCALE;
  }

  return DEFAULT_FONT_SCALE;
}

function loadContrastOutlineEnabled() {
  try {
    return window.localStorage.getItem(CONTRAST_OUTLINE_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

function loadAutoHeightEnabled() {
  try {
    return window.localStorage.getItem(AUTO_HEIGHT_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

function applyDisplaySettings() {
  windowContent.dataset.styleMode = styleMode;
  windowContent.dataset.contrastOutline = contrastOutlineEnabled ? "true" : "false";
  windowContent.style.setProperty("--content-font-scale", fontScale);
  windowContent.dataset.hovered =
    styleMode !== "default" && windowHovered ? "true" : "false";
  syncCollapsedState();
  phrasesTitle.textContent = phrasesSectionTitle();
  if (!result || phrasesList.classList.contains("empty")) {
    phrasesList.textContent = emptyPhrasesText();
  }
  syncSettingsOptions();
  syncSettingsPopover();
}

function syncSettingsOptions() {
  settingsOptionButtons.forEach((button) => {
    const { setting, value } = button.dataset;
    const active =
      (setting === "explainMode" && value === explainMode) ||
      (setting === "styleMode" && value === styleMode) ||
      (setting === "contrastOutline" &&
        value === (contrastOutlineEnabled ? "enabled" : "disabled")) ||
      (setting === "autoHeight" &&
        value === (autoHeightEnabled ? "enabled" : "disabled")) ||
      (setting === "ocrBorderMode" &&
        value === (ocrRegionState?.showBorderWhenIdle ? "alwaysVisible" : "editOnly"));
    button.dataset.active = active ? "true" : "false";
  });

  fontScaleSlider.value = fontScaleToSliderValue(fontScale);
  fontScaleSlider.setAttribute("aria-valuetext", fontScaleLabel(fontScale));
  fontScaleValue.textContent = fontScaleLabel(fontScale);
}

function setSettingsOpen(nextOpen: boolean) {
  if (settingsOpen === nextOpen) {
    return;
  }

  settingsOpen = nextOpen;
  syncSettingsPopover();
}

function syncSettingsPopover() {
  settingsMenuButton.dataset.open = settingsOpen ? "true" : "false";
  settingsMenuButton.setAttribute("aria-expanded", settingsOpen ? "true" : "false");
  settingsPopover.dataset.open = settingsOpen ? "true" : "false";
}

function setCollapsed(nextCollapsed: boolean) {
  if (collapsed === nextCollapsed) {
    return;
  }

  collapsed = nextCollapsed;
  if (collapsed) {
    setSettingsOpen(false);
  }
  syncCollapsedState();
  scheduleWindowSizeSync();
}

function syncCollapsedState() {
  windowContent.dataset.collapsed = collapsed ? "true" : "false";
  collapseButton.textContent = collapsed ? "▾" : "▴";
  collapseButton.setAttribute("aria-label", collapsed ? "展开内容" : "收起内容");
  collapseButton.title = collapsed ? "展开内容" : "收起内容";
  if (collapsed) {
    phrasesTitle.setAttribute("data-tauri-drag-region", "");
  } else {
    phrasesTitle.removeAttribute("data-tauri-drag-region");
  }
}

async function applySetting(setting?: string, value?: string) {
  if (!setting || !value) {
    return;
  }

  if (setting === "explainMode" && (value === "phrase" || value === "word")) {
    const previousMode = explainMode;
    explainMode = value;
    applyDisplaySettings();
    scheduleWindowSizeSync();

    try {
      explainMode = await invoke<ExplainMode>("save_subtitle_explain_mode", {
        explainMode,
      });
    } catch (error) {
      explainMode = previousMode;
      applyDisplaySettings();
      statusText.textContent = `切换解释模式失败：${String(error)}`;
      statusText.dataset.state = "error";
      return;
    }

    applyDisplaySettings();
    return;
  }

  if (
    setting === "styleMode" &&
    (value === "default" || value === "contrast" || value === "contrastLight")
  ) {
    styleMode = value;
    persistSetting(STYLE_MODE_STORAGE_KEY, styleMode);
  }

  if (setting === "contrastOutline" && (value === "enabled" || value === "disabled")) {
    contrastOutlineEnabled = value === "enabled";
    persistSetting(CONTRAST_OUTLINE_STORAGE_KEY, String(contrastOutlineEnabled));
  }

  if (setting === "autoHeight" && (value === "enabled" || value === "disabled")) {
    autoHeightEnabled = value === "enabled";
    persistSetting(AUTO_HEIGHT_STORAGE_KEY, String(autoHeightEnabled));
    applyDisplaySettings();
    await syncWindowSizeConstraints();
    if (autoHeightEnabled) {
      scheduleWindowSizeSync();
    } else {
      cancelWindowSizeSync();
    }
    return;
  }

  if (setting === "ocrBorderMode" && (value === "editOnly" || value === "alwaysVisible")) {
    const previousOcrRegionState = ocrRegionState;
    const showBorderWhenIdle = value === "alwaysVisible";

    try {
      const latestOcrRegionState = await invoke<OcrRegionState>("load_ocr_region_state");
      ocrRegionState = await invoke<OcrRegionState>("apply_ocr_region_state", {
        nextState: {
          ...latestOcrRegionState,
          showBorderWhenIdle,
        },
      });
    } catch (error) {
      ocrRegionState = previousOcrRegionState;
      applyDisplaySettings();
      statusText.textContent = `切换 OCR 边框显示失败：${String(error)}`;
      statusText.dataset.state = "error";
      return;
    }
  }

  applyDisplaySettings();
  scheduleWindowSizeSync();
}

function phrasesSectionTitle() {
  return explainMode === "word" ? "逐词解释" : "短语解释";
}

function emptyPhrasesText() {
  return explainMode === "word" ? "暂无逐词解释。" : "暂无短语解释。";
}

function persistSetting(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures and keep the current in-memory selection.
  }
}

function isFontScale(value: string): value is FontScale {
  return FONT_SCALE_VALUES.includes(value as FontScale);
}

function fontScaleToSliderValue(value: FontScale) {
  const sliderValue = FONT_SCALE_VALUES.indexOf(value);
  return String(sliderValue >= 0 ? sliderValue : 0);
}

function fontScaleFromSliderValue(value: string): FontScale {
  const sliderValue = Number.parseInt(value, 10);

  if (Number.isNaN(sliderValue)) {
    return DEFAULT_FONT_SCALE;
  }

  const clampedValue = Math.max(0, Math.min(FONT_SCALE_VALUES.length - 1, sliderValue));
  return FONT_SCALE_VALUES[clampedValue];
}

function fontScaleLabel(value: FontScale) {
  return `${Math.round(Number(value) * 100)}%`;
}

function applyFontScaleSliderValue(value: string) {
  const nextFontScale = fontScaleFromSliderValue(value);

  if (nextFontScale === fontScale) {
    syncSettingsOptions();
    return;
  }

  fontScale = nextFontScale;
  persistSetting(FONT_SCALE_STORAGE_KEY, fontScale);
  applyDisplaySettings();
  scheduleWindowSizeSync();
}

function setWindowHovered(nextHovered: boolean) {
  if (windowHovered === nextHovered) {
    return;
  }

  windowHovered = nextHovered;
  if (styleMode !== "default") {
    windowContent.dataset.hovered = nextHovered ? "true" : "false";
  }
}

async function syncWindowSize() {
  try {
    const targetHeight = Math.max(
      WINDOW_MIN_HEIGHT,
      Math.min(AUTO_HEIGHT_MAX_WINDOW_HEIGHT, getNaturalWindowHeight()),
    );
    const scaleFactor = await appWindow.scaleFactor();
    const currentSize = (await appWindow.innerSize()).toLogical(scaleFactor);

    if (Math.abs(currentSize.height - targetHeight) < 1) {
      return;
    }

    await appWindow.setSize(new LogicalSize(currentSize.width, targetHeight));
  } catch (error) {
    console.error("sync subtitle window size failed:", error);
  }
}

function getNaturalWindowHeight() {
  const appStyles = window.getComputedStyle(appRoot);
  const appPaddingTop = parseFloat(appStyles.paddingTop) || 0;
  const appPaddingBottom = parseFloat(appStyles.paddingBottom) || 0;
  const appBorderTop = parseFloat(appStyles.borderTopWidth) || 0;
  const appBorderBottom = parseFloat(appStyles.borderBottomWidth) || 0;
  const windowStyles = window.getComputedStyle(windowContent);
  const paddingTop = parseFloat(windowStyles.paddingTop) || 0;
  const paddingBottom = parseFloat(windowStyles.paddingBottom) || 0;
  const borderTop = parseFloat(windowStyles.borderTopWidth) || 0;
  const borderBottom = parseFloat(windowStyles.borderBottomWidth) || 0;
  const windowGap = parseFloat(windowStyles.rowGap || windowStyles.gap) || 0;
  const contentStyles = window.getComputedStyle(contentStack);
  const contentPaddingTop = parseFloat(contentStyles.paddingTop) || 0;
  const contentPaddingBottom = parseFloat(contentStyles.paddingBottom) || 0;
  const contentGap = parseFloat(contentStyles.rowGap || contentStyles.gap) || 0;
  const contentChildren = Array.from(contentStack.children) as HTMLElement[];
  const contentChildrenHeight = contentChildren.reduce(
    (total, element) => total + element.getBoundingClientRect().height,
    0,
  );
  const contentTotalGap =
    contentChildren.length > 1 ? contentGap * (contentChildren.length - 1) : 0;

  return Math.ceil(
    appPaddingTop +
      appPaddingBottom +
      appBorderTop +
      appBorderBottom +
      paddingTop +
      paddingBottom +
      borderTop +
      borderBottom +
      statusRow.getBoundingClientRect().height +
      windowGap +
      contentPaddingTop +
      contentPaddingBottom +
      contentChildrenHeight +
      contentTotalGap,
  );
}

function render() {
  renderStatus();

  if (!result) {
    sourceText.textContent = "等待 OCR 结果...";
    translationText.textContent = "等待 LLM 输出...";
    phrasesList.innerHTML = emptyPhrasesText();
    setEmptyState(sourceText, true);
    setEmptyState(translationText, true);
    setEmptyState(phrasesList, true);
    scheduleWindowSizeSync();
    return;
  }

  const hasSourceText = Boolean(result.sourceText);
  const hasTranslation = Boolean(result.translation);
  const hasPhrases = result.phrases.length > 0;

  sourceText.textContent = result.sourceText || "未识别到文字";
  translationText.textContent = result.translation || "未返回整句翻译。";
  phrasesList.innerHTML = renderPhrases(result.phrases);
  setEmptyState(sourceText, !hasSourceText);
  setEmptyState(translationText, !hasTranslation);
  setEmptyState(phrasesList, !hasPhrases);
  scheduleWindowSizeSync();
}

function renderStatus() {
  if (!status) {
    statusText.textContent = "等待手动解析。";
    statusText.dataset.state = "idle";
    syncActionButtons();
    return;
  }

  const suffix = status.error ? `：${status.error}` : "";
  statusText.textContent = `${status.message}${suffix}`;
  statusText.dataset.state = status.error
    ? "error"
    : status.analyzing
      ? "running"
      : "idle";
  syncActionButtons();
  scheduleWindowSizeSync();
}

function renderPhrases(phrases: SubtitlePhrase[]) {
  if (!phrases.length) {
    return "暂无逐词解释。";
  }

  return phrases
    .map(
      (item) => `
        <article class="phrase-item">
          <strong>${escapeHtml(item.source)}</strong>
          <span>${escapeHtml(item.explanation)}</span>
        </article>
      `,
    )
    .join("");
}

function setEmptyState(element: HTMLElement, empty: boolean) {
  element.classList.toggle("empty", empty);
}

function syncActionButtons() {
  const analyzing = Boolean(status?.analyzing);
  analyzeButton.disabled = analyzing;
  analyzeButton.textContent = analyzing ? "解析中..." : "解析当前字幕";
  syncRealtimeToggleButton();
}

function syncRealtimeToggleButton() {
  const realtimeEnabled = Boolean(subtitleSettings?.realtimeEnabled);
  realtimeToggleButton.dataset.enabled = realtimeEnabled ? "true" : "false";
  realtimeToggleButton.disabled = realtimeTogglePending;
  realtimeToggleButton.textContent = realtimeTogglePending
    ? "切换中..."
    : realtimeEnabled
      ? "实时解析开"
      : "实时解析关";
  realtimeToggleButton.setAttribute("aria-pressed", realtimeEnabled ? "true" : "false");
  realtimeToggleButton.title = realtimeEnabled ? "点击关闭实时解析" : "点击开启实时解析";
}

function waitForNextPaint() {
  return new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}

async function analyzeCurrentSubtitle() {
  analyzeButton.disabled = true;
  analyzeButton.textContent = "解析中...";
  await waitForNextPaint();

  try {
    await invoke("analyze_current_subtitle");
  } catch (error) {
    statusText.textContent = `解析失败：${String(error)}`;
    statusText.dataset.state = "error";
  } finally {
    syncActionButtons();
  }
}

async function toggleRealtimeParsing() {
  if (realtimeTogglePending) {
    return;
  }

  const previousSettings = subtitleSettings;
  const previousExplainMode = explainMode;

  try {
    realtimeTogglePending = true;
    syncRealtimeToggleButton();

    const latestSettings = await invoke<SubtitleExplainSettings>(
      "load_subtitle_explain_settings",
    );
    const nextSettings = {
      ...latestSettings,
      realtimeEnabled: !latestSettings.realtimeEnabled,
    };
    const savedSettings = await invoke<SubtitleExplainSettings>(
      "save_subtitle_explain_settings",
      {
        nextSettings,
      },
    );

    subtitleSettings = savedSettings;
    explainMode = savedSettings.explainMode;
    applyDisplaySettings();
    render();
  } catch (error) {
    subtitleSettings = previousSettings;
    explainMode = previousExplainMode;
    syncRealtimeToggleButton();
    statusText.textContent = `切换实时解析失败：${String(error)}`;
    statusText.dataset.state = "error";
  } finally {
    realtimeTogglePending = false;
    syncActionButtons();
  }
}

async function hideWindow() {
  try {
    await appWindow.hide();
  } catch (error) {
    statusText.textContent = `隐藏失败：${String(error)}`;
    statusText.dataset.state = "error";
  }
}

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

void init().catch((error) => {
  console.error("init subtitle explain window failed:", error);
  statusText.textContent = `初始化失败：${String(error)}`;
  statusText.dataset.state = "error";
});
