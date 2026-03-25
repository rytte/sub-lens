import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

type SubtitlePhrase = {
  source: string;
  explanation: string;
};

type SubtitleResult = {
  sourceText: string;
  translation: string;
  phrases: SubtitlePhrase[];
  error: string | null;
};

type SubtitleStatus = {
  analyzing: boolean;
  stage: string;
  message: string;
  error: string | null;
};

const EVENT_SUBTITLE_STATUS_CHANGED = "subtitle://status";
const EVENT_SUBTITLE_RESULT_CHANGED = "subtitle://result";
const appWindow = getCurrentWindow();

let status: SubtitleStatus | null = null;
let result: SubtitleResult | null = null;

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

app.innerHTML = `
  <main class="panel-shell">
    <section class="panel">
      <button class="resize-handle north" data-direction="North" aria-label="向上拉伸"></button>
      <button class="resize-handle south" data-direction="South" aria-label="向下拉伸"></button>
      <button class="resize-handle east" data-direction="East" aria-label="向右拉伸"></button>
      <button class="resize-handle west" data-direction="West" aria-label="向左拉伸"></button>
      <button class="resize-handle north-east" data-direction="NorthEast" aria-label="向右上拉伸"></button>
      <button class="resize-handle north-west" data-direction="NorthWest" aria-label="向左上拉伸"></button>
      <button class="resize-handle south-east" data-direction="SouthEast" aria-label="向右下拉伸"></button>
      <button class="resize-handle south-west" data-direction="SouthWest" aria-label="向左下拉伸"></button>

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

      <section class="block">
        <div class="block-title">原字幕</div>
        <pre id="sourceText" class="text-block empty">等待 OCR 结果...</pre>
      </section>

      <section class="block">
        <div class="block-title">整句翻译</div>
        <p id="translationText" class="paragraph empty">等待 LLM 输出...</p>
      </section>

      <section class="block">
        <div class="block-title">逐词解释</div>
        <div id="phrasesList" class="list empty">暂无逐词解释。</div>
      </section>
    </section>
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
const editOcrButton = requireElement<HTMLButtonElement>("#editOcrButton");
const analyzeButton = requireElement<HTMLButtonElement>("#analyzeButton");
const hideButton = requireElement<HTMLButtonElement>("#hideButton");
const sourceText = requireElement<HTMLElement>("#sourceText");
const translationText = requireElement<HTMLParagraphElement>("#translationText");
const phrasesList = requireElement<HTMLDivElement>("#phrasesList");
const resizeHandles = Array.from(
  document.querySelectorAll<HTMLButtonElement>("[data-direction]"),
);

async function init() {
  const [initialStatus, initialResult] = await Promise.all([
    invoke<SubtitleStatus>("load_subtitle_status"),
    invoke<SubtitleResult | null>("load_latest_subtitle_result"),
  ]);
  status = initialStatus;
  result = initialResult;
  bindUi();
  render();
  bindEvents();
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

  hideButton.addEventListener("click", () => {
    void hideWindow();
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

function render() {
  renderStatus();

  if (!result) {
    sourceText.textContent = "等待 OCR 结果...";
    translationText.textContent = "等待 LLM 输出...";
    phrasesList.innerHTML = "暂无逐词解释。";
    toggleEmptyState(true);
    return;
  }

  sourceText.textContent = result.sourceText || "未识别到文字";
  translationText.textContent = result.translation || "未返回整句翻译。";
  phrasesList.innerHTML = renderPhrases(result.phrases);
  toggleEmptyState(false);
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
}

function renderPhrases(phrases: SubtitlePhrase[]) {
  if (!phrases.length) {
    return "暂无逐词解释。";
  }

  return phrases
    .map(
      (item) => `
        <article class="list-item">
          <strong>${escapeHtml(item.source)}</strong>
          <p>${escapeHtml(item.explanation)}</p>
        </article>
      `,
    )
    .join("");
}

function toggleEmptyState(empty: boolean) {
  [sourceText, translationText, phrasesList].forEach((element) => {
    element.classList.toggle("empty", empty);
  });
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

void init();
