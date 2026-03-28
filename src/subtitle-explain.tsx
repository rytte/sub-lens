import { invoke } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";
import type { CSSProperties, PointerEvent } from "react";

import { createAsyncCleanupScope } from "./shared/async-cleanup";
import { mountWindowApp } from "./main";
import {
  DEFAULT_FONT_SCALE,
  FONT_SCALE_VALUES,
  LEGACY_FONT_SCALE_MAP,
} from "./shared/types";
import type {
  FontScale,
  OcrRegionState,
  StyleMode,
  SubtitleExplainMode,
  SubtitleExplainResizeDirection,
  SubtitleExplainSettings,
  SubtitleExplainWindowState,
  SubtitlePhrase,
  SubtitleResult,
  SubtitleStatus,
} from "./shared/types";

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
const RESIZE_HANDLES: Array<{
  direction: SubtitleExplainResizeDirection;
  className: string;
  ariaLabel: string;
}> = [
  { direction: "East", className: "east", ariaLabel: "向右拉伸" },
  { direction: "West", className: "west", ariaLabel: "向左拉伸" },
  { direction: "South", className: "south", ariaLabel: "向下拉伸" },
  { direction: "SouthEast", className: "south-east", ariaLabel: "向右下拉伸" },
  { direction: "SouthWest", className: "south-west", ariaLabel: "向左下拉伸" },
];

const EXPLAIN_MODE_OPTIONS: Array<{
  value: SubtitleExplainMode;
  label: string;
}> = [
  { value: "word", label: "逐词" },
  { value: "phrase", label: "短语" },
];

const STYLE_MODE_OPTIONS: Array<{
  value: StyleMode;
  label: string;
}> = [
  { value: "default", label: "默认" },
  { value: "contrast", label: "黑字" },
  { value: "contrastLight", label: "白字" },
];

const BINARY_OPTIONS = {
  contrastOutline: [
    { value: "disabled", label: "关闭" },
    { value: "enabled", label: "开启" },
  ],
  autoHeight: [
    { value: "disabled", label: "固定" },
    { value: "enabled", label: "自适应" },
  ],
  ocrBorderMode: [
    { value: "editOnly", label: "仅编辑时" },
    { value: "alwaysVisible", label: "常显" },
  ],
} as const;

function persistSetting(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures and keep the in-memory selection.
  }
}

function loadStyleMode(): StyleMode {
  try {
    const storedValue = window.localStorage.getItem(STYLE_MODE_STORAGE_KEY);

    if (storedValue === "contrast" || storedValue === "contrastLight") {
      return storedValue;
    }
  } catch {
    return "default";
  }

  return "default";
}

function isFontScale(value: string): value is FontScale {
  return FONT_SCALE_VALUES.includes(value as FontScale);
}

