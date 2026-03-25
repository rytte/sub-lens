import { invoke } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type ResizeDirection = "East" | "West";
type StyleMode = "default" | "contrast";
type FontScale = "normal" | "large" | "xlarge";

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

const EVENT_SUBTITLE_STATUS_CHANGED = "subtitle://status";
const EVENT_SUBTITLE_RESULT_CHANGED = "subtitle://result";
const STYLE_MODE_STORAGE_KEY = "subtitle-explain-style-mode";
const FONT_SCALE_STORAGE_KEY = "subtitle-explain-font-scale";
const WINDOW_MIN_WIDTH = 360;
const WINDOW_MIN_HEIGHT = 140;
const WINDOW_MAX_HEIGHT = 800;
const RESIZE_SETTLE_MS = 140;
const appWindow = getCurrentWindow();

let status: SubtitleStatus | null = null;
let result: SubtitleResult | null = null;
let styleMode: StyleMode = loadStyleMode();
let fontScale: FontScale = loadFontScale();
let windowHovered = false;
let settingsOpen = false;
let sizeSyncFrame = 0;
let windowResizeObserver: ResizeObserver | null = null;
let sizeSyncTimeout = 0;

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
          <div class="settings-menu">
            <button
              id="settingsMenuButton"
              class="action-button action-button-menu"
              type="button"
              aria-haspopup="dialog"
              aria-expanded="false"
            >
              显示
              <span class="action-caret" aria-hidden="true">▾</span>
            </button>

            <div id="settingsPopover" class="settings-popover" role="dialog" aria-label="显示设置">
              <section class="settings-group">
                <p class="settings-group-title">样式</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="styleMode" data-value="default">
                    默认
                  </button>
                  <button class="settings-option" type="button" data-setting="styleMode" data-value="contrast">
                    黑字
                  </button>
                </div>
              </section>

              <section class="settings-group">
                <p class="settings-group-title">字号</p>
                <div class="settings-options">
                  <button class="settings-option" type="button" data-setting="fontScale" data-value="normal">
                    标准
                  </button>
                  <button class="settings-option" type="button" data-setting="fontScale" data-value="large">
                    偏大
                  </button>
                  <button class="settings-option" type="button" data-setting="fontScale" data-value="xlarge">
                    更大
                  </button>
                </div>
              </section>
            </div>
          </div>
          <button id="analyzeButton" class="action-button action-button-primary" type="button">
            解析当前字幕
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
          <div class="block-title">逐词解释</div>
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
const settingsMenu = requireElement<HTMLDivElement>(".settings-menu");
const settingsMenuButton = requireElement<HTMLButtonElement>("#settingsMenuButton");
const settingsPopover = requireElement<HTMLDivElement>("#settingsPopover");
const analyzeButton = requireElement<HTMLButtonElement>("#analyzeButton");
const hideButton = requireElement<HTMLButtonElement>("#hideButton");
const sourceText = requireElement<HTMLParagraphElement>("#sourceText");
const translationText = requireElement<HTMLParagraphElement>("#translationText");
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

  try {
    await appWindow.setSizeConstraints({
      minWidth: WINDOW_MIN_WIDTH,
      minHeight: WINDOW_MIN_HEIGHT,
      maxHeight: WINDOW_MAX_HEIGHT,
    });
  } catch (error) {
    console.error("set subtitle window constraints failed:", error);
  }

  try {
    const [initialStatus, initialResult] = await Promise.all([
      invoke<SubtitleStatus>("load_subtitle_status"),
      invoke<SubtitleResult | null>("load_latest_subtitle_result"),
    ]);
    status = initialStatus;
    result = initialResult;
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
  statusMeta.addEventListener("pointerdown", async (event) => {
    if (event.button !== 0) {
      return;
    }

    await appWindow.startDragging();
  });

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

  editOcrButton.addEventListener("click", () => {
    void invoke("show_ocr_region");
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
      applySetting(button.dataset.setting, button.dataset.value);
    });
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

function bindEvents() {
  void listen<SubtitleStatus>(EVENT_SUBTITLE_STATUS_CHANGED, (event) => {
    status = event.payload;
    renderStatus();
  });

  void listen<SubtitleResult>(EVENT_SUBTITLE_RESULT_CHANGED, (event) => {
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

function loadStyleMode(): StyleMode {
  try {
    return window.localStorage.getItem(STYLE_MODE_STORAGE_KEY) === "contrast"
      ? "contrast"
      : "default";
  } catch {
    return "default";
  }
}

function loadFontScale(): FontScale {
  try {
    const value = window.localStorage.getItem(FONT_SCALE_STORAGE_KEY);
    return value === "large" || value === "xlarge" ? value : "normal";
  } catch {
    return "normal";
  }
}

function applyDisplaySettings() {
  windowContent.dataset.styleMode = styleMode;
  windowContent.dataset.fontScale = fontScale;
  windowContent.dataset.hovered =
    styleMode === "contrast" && windowHovered ? "true" : "false";
  syncSettingsOptions();
  syncSettingsPopover();
}

function syncSettingsOptions() {
  settingsOptionButtons.forEach((button) => {
    const { setting, value } = button.dataset;
    const active =
      (setting === "styleMode" && value === styleMode) ||
      (setting === "fontScale" && value === fontScale);
    button.dataset.active = active ? "true" : "false";
  });
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

function applySetting(setting?: string, value?: string) {
  if (!setting || !value) {
    return;
  }

  if (setting === "styleMode" && (value === "default" || value === "contrast")) {
    styleMode = value;
    persistSetting(STYLE_MODE_STORAGE_KEY, styleMode);
  }

  if (setting === "fontScale" && (value === "normal" || value === "large" || value === "xlarge")) {
    fontScale = value;
    persistSetting(FONT_SCALE_STORAGE_KEY, fontScale);
  }

  applyDisplaySettings();
  scheduleWindowSizeSync();
}

function persistSetting(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures and keep the current in-memory selection.
  }
}

function setWindowHovered(nextHovered: boolean) {
  if (windowHovered === nextHovered) {
    return;
  }

  windowHovered = nextHovered;
  if (styleMode === "contrast") {
    windowContent.dataset.hovered = nextHovered ? "true" : "false";
  }
}

async function syncWindowSize() {
  try {
    const targetHeight = Math.max(
      WINDOW_MIN_HEIGHT,
      Math.min(WINDOW_MAX_HEIGHT, getNaturalWindowHeight()),
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
    phrasesList.innerHTML = "暂无逐词解释。";
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
}

async function analyzeCurrentSubtitle() {
  analyzeButton.disabled = true;
  analyzeButton.textContent = "解析中...";

  try {
    await invoke("analyze_current_subtitle");
  } catch (error) {
    statusText.textContent = `解析失败：${String(error)}`;
    statusText.dataset.state = "error";
  } finally {
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
