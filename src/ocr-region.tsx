import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useEffectEvent, useRef, useState } from "react";
import type { CSSProperties, MouseEvent, PointerEvent } from "react";

import { createAsyncCleanupScope } from "./shared/async-cleanup";
import { mountWindowApp } from "./main";
import type { OcrRegionState, OverlayResizeDirection } from "./shared/types";

const EVENT_OCR_REGION_STATE_CHANGED = "ocr-region://state";
const EVENT_OCR_REGION_DRAW_REQUEST = "ocr-region://draw-request";
const MIN_DRAW_SELECTION_WIDTH = 8;
const MIN_DRAW_SELECTION_HEIGHT = 8;
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

type OcrRegionDrawRequest = {
  mode: "setRegion" | "analyzeStatic";
  viewportX: number;
  viewportY: number;
  viewportWidth: number;
  viewportHeight: number;
  backgroundImagePath: string | null;
};

type DrawPoint = {
  x: number;
  y: number;
};

type DrawRect = DrawPoint & {
  width: number;
  height: number;
};

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function normalizeDrawRect(start: DrawPoint, end: DrawPoint): DrawRect {
  const left = Math.min(start.x, end.x);
  const top = Math.min(start.y, end.y);
  const right = Math.max(start.x, end.x);
  const bottom = Math.max(start.y, end.y);

  return {
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  };
}

