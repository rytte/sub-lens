import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useEffectEvent, useRef, useState } from "react";
import type { PointerEvent } from "react";

import { createAsyncCleanupScope } from "./shared/async-cleanup";
import { mountWindowApp } from "./main";
import type { OcrRegionState, OverlayResizeDirection } from "./shared/types";

const EVENT_OCR_REGION_STATE_CHANGED = "ocr-region://state";
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

export function OcrRegionWindow() {
  const [state, setState] = useState<OcrRegionState | null>(null);
  const stateRef = useRef<OcrRegionState | null>(null);
  const geometryTimerRef = useRef<number | null>(null);

  const persistState = useEffectEvent(async () => {
    const currentState = stateRef.current;

    if (!currentState) {
      return;
    }

    try {
      const isVisible = await appWindow.isVisible();

      if (!isVisible || !currentState.visible) {
        return;
      }

      const [scaleFactor, position, size] = await Promise.all([
        appWindow.scaleFactor(),
        appWindow.outerPosition(),
        appWindow.innerSize(),
      ]);
      const nextState = {
        ...currentState,
        width: size.width / scaleFactor,
        height: size.height / scaleFactor,
        x: position.x / scaleFactor,
        y: position.y / scaleFactor,
      };
      const savedState = await invoke<OcrRegionState>(
        "save_ocr_region_window_state_command",
        {
          nextState,
        },
      );

      stateRef.current = savedState;
      setState(savedState);
    } catch (error) {
      console.error("persist ocr region state failed:", error);
    }
  });

  const scheduleGeometrySync = useEffectEvent(() => {
    if (geometryTimerRef.current) {
      window.clearTimeout(geometryTimerRef.current);
    }

    geometryTimerRef.current = window.setTimeout(() => {
      geometryTimerRef.current = null;
      void persistState();
    }, 160);
  });

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  useEffect(() => {
    const cleanupScope = createAsyncCleanupScope();

    void invoke<OcrRegionState>("load_ocr_region_state")
      .then((loadedState) => {
        stateRef.current = loadedState;
        setState(loadedState);
      })
      .catch((error) => {
        console.error("load ocr region state failed:", error);
      });

    cleanupScope.register(
      listen<OcrRegionState>(EVENT_OCR_REGION_STATE_CHANGED, ({ payload }) => {
        stateRef.current = payload;
        setState(payload);
      }),
    );

    cleanupScope.register(
      appWindow.onMoved(() => {
        void appWindow.isVisible().then((visible) => {
          if (visible) {
            scheduleGeometrySync();
          }
        });
      }),
    );

    cleanupScope.register(
      appWindow.onResized(() => {
        void appWindow.isVisible().then((visible) => {
          if (visible) {
            scheduleGeometrySync();
          }
        });
      }),
    );

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" || event.key === "Enter") {
        event.preventDefault();
        void invoke("hide_ocr_region");
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      if (geometryTimerRef.current) {
        window.clearTimeout(geometryTimerRef.current);
        geometryTimerRef.current = null;
      }

      window.removeEventListener("keydown", handleKeyDown);
      cleanupScope.dispose();
    };
  }, [scheduleGeometrySync]);

  async function handleRegionPointerDown(event: PointerEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }

    if ((event.target as HTMLElement).closest(".resize-handle, .done-button")) {
      return;
    }

    await appWindow.startDragging();
  }

  async function handleResizePointerDown(
    event: PointerEvent<HTMLButtonElement>,
    direction: OverlayResizeDirection,
  ) {
    if (event.button !== 0) {
      return;
    }

    await appWindow.startResizeDragging(direction);
  }

  return (
    <main className="region-shell">
      <section
        id="region"
        className="region"
        data-editing={state?.visible ? "true" : "false"}
        data-show-border-when-idle={state?.showBorderWhenIdle ? "true" : "false"}
        onPointerDown={handleRegionPointerDown}
      >
        <header className="region-toolbar">
          <div>
            <p className="eyebrow">OCR 区域</p>
            <p className="hint">拖动移动，边缘缩放，Enter / Esc 完成，Ctrl+Alt+J 解析当前字幕。</p>
          </div>
          <button
            id="doneButton"
            className="done-button"
            type="button"
            onClick={() => {
              void invoke("hide_ocr_region");
            }}
          >
            完成
          </button>
        </header>
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

mountWindowApp(<OcrRegionWindow />);
