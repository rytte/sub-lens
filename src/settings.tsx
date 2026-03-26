import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";

import { mountWindowApp } from "./main";
import type {
  OcrEngine,
  OcrEngineAvailability,
  OcrLanguage,
  OcrSettings,
  ProviderConnectionSettings,
  StatusTone,
  SubtitleExplainMode,
  SubtitleExplainSettings,
  SubtitleProvider,
  SubtitleProviderProfiles,
  TrayClickAction,
} from "./shared/types";

const appWindow = getCurrentWindow();

const TRAY_CLICK_ACTION_OPTIONS: Array<{
  value: TrayClickAction;
  label: string;
  description: string;
}> = [
  {
    value: "overlay",
    label: "字幕遮挡窗口",
    description: "单击托盘图标时切换字幕遮挡窗口的显示/隐藏。",
  },
  {
    value: "subtitleExplain",
    label: "字幕助手窗口",
    description: "单击托盘图标时切换字幕助手窗口的显示/隐藏。",
  },
];

const OCR_ENGINE_OPTIONS: Array<{
  value: OcrEngine;
  label: string;
  description: string;
}> = [
  {
    value: "windowsOcr",
    label: "Windows OCR",
    description: "直接调用系统 OCR，适合 Windows 10/11 的离线识别。",
  },
  {
    value: "tesseract",
    label: "Tesseract.js",
    description: "使用内置 WebAssembly OCR，不依赖本机安装 tesseract.exe。",
  },
  {
    value: "paddleOcr",
    label: "PaddleOCR",
    description: "沿用现有 Python sidecar 中的 PaddleOCR 能力。",
  },
];

const OCR_LANGUAGE_OPTIONS: Array<{
  value: OcrLanguage;
  label: string;
}> = [
  { value: "auto", label: "自动" },
  { value: "zhCn", label: "简体中文" },
  { value: "zhTw", label: "繁体中文" },
  { value: "en", label: "英语" },
  { value: "yue", label: "粤语" },
  { value: "ja", label: "日语" },
  { value: "ko", label: "韩语" },
  { value: "fr", label: "法语" },
  { value: "es", label: "西班牙语" },
  { value: "ru", label: "俄语" },
  { value: "de", label: "德语" },
  { value: "it", label: "意大利语" },
  { value: "tr", label: "土耳其语" },
  { value: "pt", label: "葡萄牙语" },
  { value: "ptBr", label: "巴西葡萄牙语" },
  { value: "vi", label: "越南语" },
  { value: "id", label: "印尼语" },
  { value: "th", label: "泰语" },
  { value: "ms", label: "马来语" },
  { value: "ar", label: "阿拉伯语" },
  { value: "hi", label: "印地语" },
  { value: "uk", label: "乌克兰语" },
  { value: "he", label: "希伯来语" },
];

type InlineStatus = {
  message: string;
  tone: StatusTone;
};

type SettingsFormState = {
  realtimeEnabled: boolean;
  provider: SubtitleProvider;
  explainMode: SubtitleExplainMode;
  trayClickAction: TrayClickAction;
  providerProfiles: SubtitleProviderProfiles;
  ocrEngine: OcrEngine;
  ocrLanguage: OcrLanguage;
  tesseractPageSegMode: string;
  ocrIntervalMs: string;
  stableFrames: string;
  similarityThreshold: string;
  blankClearMs: string;
};

function cloneProfiles(
  profiles: SubtitleProviderProfiles,
): SubtitleProviderProfiles {
  return {
    openAiResponses: { ...profiles.openAiResponses },
    ollama: { ...profiles.ollama },
  };
}

function toFormState(settings: SubtitleExplainSettings): SettingsFormState {
  return {
    realtimeEnabled: settings.realtimeEnabled,
    provider: settings.provider,
    explainMode: settings.explainMode,
    trayClickAction: settings.trayClickAction,
    providerProfiles: cloneProfiles(settings.providerProfiles),
    ocrEngine: settings.ocrSettings.engine,
    ocrLanguage: settings.ocrSettings.language,
    tesseractPageSegMode: String(settings.ocrSettings.profiles.tesseract.pageSegMode),
    ocrIntervalMs: String(settings.ocrIntervalMs),
    stableFrames: String(settings.stableFrames),
    similarityThreshold: settings.similarityThreshold.toFixed(2),
    blankClearMs: String(settings.blankClearMs),
  };
}

