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

type OcrRegionState = {
  visible: boolean;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
};

const EVENT_OCR_REGION_STATE_CHANGED = "ocr-region://state";
const appWindow = getCurrentWindow();

let state: OcrRegionState;
let geometryTimer: number | undefined;

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

app.innerHTML = `
  <main class="region-shell">
    <section id="region" class="region">
      <header class="region-toolbar">
        <div>
          <p class="eyebrow">OCR 区域</p>
          <p class="hint">拖动移动 · 边缘缩放 · Enter / Esc 完成 · Ctrl+Alt+J 解析当前字幕</p>
        </div>
        <button id="doneButton" class="done-button" type="button">完成</button>
      </header>
      <button class="resize-handle north" data-direction="North" aria-label="向上调整"></button>
      <button class="resize-handle south" data-direction="South" aria-label="向下调整"></button>
      <button class="resize-handle east" data-direction="East" aria-label="向右调整"></button>
      <button class="resize-handle west" data-direction="West" aria-label="向左调整"></button>
      <button class="resize-handle north-east" data-direction="NorthEast" aria-label="向右上调整"></button>
      <button class="resize-handle north-west" data-direction="NorthWest" aria-label="向左上调整"></button>
      <button class="resize-handle south-east" data-direction="SouthEast" aria-label="向右下调整"></button>
      <button class="resize-handle south-west" data-direction="SouthWest" aria-label="向左下调整"></button>
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

const regionEl = requireElement<HTMLElement>("#region");
const doneButton = requireElement<HTMLButtonElement>("#doneButton");
const resizeHandles = Array.from(
  document.querySelectorAll<HTMLButtonElement>("[data-direction]"),
);

async function init() {
  state = await invoke<OcrRegionState>("load_ocr_region_state");
  bindUi();
  bindWindowEvents();
}

function bindUi() {
  regionEl.addEventListener("pointerdown", async (event) => {
    if (event.button !== 0) {
      return;
    }

    if ((event.target as HTMLElement).closest(".resize-handle, .done-button")) {
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

  doneButton.addEventListener("click", () => {
    void invoke("hide_ocr_region");
  });

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" || event.key === "Enter") {
      event.preventDefault();
      void invoke("hide_ocr_region");
    }
  });
}

function bindWindowEvents() {
  void listen<OcrRegionState>(EVENT_OCR_REGION_STATE_CHANGED, (event) => {
    state = event.payload;
  });

  void appWindow.onMoved(async ({ payload }) => {
    const scaleFactor = await appWindow.scaleFactor();
    state.x = payload.x / scaleFactor;
    state.y = payload.y / scaleFactor;
    scheduleGeometrySync();
  });

  void appWindow.onResized(async ({ payload }) => {
    const scaleFactor = await appWindow.scaleFactor();
    state.width = payload.width / scaleFactor;
    state.height = payload.height / scaleFactor;
    scheduleGeometrySync();
  });
}

function scheduleGeometrySync() {
  if (geometryTimer) {
    window.clearTimeout(geometryTimer);
  }

  geometryTimer = window.setTimeout(() => {
    void persistState();
  }, 160);
}

async function persistState() {
  state = await invoke<OcrRegionState>("apply_ocr_region_state", {
    nextState: state,
  });
}

void init();
