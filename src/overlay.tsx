import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useEffectEvent, useRef, useState } from "react";
import type { CSSProperties, MouseEvent, PointerEvent } from "react";

import { createAsyncCleanupScope } from "./shared/async-cleanup";
import { mountWindowApp } from "./main";
import type { OverlayResizeDirection, OverlayState } from "./shared/types";

const EVENT_STATE_CHANGED = "overlay://state";
const EVENT_PIXEL_FRAME = "overlay://pixel-frame";
const appWindow = getCurrentWindow();

const RESIZE_HANDLES: Array<{
  direction: OverlayResizeDirection;
  className: string;
  ariaLabel: string;
}> = [
  { direction: "North", className: "north", ariaLabel: "向上调整" },
  { direction: "South", className: "south", ariaLabel: "向下调整" },
  { direction: "East", className: "east", ariaLabel: "向右调整" },
  { direction: "West", className: "west", ariaLabel: "向左调整" },
  { direction: "NorthEast", className: "north-east", ariaLabel: "向右上调整" },
  { direction: "NorthWest", className: "north-west", ariaLabel: "向左上调整" },
  { direction: "SouthEast", className: "south-east", ariaLabel: "向右下调整" },
  { direction: "SouthWest", className: "south-west", ariaLabel: "向左下调整" },
];

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

function overlayModeLabel(state: OverlayState) {
  return state.locked ? "锁定中" : "编辑中";
}

function overlayHintText(state: OverlayState) {
  if (state.locked && state.clickThroughWhenLocked) {
    return "锁定并已穿透，可从托盘菜单恢复编辑。";
  }

  if (state.locked) {
    return "锁定但未穿透，右键仍可打开系统菜单。";
  }

  if (state.autoCenter) {
    return "自动水平居中已开启，可上下拖动，Ctrl+Shift+E 编辑 OCR 区域。";
  }

  return "右键打开菜单。";
}