function readNumber(value: string, fallback: number) {
  const parsedValue = Number.parseFloat(value);
  return Number.isFinite(parsedValue) ? parsedValue : fallback;
}

function normalizeProfile(
  profile: ProviderConnectionSettings,
): ProviderConnectionSettings {
  return {
    baseUrl: profile.baseUrl.trim(),
    apiKey: profile.apiKey,
    model: profile.model.trim(),
  };
}

function buildOcrSettings(
  form: SettingsFormState,
  fallback: OcrSettings,
): OcrSettings {
  return {
    engine: form.ocrEngine,
    language: form.ocrLanguage,
    profiles: {
      tesseract: {
        pageSegMode: Math.max(
          0,
          Math.min(
            13,
            Math.round(
              readNumber(
                form.tesseractPageSegMode,
                fallback.profiles.tesseract.pageSegMode,
              ),
            ),
          ),
        ),
      },
    },
  };
}

function buildSettings(
  form: SettingsFormState,
  fallback: SubtitleExplainSettings,
): SubtitleExplainSettings {
  const providerProfiles = {
    openAiResponses: normalizeProfile(form.providerProfiles.openAiResponses),
    ollama: normalizeProfile(form.providerProfiles.ollama),
  };
  const activeProfile = providerProfiles[form.provider];

  return {
    realtimeEnabled: form.realtimeEnabled,
    provider: form.provider,
    explainMode: form.explainMode,
    trayClickAction: form.trayClickAction,
    baseUrl: activeProfile.baseUrl,
    apiKey: activeProfile.apiKey,
    model: activeProfile.model,
    providerProfiles,
    ocrSettings: buildOcrSettings(form, fallback.ocrSettings),
    ocrIntervalMs: readNumber(form.ocrIntervalMs, fallback.ocrIntervalMs),
    stableFrames: readNumber(form.stableFrames, fallback.stableFrames),
    similarityThreshold: readNumber(
      form.similarityThreshold,
      fallback.similarityThreshold,
    ),
    blankClearMs: readNumber(form.blankClearMs, fallback.blankClearMs),
  };
}

function providerHint(provider: SubtitleProvider) {
  if (provider === "openAiResponses") {
    return "推荐填写 OpenAI 兼容地址，例如 https://api.openai.com/v1，并提供对应 API Key。";
  }

  return "本地 Ollama 常见地址是 http://127.0.0.1:11434，通常可以不填 API Key。";
}

function trayClickActionHint(action: TrayClickAction) {
  return (
    TRAY_CLICK_ACTION_OPTIONS.find((option) => option.value === action)?.description ??
    ""
  );
}

function defaultModelHint(provider: SubtitleProvider) {
  if (provider === "openAiResponses") {
    return "可以手动填写模型，也可以根据当前 Base URL 和 API Key 拉取模型列表。";
  }

  return "可以手动填写模型，也可以从当前 Ollama 服务拉取已安装模型。";
}

function providerPlaceholders(provider: SubtitleProvider) {
  if (provider === "openAiResponses") {
    return {
      baseUrl: "https://api.openai.com/v1",
      apiKey: "sk-...",
      model: "gpt-4.1-mini",
    };
  }

  return {
    baseUrl: "http://127.0.0.1:11434",
    apiKey: "通常可留空",
    model: "qwen2.5:7b",
  };
}

function updateProviderProfile(
  profiles: SubtitleProviderProfiles,
  provider: SubtitleProvider,
  patch: Partial<ProviderConnectionSettings>,
): SubtitleProviderProfiles {
  return {
    ...profiles,
    [provider]: {
      ...profiles[provider],
      ...patch,
    },
  } as SubtitleProviderProfiles;
}

function defaultOcrHint(engine: OcrEngine) {
  switch (engine) {
    case "windowsOcr":
      return "Windows OCR 依赖系统语言包；如果识别不到目标语言，请先在系统里安装对应 OCR 语言支持。";
    case "tesseract":
      return "Tesseract.js 不需要本地安装可执行文件。首次使用会下载 WASM 与语言数据，自动模式默认会尝试英文、简中、日文、韩文。";
    case "paddleOcr":
      return "PaddleOCR 继续通过 Python sidecar 运行，适合保留现有识别链路。";
  }
}

