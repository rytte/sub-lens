import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";

import { mountWindowApp } from "./main";
import type {
  ProviderConnectionSettings,
  StatusTone,
  SubtitleExplainMode,
  SubtitleExplainSettings,
  SubtitleProvider,
  SubtitleProviderProfiles,
} from "./shared/types";

const appWindow = getCurrentWindow();

type InlineStatus = {
  message: string;
  tone: StatusTone;
};

type SettingsFormState = {
  realtimeEnabled: boolean;
  provider: SubtitleProvider;
  explainMode: SubtitleExplainMode;
  providerProfiles: SubtitleProviderProfiles;
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
    providerProfiles: cloneProfiles(settings.providerProfiles),
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
    baseUrl: activeProfile.baseUrl,
    apiKey: activeProfile.apiKey,
    model: activeProfile.model,
    providerProfiles,
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
    return "推荐填写 OpenAI 兼容地址，例如 https://api.openai.com/v1，API Key 一般必填。";
  }

  return "本地 Ollama 常用地址是 http://127.0.0.1:11434，直连场景通常可以留空 API Key。";
}

function defaultModelHint(provider: SubtitleProvider) {
  if (provider === "openAiResponses") {
    return "可手动填写，也可以用当前 Base URL 和 API Key 拉取模型列表。";
  }

  return "可手动填写，也可以从当前 Ollama 服务拉取已安装模型。";
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

export function SettingsWindow() {
  const [form, setForm] = useState<SettingsFormState | null>(null);
  const [status, setStatus] = useState<InlineStatus>({
    message: "正在加载设置...",
    tone: "idle",
  });
  const [modelHint, setModelHint] = useState<InlineStatus>({
    message: "正在加载模型设置...",
    tone: "idle",
  });
  const [modelSuggestions, setModelSuggestions] = useState<string[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const loadedSettingsRef = useRef<SubtitleExplainSettings | null>(null);

  const activeProfile = form?.providerProfiles[form.provider] ?? null;
  const placeholders = providerPlaceholders(form?.provider ?? "openAiResponses");

  useEffect(() => {
    void invoke<SubtitleExplainSettings>("load_subtitle_explain_settings")
      .then((settings) => {
        loadedSettingsRef.current = settings;
        setForm(toFormState(settings));
        setStatus({
          message: "已加载设置。",
          tone: "idle",
        });
        setModelHint({
          message: defaultModelHint(settings.provider),
          tone: "idle",
        });
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
        {
          nextSettings,
        },
      );

      loadedSettingsRef.current = savedSettings;
      setForm(toFormState(savedSettings));
      setModelSuggestions([]);
      setModelHint({
        message: defaultModelHint(savedSettings.provider),
        tone: "idle",
      });
      setStatus({
        message: "设置已保存。",
        tone: "success",
      });
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
    if (!window.confirm("确定恢复字幕解析相关默认配置吗？")) {
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

      loadedSettingsRef.current = settings;
      setForm(toFormState(settings));
      setModelSuggestions([]);
      setModelHint({
        message: defaultModelHint(settings.provider),
        tone: "idle",
      });
      setStatus({
        message: "已恢复默认配置。",
        tone: "success",
      });
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
        message: `拉取失败：${String(error)}`,
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
          <p className="eyebrow">字幕解析与解释</p>
          <h1 className="hero-title">更多设置</h1>
          <p className="hero-subtitle">
            这里集中管理新增配置项，包括 Provider、Base URL、模型、实时参数等。
          </p>
        </div>
        <div className="hero-tags" aria-label="方案范围">
          <span className="tag">PP-OCRv5</span>
          <span className="tag">OpenAI Responses</span>
          <span className="tag">Ollama</span>
        </div>
      </header>

      <section className="settings-card section-grid">
        <div className="section-head section-head-provider">
          <div>
            <h2>Provider</h2>
          </div>
        </div>
        <label className="field">
          <span className="field-label">服务类型</span>
          <select
            className="text-input select-input"
            value={form?.provider ?? "openAiResponses"}
            onChange={(event) => {
              const nextProvider = event.target.value as SubtitleProvider;

              setForm((currentForm) => {
                if (!currentForm) {
                  return currentForm;
                }

                return {
                  ...currentForm,
                  provider: nextProvider,
                };
              });
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
                      baseUrl,
                    },
                  ),
                };
              });
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
                      apiKey,
                    },
                  ),
                };
              });
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
                        model,
                      },
                    ),
                  };
                });
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
                          model,
                        },
                      ),
                    };
                  });
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
                currentForm
                  ? {
                      ...currentForm,
                      ocrIntervalMs,
                    }
                  : currentForm,
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
                currentForm
                  ? {
                      ...currentForm,
                      stableFrames,
                    }
                  : currentForm,
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
                currentForm
                  ? {
                      ...currentForm,
                      similarityThreshold,
                    }
                  : currentForm,
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
                currentForm
                  ? {
                      ...currentForm,
                      blankClearMs,
                    }
                  : currentForm,
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
