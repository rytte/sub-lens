import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

type OverlayAppearance = "pixelate" | "solid";
type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

type OverlayState = {
  visible: boolean;
  locked: boolean;
  clickThroughWhenLocked: boolean;
  alwaysOnTop: boolean;
  autoCenter: boolean;
  refreshRate: number;
  pixelDetail: "coarse" | "medium" | "fine";
  opacity: number;
  appearance: OverlayAppearance;
  color: string;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
};

const EVENT_STATE_CHANGED = "overlay://state";
const EVENT_PIXEL_FRAME = "overlay://pixel-frame";
const appWindow = getCurrentWindow();

let state: OverlayState;
let geometryTimer: number | undefined;

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

app.innerHTML = `
  <main class="shell">
    <section id="overlay" class="overlay" data-appearance="pixelate">
      <div id="pixelLayer" class="pixel-layer" aria-hidden="true"></div>
      <div class="drag-handle">
        <div class="mode-chip">
          <span class="mode-dot"></span>
          <span id="modeLabel">编辑中</span>
        </div>
        <p id="hintText" class="hint-text">右键打开系统菜单 · 拖动移动 · 边缘缩放 · Ctrl+Alt+H 显示隐藏</p>
      </div>
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

const overlayEl = requireElement<HTMLElement>("#overlay");
const pixelLayerEl = requireElement<HTMLElement>("#pixelLayer");
const modeLabelEl = requireElement<HTMLElement>("#modeLabel");
const hintTextEl = requireElement<HTMLElement>("#hintText");

const resizeHandles = Array.from(
  document.querySelectorAll<HTMLButtonElement>("[data-direction]"),
);

async function init() {
  state = await invoke<OverlayState>("load_overlay_state");
  render();
  bindUi();
  bindWindowEvents();
}

function bindUi() {
  overlayEl.addEventListener("pointerdown", async (event) => {
    if (state.locked || event.button !== 0) {
      return;
    }

    if ((event.target as HTMLElement).closest(".resize-handle")) {
      return;
    }

    await appWindow.startDragging();
  });

  resizeHandles.forEach((handle) => {
    handle.addEventListener("pointerdown", async (event) => {
      if (state.locked || event.button !== 0) {
        return;
      }

      const direction = handle.dataset.direction as ResizeDirection;
      await appWindow.startResizeDragging(direction);
    });
  });

  document.addEventListener("contextmenu", (event) => {
    event.preventDefault();

    if (state.locked && state.clickThroughWhenLocked) {
      return;
    }

    void invoke("show_overlay_menu", {
      x: event.clientX,
      y: event.clientY,
    });
  });
}

function bindWindowEvents() {
  void listen<OverlayState>(EVENT_STATE_CHANGED, (event) => {
    state = event.payload;
    render();
  });

  void listen<{ dataUrl: string }>(EVENT_PIXEL_FRAME, (event) => {
    if (state.appearance !== "pixelate") {
      return;
    }

    pixelLayerEl.style.backgroundImage = `url("${event.payload.dataUrl}")`;
  });

  void appWindow.onMoved(async ({ payload }) => {
    const scaleFactor = await appWindow.scaleFactor();
    state.x = state.autoCenter ? null : payload.x / scaleFactor;
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

function render() {
  document.documentElement.dataset.locked = state.locked ? "true" : "false";
  overlayEl.dataset.appearance = state.appearance;
  overlayEl.style.setProperty("--overlay-rgb", hexToRgbTriplet(state.color));
  overlayEl.style.setProperty("--overlay-opacity", state.opacity.toFixed(2));
  pixelLayerEl.style.opacity =
    state.appearance === "pixelate" ? state.opacity.toFixed(2) : "0";

  if (state.appearance !== "pixelate") {
    pixelLayerEl.style.backgroundImage = "";
  }

  modeLabelEl.textContent = state.locked ? "锁定中" : "编辑中";
  hintTextEl.textContent = state.locked
    ? state.clickThroughWhenLocked
      ? "锁定且已穿透 · 从托盘菜单恢复编辑"
      : "锁定但不穿透 · 右键仍可打开菜单"
    : state.autoCenter
      ? "自动水平居中已开启 · 可上下拖动 · Ctrl+Alt+H 显示隐藏"
      : "右键打开系统菜单 · 拖动移动 · 边缘缩放 · Ctrl+Alt+H 显示隐藏";
}

function scheduleGeometrySync() {
  if (geometryTimer) {
    window.clearTimeout(geometryTimer);
  }

  geometryTimer = window.setTimeout(() => {
    void persistState();
  }, 180);
}

async function persistState() {
  state = await invoke<OverlayState>("apply_overlay_state", {
    nextState: state,
  });
  render();
}

function normalizeHex(value: string) {
  if (!value.startsWith("#")) {
    return "#0d1117";
  }

  return value.length === 7 ? value.toLowerCase() : "#0d1117";
}

function hexToRgbTriplet(hex: string) {
  const normalized = normalizeHex(hex).replace("#", "");
  const red = Number.parseInt(normalized.slice(0, 2), 16);
  const green = Number.parseInt(normalized.slice(2, 4), 16);
  const blue = Number.parseInt(normalized.slice(4, 6), 16);
  return `${red} ${green} ${blue}`;
}

void init();