export function OcrRegionWindow() {
  const [state, setState] = useState<OcrRegionState | null>(null);
  const [drawRequest, setDrawRequest] = useState<OcrRegionDrawRequest | null>(null);
  const [drawRect, setDrawRect] = useState<DrawRect | null>(null);

  const stateRef = useRef<OcrRegionState | null>(null);
  const drawRequestRef = useRef<OcrRegionDrawRequest | null>(null);
  const drawRequestFetchPendingRef = useRef(false);
  const drawSubmissionPendingRef = useRef(false);
  const geometryTimerRef = useRef<number | null>(null);
  const drawOriginRef = useRef<DrawPoint | null>(null);
  const drawPointerIdRef = useRef<number | null>(null);

  const beginDrawingSession = useEffectEvent((payload: OcrRegionDrawRequest) => {
    drawOriginRef.current = null;
    drawPointerIdRef.current = null;
    drawRequestRef.current = payload;
    setDrawRect(null);
    setDrawRequest(payload);
  });

  const resetDrawingSession = useEffectEvent(() => {
    drawOriginRef.current = null;
    drawPointerIdRef.current = null;
    drawRequestRef.current = null;
    setDrawRequest(null);
    setDrawRect(null);
  });

  const persistState = useEffectEvent(async () => {
    const currentState = stateRef.current;

    if (!currentState || drawRequestRef.current || drawSubmissionPendingRef.current) {
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

  const ensureDrawingSession = useEffectEvent(async () => {
    if (
      drawRequestRef.current ||
      drawRequestFetchPendingRef.current ||
      drawSubmissionPendingRef.current
    ) {
      return;
    }

    drawRequestFetchPendingRef.current = true;

    try {
      const pendingDrawRequest = await invoke<OcrRegionDrawRequest | null>(
        "load_ocr_region_draw_request",
      );

      if (pendingDrawRequest && !drawRequestRef.current) {
        beginDrawingSession(pendingDrawRequest);
      }
    } catch (error) {
      console.error("load ocr region draw request failed:", error);
    } finally {
      drawRequestFetchPendingRef.current = false;
    }
  });

  const cancelDrawing = useEffectEvent(async () => {
    try {
      const restoredState = await invoke<OcrRegionState>("cancel_ocr_region_drawing");
      stateRef.current = restoredState;
      setState(restoredState);
      resetDrawingSession();
    } catch (error) {
      console.error("cancel ocr region drawing failed:", error);
    }
  });

  const finishDrawing = useEffectEvent(async (nextDrawRect: DrawRect) => {
    const currentDrawRequest = drawRequestRef.current;

    if (!currentDrawRequest) {
      return;
    }

    try {
      if (currentDrawRequest.mode === "analyzeStatic") {
        drawSubmissionPendingRef.current = true;
        resetDrawingSession();
        await invoke("submit_static_screen_selection", {
          selection: nextDrawRect,
        });
        const latestState = await invoke<OcrRegionState>("load_ocr_region_state");
        stateRef.current = latestState;
        setState(latestState);
        return;
      }

      const currentState =
        stateRef.current ?? (await invoke<OcrRegionState>("load_ocr_region_state"));
      const savedState = await invoke<OcrRegionState>("apply_ocr_region_state", {
        nextState: {
          ...currentState,
          visible: true,
          x: currentDrawRequest.viewportX + nextDrawRect.x,
          y: currentDrawRequest.viewportY + nextDrawRect.y,
          width: nextDrawRect.width,
          height: nextDrawRect.height,
        },
      });

      stateRef.current = savedState;
      setState(savedState);
      resetDrawingSession();
    } catch (error) {
      if (currentDrawRequest.mode === "analyzeStatic") {
        try {
          const latestState = await invoke<OcrRegionState>("load_ocr_region_state");
          stateRef.current = latestState;
          setState(latestState);
        } catch (stateError) {
          console.error("reload ocr region state after static submit failed:", stateError);
        }
      }
      console.error("finish ocr region drawing failed:", error);
    } finally {
      if (currentDrawRequest.mode === "analyzeStatic") {
        drawSubmissionPendingRef.current = false;
      }
    }
  });

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  useEffect(() => {
    drawRequestRef.current = drawRequest;
  }, [drawRequest]);

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
      listen<OcrRegionDrawRequest>(EVENT_OCR_REGION_DRAW_REQUEST, ({ payload }) => {
        beginDrawingSession(payload);
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
      if (drawRequestRef.current) {
        if (event.key === "Escape") {
          event.preventDefault();
          void cancelDrawing();
        }

        return;
      }

      if (event.key === "Escape" || event.key === "Enter") {
        event.preventDefault();
        void invoke("hide_ocr_region");
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    void ensureDrawingSession();

    return () => {
      if (geometryTimerRef.current) {
        window.clearTimeout(geometryTimerRef.current);
        geometryTimerRef.current = null;
      }

      window.removeEventListener("keydown", handleKeyDown);
      cleanupScope.dispose();
    };
  }, [beginDrawingSession, cancelDrawing, ensureDrawingSession, scheduleGeometrySync]);

  useEffect(() => {
    if (!state || drawRequest || drawSubmissionPendingRef.current) {
      return;
    }

    void ensureDrawingSession();
  }, [drawRequest, ensureDrawingSession, state]);

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

  function getDrawPoint(event: PointerEvent<HTMLElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();

    return {
      x: clamp(event.clientX - bounds.left, 0, bounds.width),
      y: clamp(event.clientY - bounds.top, 0, bounds.height),
    };
  }

  function handleDrawingPointerDown(event: PointerEvent<HTMLElement>) {
    if (!drawRequestRef.current) {
      return;
    }

    if (event.button === 2) {
      event.preventDefault();
      void cancelDrawing();
      return;
    }

    if (event.button !== 0) {
      return;
    }

    event.preventDefault();

    const nextOrigin = getDrawPoint(event);

    drawOriginRef.current = nextOrigin;
    drawPointerIdRef.current = event.pointerId;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrawRect({
      x: nextOrigin.x,
      y: nextOrigin.y,
      width: 0,
      height: 0,
    });
  }

  function handleDrawingPointerMove(event: PointerEvent<HTMLElement>) {
    if (
      !drawRequestRef.current ||
      drawPointerIdRef.current !== event.pointerId ||
      !drawOriginRef.current
    ) {
      return;
    }

    setDrawRect(normalizeDrawRect(drawOriginRef.current, getDrawPoint(event)));
  }

  function handleDrawingContextMenu(event: MouseEvent<HTMLElement>) {
    event.preventDefault();
  }

  function handleDrawingPointerUp(event: PointerEvent<HTMLElement>) {
    if (
      !drawRequestRef.current ||
      drawPointerIdRef.current !== event.pointerId ||
      !drawOriginRef.current
    ) {
      return;
    }

    const nextDrawRect = normalizeDrawRect(drawOriginRef.current, getDrawPoint(event));

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }

    drawOriginRef.current = null;
    drawPointerIdRef.current = null;

    if (
      nextDrawRect.width < MIN_DRAW_SELECTION_WIDTH ||
      nextDrawRect.height < MIN_DRAW_SELECTION_HEIGHT
    ) {
      setDrawRect(null);
      return;
    }

    setDrawRect(nextDrawRect);
    void finishDrawing(nextDrawRect);
  }

  function handleDrawingPointerCancel() {
    drawOriginRef.current = null;
    drawPointerIdRef.current = null;
    setDrawRect(null);
  }

  const drawRectStyle = drawRect
    ? ({
        left: drawRect.x,
        top: drawRect.y,
        width: drawRect.width,
        height: drawRect.height,
      } as CSSProperties)
    : undefined;
  const drawIdleLabel =
    drawRequest?.mode === "analyzeStatic"
      ? "按住鼠标左键拖拽划词翻译，松开后直接识别解析，右键或 Esc 取消"
      : "按住鼠标左键拖拽选择 OCR 区域，右键或 Esc 取消";
  const drawSizeLabel = drawRect
    ? `${Math.round(drawRect.width)} × ${Math.round(drawRect.height)}`
    : drawIdleLabel;
  const drawBackgroundSrc = drawRequest?.backgroundImagePath
    ? convertFileSrc(drawRequest.backgroundImagePath)
    : null;

  if (drawRequest) {
    return (
      <main className="region-shell" data-mode="drawing">
        <section
          className="drawing-surface"
          onContextMenu={handleDrawingContextMenu}
          onPointerDown={handleDrawingPointerDown}
          onPointerMove={handleDrawingPointerMove}
          onPointerUp={handleDrawingPointerUp}
          onPointerCancel={handleDrawingPointerCancel}
        >
          {drawBackgroundSrc ? (
            <img
              className="drawing-background"
              src={drawBackgroundSrc}
              alt=""
              draggable={false}
            />
          ) : null}
          <div className="drawing-hint">{drawSizeLabel}</div>
          {drawRect ? <div className="drawing-box" style={drawRectStyle}></div> : null}
        </section>
      </main>
    );
  }

  return (
    <main className="region-shell" data-mode="edit">
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
            <p className="hint">拖动移动，边缘缩放，Enter / Esc 完成；相关快捷键可在更多设置中修改。</p>
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