export function OverlayWindow() {
  const [state, setState] = useState<OverlayState | null>(null);
  const [pixelFrame, setPixelFrame] = useState("");
  const stateRef = useRef<OverlayState | null>(null);
  const geometryTimerRef = useRef<number | null>(null);
  const interactionNotifyAtRef = useRef(0);

  const persistState = useEffectEvent(async () => {
    const currentState = stateRef.current;

    if (!currentState) {
      return;
    }

    try {
      const [scaleFactor, position, size] = await Promise.all([
        appWindow.scaleFactor(),
        appWindow.outerPosition(),
        appWindow.innerSize(),
      ]);
      const nextState = {
        ...currentState,
        width: size.width / scaleFactor,
        height: size.height / scaleFactor,
        x: currentState.autoCenter ? null : position.x / scaleFactor,
        y: position.y / scaleFactor,
      };
      const savedState = await invoke<OverlayState>("apply_overlay_state", {
        nextState,
      });

      stateRef.current = savedState;
      setState(savedState);
    } catch (error) {
      console.error("persist overlay state failed:", error);
    }
  });

  const notifyInteraction = useEffectEvent(() => {
    const now = Date.now();

    if (now - interactionNotifyAtRef.current < 120) {
      return;
    }

    interactionNotifyAtRef.current = now;
    void invoke("mark_overlay_interacting", {
      durationMs: 360,
    }).catch((error) => {
      console.error("mark overlay interaction failed:", error);
    });
  });

  const scheduleGeometrySync = useEffectEvent(() => {
    notifyInteraction();

    if (geometryTimerRef.current) {
      window.clearTimeout(geometryTimerRef.current);
    }

    geometryTimerRef.current = window.setTimeout(() => {
      geometryTimerRef.current = null;
      void persistState();
    }, 180);
  });

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  useEffect(() => {
    if (!state) {
      delete document.documentElement.dataset.locked;
      return undefined;
    }

    document.documentElement.dataset.locked = state.locked ? "true" : "false";

    return () => {
      delete document.documentElement.dataset.locked;
    };
  }, [state]);

  useEffect(() => {
    const cleanupScope = createAsyncCleanupScope();

    void invoke<OverlayState>("load_overlay_state")
      .then((loadedState) => {
        stateRef.current = loadedState;
        setState(loadedState);
      })
      .catch((error) => {
        console.error("load overlay state failed:", error);
      });

    cleanupScope.register(
      listen<OverlayState>(EVENT_STATE_CHANGED, ({ payload }) => {
        stateRef.current = payload;
        setState(payload);
      }),
    );

    cleanupScope.register(
      listen<{ dataUrl: string }>(EVENT_PIXEL_FRAME, ({ payload }) => {
        if (stateRef.current?.appearance !== "pixelate") {
          return;
        }

        setPixelFrame(payload.dataUrl);
      }),
    );

    cleanupScope.register(
      appWindow.onMoved(() => {
        scheduleGeometrySync();
      }),
    );

    cleanupScope.register(
      appWindow.onResized(() => {
        scheduleGeometrySync();
      }),
    );

    return () => {
      if (geometryTimerRef.current) {
        window.clearTimeout(geometryTimerRef.current);
        geometryTimerRef.current = null;
      }

      cleanupScope.dispose();
    };
  }, [scheduleGeometrySync]);

  async function handleOverlayPointerDown(event: PointerEvent<HTMLElement>) {
    if (!state || state.locked || event.button !== 0) {
      return;
    }

    if ((event.target as HTMLElement).closest(".resize-handle")) {
      return;
    }

    notifyInteraction();
    await appWindow.startDragging();
  }

  async function handleResizePointerDown(
    event: PointerEvent<HTMLButtonElement>,
    direction: OverlayResizeDirection,
  ) {
    if (!state || state.locked || event.button !== 0) {
      return;
    }

    notifyInteraction();
    await appWindow.startResizeDragging(direction);
  }

  function handleContextMenu(event: MouseEvent<HTMLElement>) {
    event.preventDefault();

    if (!state || (state.locked && state.clickThroughWhenLocked)) {
      return;
    }

    void invoke("show_overlay_menu", {
      x: event.clientX,
      y: event.clientY,
    });
  }

  const overlayStyle = state
    ? ({
        "--overlay-rgb": hexToRgbTriplet(state.color),
        "--overlay-opacity": state.opacity.toFixed(2),
      } as CSSProperties)
    : undefined;

  const pixelLayerStyle = state
    ? ({
        opacity: state.appearance === "pixelate" ? state.opacity.toFixed(2) : "0",
        backgroundImage:
          state.appearance === "pixelate" && pixelFrame ? `url("${pixelFrame}")` : "",
      } as CSSProperties)
    : undefined;

  return (
    <main className="shell" onContextMenu={handleContextMenu}>
      <section
        id="overlay"
        className="overlay"
        data-appearance={state?.appearance ?? "pixelate"}
        style={overlayStyle}
        onPointerDown={handleOverlayPointerDown}
      >
        <div id="pixelLayer" className="pixel-layer" aria-hidden="true" style={pixelLayerStyle} />
        <div className="drag-handle">
          <div className="mode-chip">
            <span className="mode-dot"></span>
            <span id="modeLabel">{state ? overlayModeLabel(state) : "加载中"}</span>
          </div>
          <p id="hintText" className="hint-text">
            {state ? overlayHintText(state) : "正在加载字幕遮挡状态..."}
          </p>
        </div>
        {RESIZE_HANDLES.map((handle) => (
          <button
            key={handle.direction}
            className={`resize-handle ${handle.className}`}
            data-direction={handle.direction}
            aria-label={handle.ariaLabel}
            type="button"
            onPointerDown={(event) => {
              void handleResizePointerDown(event, handle.direction);
            }}
          ></button>
        ))}
      </section>
    </main>
  );
}

mountWindowApp(<OverlayWindow />);