function formatAvailabilityMessage(
  availability: OcrEngineAvailability | undefined,
  fallbackMessage: string,
): InlineStatus {
  if (!availability) {
    return {
      message: fallbackMessage,
      tone: "idle",
    };
  }

  if (availability.available) {
    return {
      message: availability.reason
        ? `${availability.label} 可用：${availability.reason}`
        : `${availability.label} 可用。`,
      tone: "success",
    };
  }

  return {
    message: availability.reason
      ? `${availability.label} 当前不可用：${availability.reason}`
      : `${availability.label} 当前不可用。`,
    tone: "error",
  };
}

function engineStatusLabel(item: OcrEngineAvailability) {
  return `${item.label} · ${item.available ? "可用" : "未就绪"}`;
}

export function SettingsWindow() {
  const [form, setForm] = useState<SettingsFormState | null>(null);
  const [status, setStatus] = useState<InlineStatus>({
    message: "正在加载设置...",
    tone: "idle",
  });
  const [modelHint, setModelHint] = useState<InlineStatus>({
    message: "正在加载模型配置...",
    tone: "idle",
  });
  const [ocrHint, setOcrHint] = useState<InlineStatus>({
    message: "正在加载 OCR 配置...",
    tone: "idle",
  });
  const [modelSuggestions, setModelSuggestions] = useState<string[]>([]);
  const [ocrAvailability, setOcrAvailability] = useState<OcrEngineAvailability[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [isInspectingOcr, setIsInspectingOcr] = useState(false);
  const loadedSettingsRef = useRef<SubtitleExplainSettings | null>(null);

  const activeProfile = form?.providerProfiles[form.provider] ?? null;
  const placeholders = providerPlaceholders(form?.provider ?? "openAiResponses");

  async function inspectOcrAvailability(
    nextForm?: SettingsFormState,
    nextSettings?: SubtitleExplainSettings,
  ) {
    const effectiveForm = nextForm ?? form;
    const effectiveSettings = nextSettings ?? loadedSettingsRef.current;

    if (!effectiveForm || !effectiveSettings) {
      return;
    }

    try {
      setIsInspectingOcr(true);
      setOcrHint({
        message: "正在检测 OCR 引擎...",
        tone: "idle",
      });

      const availability = await invoke<OcrEngineAvailability[]>("inspect_ocr_engines", {
        ocrSettings: buildOcrSettings(effectiveForm, effectiveSettings.ocrSettings),
      });

      setOcrAvailability(availability);
      const current = availability.find((item) => item.engine === effectiveForm.ocrEngine);
      setOcrHint(formatAvailabilityMessage(current, defaultOcrHint(effectiveForm.ocrEngine)));
    } catch (error) {
      setOcrAvailability([]);
      setOcrHint({
        message: `检测 OCR 引擎失败：${String(error)}`,
        tone: "error",
      });
    } finally {
      setIsInspectingOcr(false);
    }
  }

  useEffect(() => {
    void invoke<SubtitleExplainSettings>("load_subtitle_explain_settings")
      .then((settings) => {
        const nextForm = toFormState(settings);

        loadedSettingsRef.current = settings;
        setForm(nextForm);
        setStatus({
          message: "已加载设置。",
          tone: "idle",
        });
        setModelHint({
          message: defaultModelHint(settings.provider),
          tone: "idle",
        });
        setOcrHint({
          message: defaultOcrHint(nextForm.ocrEngine),
          tone: "idle",
        });
        void inspectOcrAvailability(nextForm, settings);
      })
      .catch((error) => {
        setStatus({
          message: `初始化失败：${String(error)}`,
          tone: "error",
        });
      });
  }, []);

  async function dismissWindow() {
    try {
      await appWindow.hide();
    } catch (error) {
      setStatus({
        message: `关闭失败：${String(error)}`,
        tone: "error",
      });
    }
  }

  async function saveSettings() {
    if (isSaving || !form || !loadedSettingsRef.current) {
      return;
    }

    try {
      setIsSaving(true);
      setStatus({
        message: "正在保存设置...",
        tone: "idle",
      });

      const nextSettings = buildSettings(form, loadedSettingsRef.current);
      const savedSettings = await invoke<SubtitleExplainSettings>(
        "save_subtitle_explain_settings",
        { nextSettings },
      );
      const nextForm = toFormState(savedSettings);

      loadedSettingsRef.current = savedSettings;
      setForm(nextForm);
      setModelSuggestions([]);
      setModelHint({
        message: defaultModelHint(savedSettings.provider),
        tone: "idle",
      });
      setStatus({
        message: "设置已保存。",
        tone: "success",
      });
      void inspectOcrAvailability(nextForm, savedSettings);
    } catch (error) {
      setStatus({
        message: `保存失败：${String(error)}`,
        tone: "error",
      });
    } finally {
      setIsSaving(false);
    }
  }

  async function resetSettings() {
    if (!window.confirm("确定恢复字幕解析相关的默认配置吗？")) {
      return;
    }

    try {
      setStatus({
        message: "正在恢复默认配置...",
        tone: "idle",
      });

      const settings = await invoke<SubtitleExplainSettings>(
        "reset_subtitle_explain_settings",
      );
      const nextForm = toFormState(settings);

      loadedSettingsRef.current = settings;
      setForm(nextForm);
      setModelSuggestions([]);
      setModelHint({
        message: defaultModelHint(settings.provider),
        tone: "idle",
      });
      setStatus({
        message: "已恢复默认配置。",
        tone: "success",
      });
      void inspectOcrAvailability(nextForm, settings);
    } catch (error) {
      setStatus({
        message: `恢复失败：${String(error)}`,
        tone: "error",
      });
    }
  }

  async function fetchAvailableModels() {
    if (isFetchingModels || !form || !activeProfile) {
      return;
    }

    try {
      setIsFetchingModels(true);
      setStatus({
        message: "正在拉取可用模型...",
        tone: "idle",
      });
      setModelHint({
        message: "正在请求模型列表...",
        tone: "idle",
      });

      const models = await invoke<string[]>("fetch_available_subtitle_models", {
        provider: form.provider,
        baseUrl: activeProfile.baseUrl.trim(),
        apiKey: activeProfile.apiKey,
      });

      setModelSuggestions(models);

      if (!activeProfile.model.trim() && models.length > 0) {
        setForm((currentForm) => {
          if (!currentForm) {
            return currentForm;
          }

          return {
            ...currentForm,
            providerProfiles: updateProviderProfile(
              currentForm.providerProfiles,
              currentForm.provider,
              {
                model: models[0],
              },
            ),
          };
        });
      }

      setModelHint({
        message: `已拉取 ${models.length} 个模型，可继续筛选或直接输入。`,
        tone: "success",
      });
      setStatus({
        message: `已拉取 ${models.length} 个可用模型。`,
        tone: "success",
      });
    } catch (error) {
      setModelSuggestions([]);
      setModelHint({
        message: `拉取模型失败：${String(error)}`,
        tone: "error",
      });
      setStatus({
        message: `拉取模型失败：${String(error)}`,
        tone: "error",
      });
    } finally {
      setIsFetchingModels(false);
    }
  }

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveSettings();
      }

      if (event.key === "Escape") {
        event.preventDefault();
        void dismissWindow();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  });

  return (
    <main className="settings-shell">
      <header className="hero-card">
        <div>
          <p className="eyebrow">字幕解析与 OCR</p>
          <h1 className="hero-title">更多设置</h1>
          <p className="hero-subtitle">
            在这里统一管理 OCR 引擎、字幕解释 Provider 和实时识别参数。
          </p>
        </div>
        <div className="hero-tags" aria-label="能力范围">
          <span className="tag">Windows OCR</span>
          <span className="tag">Tesseract.js</span>
          <span className="tag">PaddleOCR</span>
          <span className="tag">OpenAI Responses</span>
          <span className="tag">Ollama</span>
        </div>
      </header>

      <section className="settings-card section-grid two-columns">
        <div className="section-head full-span">
          <div>
            <h2>OCR 引擎</h2>
          </div>
        </div>
        <label className="field">
          <span className="field-label">OCR 引擎</span>
          <select
            className="text-input select-input"
            value={form?.ocrEngine ?? "windowsOcr"}
            onChange={(event) => {
              const nextEngine = event.target.value as OcrEngine;

              setForm((currentForm) =>
                currentForm
                  ? {
                      ...currentForm,
                      ocrEngine: nextEngine,
                    }
                  : currentForm,
              );
              setOcrHint({
                message: defaultOcrHint(nextEngine),
                tone: "idle",
              });
            }}
          >
            {OCR_ENGINE_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="field-label">识别语言</span>
          <select
            className="text-input select-input"
            value={form?.ocrLanguage ?? "auto"}
            onChange={(event) => {
              const nextLanguage = event.target.value as OcrLanguage;

              setForm((currentForm) =>
                currentForm
                  ? {
                      ...currentForm,
                      ocrLanguage: nextLanguage,
                    }
                  : currentForm,
              );
            }}
          >
            {OCR_LANGUAGE_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <div className="field">
          <span className="field-label">引擎说明</span>
          <span className="field-help" data-tone={ocrHint.tone}>
            {form
              ? OCR_ENGINE_OPTIONS.find((option) => option.value === form.ocrEngine)?.description
              : ""}
          </span>
          <span className="field-help" data-tone={ocrHint.tone}>
            {ocrHint.message}
          </span>
          <div className="input-action-row">
            <button
              className="ghost-button inline-button"
              type="button"
              disabled={isInspectingOcr || !form}
              onClick={() => {
                void inspectOcrAvailability();
              }}
            >
              {isInspectingOcr ? "检测中..." : "检测引擎"}
            </button>
          </div>
          <div className="model-list" hidden={ocrAvailability.length === 0}>
            {ocrAvailability.map((item) => (
              <span
                key={item.engine}
                className="model-chip"
                title={item.reason ?? ""}
              >
                {engineStatusLabel(item)}
              </span>
            ))}
          </div>
        </div>
        {form?.ocrEngine === "tesseract" ? (
          <label className="field">
            <span className="field-label">PSM</span>
            <input
              className="text-input"
              type="number"
              min="0"
              max="13"
              step="1"
              value={form.tesseractPageSegMode}
              onChange={(event) => {
                const tesseractPageSegMode = event.target.value;

                setForm((currentForm) =>
                  currentForm
                    ? {
                        ...currentForm,
                        tesseractPageSegMode,
                      }
                    : currentForm,
                );
              }}
            />
            <span className="field-help" data-tone="idle">
              常用值是 `6`，表示按单个统一文本块识别，适合字幕场景。
            </span>
          </label>
        ) : null}
        <p className="inline-hint full-span">
          {form ? defaultOcrHint(form.ocrEngine) : ""}
        </p>
      </section>

      <section className="settings-card section-grid">
        <div className="section-head">
          <div>
            <h2>托盘点击</h2>
          </div>
        </div>
        <label className="field">
          <span className="field-label">单击托盘图标</span>
          <select
            className="text-input select-input"
            value={form?.trayClickAction ?? "overlay"}
            onChange={(event) => {
              const trayClickAction = event.target.value as TrayClickAction;

              setForm((currentForm) =>
                currentForm
                  ? {
                      ...currentForm,
                      trayClickAction,
                    }
                  : currentForm,
              );
            }}
          >
            {TRAY_CLICK_ACTION_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
          <span className="field-help" data-tone="idle">
            {trayClickActionHint(form?.trayClickAction ?? "overlay")}
          </span>
        </label>
      </section>

      <section className="settings-card section-grid">
        <div className="section-head section-head-provider">
          <div>
            <h2>字幕解释 Provider</h2>
          </div>
        </div>
        <label className="field">
          <span className="field-label">服务类型</span>
          <select
            className="text-input select-input"
            value={form?.provider ?? "openAiResponses"}
            onChange={(event) => {
              const nextProvider = event.target.value as SubtitleProvider;

              setForm((currentForm) =>
                currentForm
                  ? {
                      ...currentForm,
                      provider: nextProvider,
                    }
                  : currentForm,
              );
              setModelSuggestions([]);
              setModelHint({
                message: defaultModelHint(nextProvider),
                tone: "idle",
              });
            }}
          >
            <option value="openAiResponses">OpenAI Responses</option>
            <option value="ollama">Ollama</option>
          </select>
        </label>
        <p className="inline-hint">{providerHint(form?.provider ?? "openAiResponses")}</p>
        <label className="field">
          <span className="field-label">Base URL</span>
          <input
            className="text-input"
            type="url"
            spellCheck="false"
            placeholder={placeholders.baseUrl}
            value={activeProfile?.baseUrl ?? ""}
            onChange={(event) => {
              const baseUrl = event.target.value;

              setForm((currentForm) =>
                currentForm
                  ? {
                      ...currentForm,
                      providerProfiles: updateProviderProfile(
                        currentForm.providerProfiles,
                        currentForm.provider,
                        { baseUrl },
                      ),
                    }
                  : currentForm,
              );
            }}
          />
        </label>
        <label className="field">
          <span className="field-label">API Key</span>
          <input
            className="text-input"
            type="password"
            spellCheck="false"
            placeholder={placeholders.apiKey}
            value={activeProfile?.apiKey ?? ""}
            onChange={(event) => {
              const apiKey = event.target.value;

              setForm((currentForm) =>
                currentForm
                  ? {
                      ...currentForm,
                      providerProfiles: updateProviderProfile(
                        currentForm.providerProfiles,
                        currentForm.provider,
                        { apiKey },
                      ),
                    }
                  : currentForm,
              );
            }}
          />
        </label>
        <label className="field">
          <span className="field-label">模型</span>
          <div className="input-action-row">
            <input
              className="text-input"
              type="text"
              spellCheck="false"
              placeholder={placeholders.model}
              value={activeProfile?.model ?? ""}
              onChange={(event) => {
                const model = event.target.value;

                setForm((currentForm) =>
                  currentForm
                    ? {
                        ...currentForm,
                        providerProfiles: updateProviderProfile(
                          currentForm.providerProfiles,
                          currentForm.provider,
                          { model },
                        ),
                      }
                    : currentForm,
                );
              }}
            />
            <button
              className="ghost-button inline-button"
              type="button"
              disabled={isFetchingModels}
              onClick={() => {
                void fetchAvailableModels();
              }}
            >
              拉取
            </button>
          </div>
          <div className="model-list" hidden={modelSuggestions.length === 0}>
            {modelSuggestions.map((model) => (
              <button
                key={model}
                className="model-chip"
                type="button"
                title={`填入模型：${model}`}
                onClick={() => {
                  setForm((currentForm) =>
                    currentForm
                      ? {
                          ...currentForm,
                          providerProfiles: updateProviderProfile(
                            currentForm.providerProfiles,
                            currentForm.provider,
                            { model },
                          ),
                        }
                      : currentForm,
                  );
                  setModelHint({
                    message: `已选中模型：${model}`,
                    tone: "success",
                  });
                  setStatus({
                    message: `已填入模型 ${model}。`,
                    tone: "success",
                  });
                }}
              >
                {model}
              </button>
            ))}
          </div>
          <span className="field-help" data-tone={modelHint.tone}>
            {modelHint.message}
          </span>
        </label>
      </section>

      <section className="settings-card section-grid two-columns">
        <div className="section-head section-head-realtime full-span">
          <div>
            <h2>实时参数</h2>
          </div>
        </div>
        <label className="field">
          <span className="field-label">OCR 间隔 (ms)</span>
          <input
            className="text-input"
            type="number"
            min="250"
            max="4000"
            step="10"
            value={form?.ocrIntervalMs ?? ""}
            onChange={(event) => {
              const ocrIntervalMs = event.target.value;
              setForm((currentForm) =>
                currentForm ? { ...currentForm, ocrIntervalMs } : currentForm,
              );
            }}
          />
        </label>
        <label className="field">
          <span className="field-label">稳定帧数</span>
          <input
            className="text-input"
            type="number"
            min="1"
            max="10"
            step="1"
            value={form?.stableFrames ?? ""}
            onChange={(event) => {
              const stableFrames = event.target.value;
              setForm((currentForm) =>
                currentForm ? { ...currentForm, stableFrames } : currentForm,
              );
            }}
          />
        </label>
        <label className="field">
          <span className="field-label">相似度阈值</span>
          <input
            className="text-input"
            type="number"
            min="0.5"
            max="1"
            step="0.01"
            value={form?.similarityThreshold ?? ""}
            onChange={(event) => {
              const similarityThreshold = event.target.value;
              setForm((currentForm) =>
                currentForm ? { ...currentForm, similarityThreshold } : currentForm,
              );
            }}
          />
        </label>
        <label className="field">
          <span className="field-label">空白清空阈值 (ms)</span>
          <input
            className="text-input"
            type="number"
            min="300"
            max="5000"
            step="10"
            value={form?.blankClearMs ?? ""}
            onChange={(event) => {
              const blankClearMs = event.target.value;
              setForm((currentForm) =>
                currentForm ? { ...currentForm, blankClearMs } : currentForm,
              );
            }}
          />
        </label>
      </section>

      <footer className="footer-bar">
        <div className="status-text" data-tone={status.tone}>
          {status.message}
        </div>
        <div className="action-row">
          <button
            className="ghost-button"
            type="button"
            onClick={() => {
              void resetSettings();
            }}
          >
            恢复默认
          </button>
          <button
            className="ghost-button"
            type="button"
            onClick={() => {
              void dismissWindow();
            }}
          >
            关闭
          </button>
          <button
            className="primary-button"
            type="button"
            disabled={isSaving}
            onClick={() => {
              void saveSettings();
            }}
          >
            保存
          </button>
        </div>
      </footer>
    </main>
  );
}

mountWindowApp(<SettingsWindow />);