function loadFontScale(): FontScale {
  try {
    const storedValue = window.localStorage.getItem(FONT_SCALE_STORAGE_KEY);

    if (storedValue && isFontScale(storedValue)) {
      return storedValue;
    }

    if (storedValue === "normal" || storedValue === "large" || storedValue === "xlarge") {
      const migratedValue = LEGACY_FONT_SCALE_MAP[storedValue];

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

function phrasesSectionTitle(explainMode: SubtitleExplainMode) {
  return explainMode === "word" ? "逐词解释" : "短语解释";
}

function emptyPhrasesText(explainMode: SubtitleExplainMode) {
  return explainMode === "word" ? "暂无逐词解释。" : "暂无短语解释。";
}

function statusMessage(status: SubtitleStatus | null) {
  if (!status) {
    return "等待手动解析。";
  }

  return status.error ? `${status.message}（${status.error}）` : status.message;
}

function statusVisualState(status: SubtitleStatus | null) {
  if (!status) {
    return "idle";
  }

  if (status.error) {
    return "error";
  }

  return status.analyzing ? "running" : "idle";
}

function waitForNextPaint() {
  return new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}

export function SubtitleExplainWindow() {
  const [status, setSubtitleStatus] = useState<SubtitleStatus | null>(null);
  const [result, setResult] = useState<SubtitleResult | null>(null);
  const [styleMode, setStyleMode] = useState<StyleMode>(loadStyleMode);
  const [contrastOutlineEnabled, setContrastOutlineEnabled] = useState(
    loadContrastOutlineEnabled,
  );
  const [fontScale, setFontScale] = useState<FontScale>(loadFontScale);
  const [autoHeightEnabled, setAutoHeightEnabled] = useState(loadAutoHeightEnabled);
  const [explainMode, setExplainMode] = useState<SubtitleExplainMode>("word");
  const [subtitleSettings, setSubtitleSettings] =
    useState<SubtitleExplainSettings | null>(null);
  const [ocrRegionState, setOcrRegionState] = useState<OcrRegionState | null>(null);
  const [windowHovered, setWindowHovered] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [manualAnalyzePending, setManualAnalyzePending] = useState(false);
  const [realtimeTogglePending, setRealtimeTogglePending] = useState(false);

  const autoHeightEnabledRef = useRef(autoHeightEnabled);
  const settingsMenuRef = useRef<HTMLDivElement | null>(null);
  const windowContentRef = useRef<HTMLElement | null>(null);
  const statusRowRef = useRef<HTMLDivElement | null>(null);
  const contentStackRef = useRef<HTMLDivElement | null>(null);
  const sizeSyncFrameRef = useRef(0);
  const sizeSyncTimeoutRef = useRef(0);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);
  const windowStatePersistTimerRef = useRef(0);

  useEffect(() => {
    autoHeightEnabledRef.current = autoHeightEnabled;
  }, [autoHeightEnabled]);

  const cancelWindowSizeSync = useEffectEvent(() => {
    if (sizeSyncTimeoutRef.current) {
      window.clearTimeout(sizeSyncTimeoutRef.current);
      sizeSyncTimeoutRef.current = 0;
    }

    if (sizeSyncFrameRef.current) {
      window.cancelAnimationFrame(sizeSyncFrameRef.current);
      sizeSyncFrameRef.current = 0;
    }
  });

  const getNaturalWindowHeight = useEffectEvent(() => {
    const windowContent = windowContentRef.current;
    const statusRow = statusRowRef.current;
    const contentStack = contentStackRef.current;

    if (!windowContent || !statusRow || !contentStack) {
      return WINDOW_MIN_HEIGHT;
    }

    const appRootStyles = window.getComputedStyle(document.documentElement);
    const windowStyles = window.getComputedStyle(windowContent);
    const contentStyles = window.getComputedStyle(contentStack);
    const appPaddingTop = parseFloat(appRootStyles.paddingTop) || 0;
    const appPaddingBottom = parseFloat(appRootStyles.paddingBottom) || 0;
    const appBorderTop = parseFloat(appRootStyles.borderTopWidth) || 0;
    const appBorderBottom = parseFloat(appRootStyles.borderBottomWidth) || 0;
    const windowPaddingTop = parseFloat(windowStyles.paddingTop) || 0;
    const windowPaddingBottom = parseFloat(windowStyles.paddingBottom) || 0;
    const windowBorderTop = parseFloat(windowStyles.borderTopWidth) || 0;
    const windowBorderBottom = parseFloat(windowStyles.borderBottomWidth) || 0;
    const windowGap = parseFloat(windowStyles.rowGap || windowStyles.gap) || 0;
    const contentPaddingTop = parseFloat(contentStyles.paddingTop) || 0;
    const contentPaddingBottom = parseFloat(contentStyles.paddingBottom) || 0;
    const contentGap = parseFloat(contentStyles.rowGap || contentStyles.gap) || 0;
    const contentChildren = Array.from(contentStack.children) as HTMLElement[];
    const contentChildrenHeight = contentChildren.reduce((totalHeight, element) => {
      return totalHeight + element.getBoundingClientRect().height;
    }, 0);
    const contentTotalGap =
      contentChildren.length > 1 ? contentGap * (contentChildren.length - 1) : 0;

    return Math.ceil(
      appPaddingTop +
        appPaddingBottom +
        appBorderTop +
        appBorderBottom +
        windowPaddingTop +
        windowPaddingBottom +
        windowBorderTop +
        windowBorderBottom +
        statusRow.getBoundingClientRect().height +
        windowGap +
        contentPaddingTop +
        contentPaddingBottom +
        contentChildrenHeight +
        contentTotalGap,
    );
  });

  const syncWindowSize = useEffectEvent(async () => {
    if (!autoHeightEnabledRef.current) {
      return;
    }

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
  });

  const scheduleWindowSizeSync = useEffectEvent((delay = 0) => {
    if (!autoHeightEnabledRef.current) {
      cancelWindowSizeSync();
      return;
    }

    if (sizeSyncTimeoutRef.current) {
      window.clearTimeout(sizeSyncTimeoutRef.current);
      sizeSyncTimeoutRef.current = 0;
    }

    if (delay > 0) {
      sizeSyncTimeoutRef.current = window.setTimeout(() => {
        sizeSyncTimeoutRef.current = 0;
        scheduleWindowSizeSync();
      }, delay);
      return;
    }

    if (sizeSyncFrameRef.current) {
      return;
    }

    sizeSyncFrameRef.current = window.requestAnimationFrame(() => {
      sizeSyncFrameRef.current = 0;
      void syncWindowSize();
    });
  });

  const syncWindowSizeConstraints = useEffectEvent(async () => {
    try {
      await appWindow.setSizeConstraints({
        minWidth: WINDOW_MIN_WIDTH,
        minHeight: WINDOW_MIN_HEIGHT,
        maxHeight: autoHeightEnabledRef.current ? AUTO_HEIGHT_MAX_WINDOW_HEIGHT : undefined,
      });
    } catch (error) {
      console.error("set subtitle window constraints failed:", error);
    }
  });

  const persistWindowState = useEffectEvent(async (visible: boolean) => {
    try {
      const isVisible = await appWindow.isVisible();

      if (!isVisible) {
        return;
      }

      const scaleFactor = await appWindow.scaleFactor();
      const [size, position] = await Promise.all([
        appWindow.innerSize(),
        appWindow.outerPosition(),
      ]);
      const logicalSize = size.toLogical(scaleFactor);

      await invoke<SubtitleExplainWindowState>(
        "save_subtitle_explain_window_state_command",
        {
          nextState: {
            visible,
            width: logicalSize.width,
            height: logicalSize.height,
            x: position.x / scaleFactor,
            y: position.y / scaleFactor,
          },
        },
      );
    } catch (error) {
      console.error("save subtitle explain window state failed:", error);
    }
  });

  const scheduleWindowStatePersist = useEffectEvent(
    (visible: boolean, delay = 160) => {
      if (windowStatePersistTimerRef.current) {
        window.clearTimeout(windowStatePersistTimerRef.current);
        windowStatePersistTimerRef.current = 0;
      }

      windowStatePersistTimerRef.current = window.setTimeout(() => {
        windowStatePersistTimerRef.current = 0;
        void persistWindowState(visible);
      }, delay);
    },
  );

  useEffect(() => {
    const cleanupScope = createAsyncCleanupScope();

    cleanupScope.register(
      listen<SubtitleStatus>(EVENT_SUBTITLE_STATUS_CHANGED, ({ payload }) => {
        startTransition(() => {
          setSubtitleStatus(payload);
        });
      }),
    );

    cleanupScope.register(
      listen<OcrRegionState>(EVENT_OCR_REGION_STATE_CHANGED, ({ payload }) => {
        startTransition(() => {
          setOcrRegionState(payload);
        });
      }),
    );

    cleanupScope.register(
      listen<SubtitleResult | null>(EVENT_SUBTITLE_RESULT_CHANGED, ({ payload }) => {
        startTransition(() => {
          setResult(payload);
        });
      }),
    );

    cleanupScope.register(
      appWindow.onResized(() => {
        void appWindow.isVisible().then((visible) => {
          if (visible) {
            scheduleWindowStatePersist(true);
          }
        });
      }),
    );

    cleanupScope.register(
      appWindow.onMoved(() => {
        void appWindow.isVisible().then((visible) => {
          if (visible) {
            scheduleWindowStatePersist(true);
          }
        });
      }),
    );

    void syncWindowSizeConstraints();

    void Promise.all([
      invoke<SubtitleStatus>("load_subtitle_status"),
      invoke<SubtitleResult | null>("load_latest_subtitle_result"),
      invoke<SubtitleExplainMode>("load_subtitle_explain_mode"),
      invoke<SubtitleExplainSettings>("load_subtitle_explain_settings"),
      invoke<OcrRegionState>("load_ocr_region_state"),
    ])
      .then(
        ([
          initialStatus,
          initialResult,
          initialExplainMode,
          initialSettings,
          initialOcrRegionState,
        ]) => {
          setSubtitleStatus(initialStatus);
          setResult(initialResult);
          setExplainMode(initialExplainMode);
          setSubtitleSettings(initialSettings);
          setOcrRegionState(initialOcrRegionState);
        },
      )
      .catch((error) => {
        setSubtitleStatus({
          analyzing: false,
          stage: "error",
          message: "字幕助手初始化失败。",
          error: String(error),
        });
      });

    return () => {
      if (windowStatePersistTimerRef.current) {
        window.clearTimeout(windowStatePersistTimerRef.current);
        windowStatePersistTimerRef.current = 0;
      }

      cancelWindowSizeSync();
      cleanupScope.dispose();
    };
  }, [cancelWindowSizeSync, scheduleWindowStatePersist, syncWindowSizeConstraints]);

  useEffect(() => {
    const windowContent = windowContentRef.current;

    if (!windowContent) {
      return undefined;
    }

    resizeObserverRef.current?.disconnect();

    const resizeObserver = new ResizeObserver(() => {
      scheduleWindowSizeSync(RESIZE_SETTLE_MS);
    });

    resizeObserver.observe(windowContent);
    resizeObserverRef.current = resizeObserver;

    return () => {
      resizeObserver.disconnect();
      resizeObserverRef.current = null;
    };
  }, [scheduleWindowSizeSync]);

  useEffect(() => {
    void syncWindowSizeConstraints();

    if (autoHeightEnabled) {
      scheduleWindowSizeSync(RESIZE_SETTLE_MS);
    } else {
      cancelWindowSizeSync();
    }
  }, [
    autoHeightEnabled,
    cancelWindowSizeSync,
    scheduleWindowSizeSync,
    syncWindowSizeConstraints,
  ]);

  useEffect(() => {
    scheduleWindowSizeSync(RESIZE_SETTLE_MS);
  }, [collapsed, fontScale, explainMode, result, status, scheduleWindowSizeSync]);

  useEffect(() => {
    const handlePointerDown = (event: globalThis.PointerEvent) => {
      if (
        event.target instanceof Node &&
        settingsMenuRef.current?.contains(event.target)
      ) {
        return;
      }

      setSettingsOpen(false);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSettingsOpen(false);
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  async function handleDragStart(event: PointerEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    await appWindow.startDragging();
  }

  async function handleResizePointerDown(
    event: PointerEvent<HTMLButtonElement>,
    direction: SubtitleExplainResizeDirection,
  ) {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    await appWindow.startResizeDragging(direction);
  }

  async function analyzeCurrentSubtitle() {
    try {
      setManualAnalyzePending(true);
      await waitForNextPaint();
      await invoke("analyze_current_subtitle");
    } catch (error) {
      setSubtitleStatus({
        analyzing: false,
        stage: "error",
        message: "解析失败。",
        error: String(error),
      });
    } finally {
      setManualAnalyzePending(false);
    }
  }

  async function toggleRealtimeParsing() {
    if (realtimeTogglePending) {
      return;
    }

    const previousSettings = subtitleSettings;
    const previousExplainMode = explainMode;

    try {
      setRealtimeTogglePending(true);

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

      setSubtitleSettings(savedSettings);
      setExplainMode(savedSettings.explainMode);
    } catch (error) {
      setSubtitleSettings(previousSettings);
      setExplainMode(previousExplainMode);
      setSubtitleStatus({
        analyzing: false,
        stage: "error",
        message: "切换实时解析失败。",
        error: String(error),
      });
    } finally {
      setRealtimeTogglePending(false);
    }
  }

  async function hideWindow() {
    try {
      await invoke("hide_subtitle_explain");
    } catch (error) {
      setSubtitleStatus({
        analyzing: false,
        stage: "error",
        message: "隐藏失败。",
        error: String(error),
      });
    }
  }

  async function startOcrRegionDrawing() {
    try {
      await invoke("start_ocr_region_drawing");
    } catch (error) {
      setSubtitleStatus({
        analyzing: false,
        stage: "error",
        message: "进入 OCR 绘制失败。",
        error: String(error),
      });
    }
  }

  async function applySetting(setting: string, value: string) {
    if (setting === "explainMode" && (value === "phrase" || value === "word")) {
      const previousMode = explainMode;

      setExplainMode(value);
      setSubtitleSettings((currentSettings) =>
        currentSettings
          ? {
              ...currentSettings,
              explainMode: value,
            }
          : currentSettings,
      );

      try {
        const savedExplainMode = await invoke<SubtitleExplainMode>(
          "save_subtitle_explain_mode",
          {
            explainMode: value,
          },
        );

        setExplainMode(savedExplainMode);
        setSubtitleSettings((currentSettings) =>
          currentSettings
            ? {
                ...currentSettings,
                explainMode: savedExplainMode,
              }
            : currentSettings,
        );
      } catch (error) {
        setExplainMode(previousMode);
        setSubtitleSettings((currentSettings) =>
          currentSettings
            ? {
                ...currentSettings,
                explainMode: previousMode,
              }
            : currentSettings,
        );
        setSubtitleStatus({
          analyzing: false,
          stage: "error",
          message: "切换解释模式失败。",
          error: String(error),
        });
      }

      return;
    }

    if (
      setting === "styleMode" &&
      (value === "default" || value === "contrast" || value === "contrastLight")
    ) {
      setStyleMode(value);
      persistSetting(STYLE_MODE_STORAGE_KEY, value);
      return;
    }

    if (setting === "contrastOutline" && (value === "enabled" || value === "disabled")) {
      const enabled = value === "enabled";

      setContrastOutlineEnabled(enabled);
      persistSetting(CONTRAST_OUTLINE_STORAGE_KEY, String(enabled));
      return;
    }

    if (setting === "autoHeight" && (value === "enabled" || value === "disabled")) {
      const enabled = value === "enabled";

      setAutoHeightEnabled(enabled);
      persistSetting(AUTO_HEIGHT_STORAGE_KEY, String(enabled));
      return;
    }

    if (setting === "ocrBorderMode" && (value === "editOnly" || value === "alwaysVisible")) {
      const previousOcrRegionState = ocrRegionState;

      try {
        const latestOcrRegionState = await invoke<OcrRegionState>("load_ocr_region_state");
        const nextOcrRegionState = await invoke<OcrRegionState>("apply_ocr_region_state", {
          nextState: {
            ...latestOcrRegionState,
            showBorderWhenIdle: value === "alwaysVisible",
          },
        });

        setOcrRegionState(nextOcrRegionState);
      } catch (error) {
        setOcrRegionState(previousOcrRegionState);
        setSubtitleStatus({
          analyzing: false,
          stage: "error",
          message: "切换 OCR 边框显示失败。",
          error: String(error),
        });
      }
    }
  }

  const analyzing = manualAnalyzePending || Boolean(status?.analyzing);
  const realtimeEnabled = Boolean(subtitleSettings?.realtimeEnabled);
  const windowStyle = {
    "--content-font-scale": fontScale,
  } as CSSProperties;
  const phrases = result?.phrases ?? [];
  const sourceText = result?.sourceText || "等待 OCR 结果...";
  const translationText = result?.translation || "等待 LLM 输出...";
  const hasSourceText = Boolean(result?.sourceText);
  const hasTranslation = Boolean(result?.translation);
  const hasPhrases = phrases.length > 0;
  const fontScaleSliderValue = fontScaleToSliderValue(fontScale);

  return (
    <main
      ref={windowContentRef}
      className="subtitle-window"
      data-style-mode={styleMode}
      data-contrast-outline={contrastOutlineEnabled ? "true" : "false"}
      data-hovered={styleMode !== "default" && windowHovered ? "true" : "false"}
      data-collapsed={collapsed ? "true" : "false"}
      style={windowStyle}
      onPointerEnter={() => {
        setWindowHovered(true);
      }}
      onPointerLeave={() => {
        setWindowHovered(false);
      }}
    >
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

      <div ref={statusRowRef} className="status-row">
        <div className="status-meta" onPointerDown={handleDragStart}>
          <div className="drag-meta">
            <span className="drag-pill"></span>
            <span className="status-label">字幕助手</span>
          </div>
          <span id="statusText" className="status-text" data-state={statusVisualState(status)}>
            {statusMessage(status)}
          </span>
        </div>
        <div className="action-row">
          <button
            id="drawOcrButton"
            className="action-button"
            type="button"
            onClick={() => {
              void startOcrRegionDrawing();
            }}
          >
            绘制 OCR 区域
          </button>
          <button
            id="editOcrButton"
            className="action-button"
            type="button"
            onClick={() => {
              void invoke("show_ocr_region");
            }}
          >
            编辑 OCR 区域
          </button>
          <button
            id="realtimeToggleButton"
            className="action-button action-button-toggle-live"
            type="button"
            data-enabled={realtimeEnabled ? "true" : "false"}
            aria-label="切换实时解析"
            aria-pressed={realtimeEnabled}
            disabled={realtimeTogglePending}
            title={realtimeEnabled ? "点击关闭实时解析" : "点击开启实时解析"}
            onClick={() => {
              void toggleRealtimeParsing();
            }}
          >
            {realtimeTogglePending ? "切换中..." : realtimeEnabled ? "实时解析开" : "实时解析关"}
          </button>
          <button
            id="analyzeButton"
            className="action-button action-button-primary"
            type="button"
            disabled={analyzing}
            onClick={() => {
              void analyzeCurrentSubtitle();
            }}
          >
            {analyzing ? "解析中..." : "解析当前字幕"}
          </button>
          <div ref={settingsMenuRef} className="settings-menu">
            <button
              id="settingsMenuButton"
              className="action-button action-button-menu"
              type="button"
              data-open={settingsOpen ? "true" : "false"}
              aria-haspopup="dialog"
              aria-expanded={settingsOpen}
              onClick={(event) => {
                event.stopPropagation();
                setSettingsOpen((open) => !open);
              }}
            >
              设置
              <span className="action-caret" aria-hidden="true">
                ▼
              </span>
            </button>

            <div
              id="settingsPopover"
              className="settings-popover"
              role="dialog"
              aria-label="设置"
              data-open={settingsOpen ? "true" : "false"}
              onPointerDown={(event) => {
                event.stopPropagation();
              }}
            >
              <section className="settings-group">
                <p className="settings-group-title">解释</p>
                <div className="settings-options">
                  {EXPLAIN_MODE_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      className="settings-option"
                      type="button"
                      data-setting="explainMode"
                      data-value={option.value}
                      data-active={option.value === explainMode ? "true" : "false"}
                      onClick={() => {
                        void applySetting("explainMode", option.value);
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </section>

              <section className="settings-group">
                <p className="settings-group-title">样式</p>
                <div className="settings-options">
                  {STYLE_MODE_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      className="settings-option"
                      type="button"
                      data-setting="styleMode"
                      data-value={option.value}
                      data-active={option.value === styleMode ? "true" : "false"}
                      onClick={() => {
                        void applySetting("styleMode", option.value);
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </section>

              <section className="settings-group">
                <p className="settings-group-title">字体描边</p>
                <div className="settings-options">
                  {BINARY_OPTIONS.contrastOutline.map((option) => (
                    <button
                      key={option.value}
                      className="settings-option"
                      type="button"
                      data-setting="contrastOutline"
                      data-value={option.value}
                      data-active={
                        option.value === (contrastOutlineEnabled ? "enabled" : "disabled")
                          ? "true"
                          : "false"
                      }
                      onClick={() => {
                        void applySetting("contrastOutline", option.value);
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </section>

              <section className="settings-group">
                <p className="settings-group-title">窗口高度</p>
                <div className="settings-options">
                  {BINARY_OPTIONS.autoHeight.map((option) => (
                    <button
                      key={option.value}
                      className="settings-option"
                      type="button"
                      data-setting="autoHeight"
                      data-value={option.value}
                      data-active={
                        option.value === (autoHeightEnabled ? "enabled" : "disabled")
                          ? "true"
                          : "false"
                      }
                      onClick={() => {
                        void applySetting("autoHeight", option.value);
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </section>

              <section className="settings-group">
                <div className="settings-slider-header">
                  <p className="settings-group-title">字号</p>
                  <output className="settings-slider-value" htmlFor="fontScaleSlider">
                    {fontScaleLabel(fontScale)}
                  </output>
                </div>
                <div className="settings-slider">
                  <input
                    id="fontScaleSlider"
                    className="settings-slider-input"
                    type="range"
                    min="0"
                    max={String(FONT_SCALE_VALUES.length - 1)}
                    step="1"
                    value={fontScaleSliderValue}
                    aria-label="调节内容字号"
                    aria-valuetext={fontScaleLabel(fontScale)}
                    onChange={(event) => {
                      const nextFontScale = fontScaleFromSliderValue(event.target.value);

                      setFontScale(nextFontScale);
                      persistSetting(FONT_SCALE_STORAGE_KEY, nextFontScale);
                    }}
                  />
                  <div className="settings-slider-labels" aria-hidden="true">
                    {FONT_SCALE_VALUES.map((value) => (
                      <span key={value}>{fontScaleLabel(value)}</span>
                    ))}
                  </div>
                </div>
              </section>

              <section className="settings-group">
                <p className="settings-group-title">OCR 边框</p>
                <div className="settings-options">
                  {BINARY_OPTIONS.ocrBorderMode.map((option) => (
                    <button
                      key={option.value}
                      className="settings-option"
                      type="button"
                      data-setting="ocrBorderMode"
                      data-value={option.value}
                      data-active={
                        option.value ===
                        (ocrRegionState?.showBorderWhenIdle ? "alwaysVisible" : "editOnly")
                          ? "true"
                          : "false"
                      }
                      onClick={() => {
                        void applySetting("ocrBorderMode", option.value);
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </section>
            </div>
          </div>
          <button
            id="collapseButton"
            className="action-button action-button-icon action-button-toggle"
            type="button"
            aria-label={collapsed ? "展开内容" : "收起内容"}
            title={collapsed ? "展开内容" : "收起内容"}
            onClick={() => {
              setCollapsed((currentCollapsed) => {
                const nextCollapsed = !currentCollapsed;

                if (nextCollapsed) {
                  setSettingsOpen(false);
                }

                return nextCollapsed;
              });
            }}
          >
            {collapsed ? "▼" : "▲"}
          </button>
          <button
            id="hideButton"
            className="action-button action-button-icon"
            type="button"
            aria-label="隐藏窗口"
            title="隐藏窗口"
            onClick={() => {
              void hideWindow();
            }}
          >
            ×
          </button>
          <div
            id="collapsedDragRegion"
            className="collapsed-drag-region"
            aria-hidden="true"
            onPointerDown={handleDragStart}
          ></div>
        </div>
      </div>

      <div ref={contentStackRef} className="content-stack">
        <section className="block block-summary">
          <div className="summary-row">
            <span className="summary-label">原文</span>
            <p
              id="sourceText"
              className={`summary-text summary-text-source ${hasSourceText ? "" : "empty"}`.trim()}
            >
              {sourceText}
            </p>
          </div>
          <div className="summary-divider" aria-hidden="true"></div>
          <div className="summary-row">
            <span className="summary-label">翻译</span>
            <p id="translationText" className={`summary-text ${hasTranslation ? "" : "empty"}`.trim()}>
              {translationText}
            </p>
          </div>
        </section>

        <section className="block block-phrases">
          <div
            id="phrasesTitle"
            className="block-title"
            onPointerDown={collapsed ? handleDragStart : undefined}
          >
            {phrasesSectionTitle(explainMode)}
          </div>
          <div id="phrasesList" className={`phrase-list ${hasPhrases ? "" : "empty"}`.trim()}>
            {hasPhrases
              ? phrases.map((phrase: SubtitlePhrase) => (
                  <article key={`${phrase.source}-${phrase.explanation}`} className="phrase-item">
                    <strong>{phrase.source}</strong>
                    <span>{phrase.explanation}</span>
                  </article>
                ))
              : emptyPhrasesText(explainMode)}
          </div>
        </section>
      </div>
    </main>
  );
}

mountWindowApp(<SubtitleExplainWindow />);
