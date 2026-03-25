import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

type SubtitleProvider = "openAiResponses" | "ollama";
type SubtitleExplainMode = "phrase" | "word";

type ProviderConnectionSettings = {
  baseUrl: string;
  apiKey: string;
  model: string;
};

type SubtitleProviderProfiles = {
  openAiResponses: ProviderConnectionSettings;
  ollama: ProviderConnectionSettings;
};

type SubtitleExplainSettings = {
  realtimeEnabled: boolean;
  provider: SubtitleProvider;
  explainMode: SubtitleExplainMode;
  baseUrl: string;
  apiKey: string;
  model: string;
  providerProfiles: SubtitleProviderProfiles;
  ocrIntervalMs: number;
  stableFrames: number;
  similarityThreshold: number;
  blankClearMs: number;
};

type StatusTone = "idle" | "success" | "error";

const appWindow = getCurrentWindow();
const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

app.innerHTML = `
  <main class="settings-shell">
    <header class="hero-card">
      <div>
        <p class="eyebrow">字幕解析与解释</p>
        <h1 class="hero-title">更多设置</h1>
        <p class="hero-subtitle">这里集中管理新增配置项，包括 Provider、base_url、模型、实时参数等。</p>
      </div>
      <div class="hero-tags" aria-label="方案落地范围">
        <span class="tag">PP-OCRv5</span>
        <span class="tag">OpenAI Responses</span>
        <span class="tag">Ollama</span>
      </div>
    </header>

    <section class="settings-card section-grid">
      <div class="section-head">
        <div>
          <h2>功能开关</h2>
          <p>当前默认以手动解析为主，实时解析作为增强项预留。</p>
        </div>
      </div>
      <label class="toggle-row">
        <div>
          <span class="field-label">实时解析</span>
          <span class="field-help">二阶段增强项，配置现在可保存，能力接入后直接生效。</span>
        </div>
        <input id="realtimeInput" type="checkbox" class="toggle-input" />
      </label>
    </section>

    <section class="settings-card section-grid">
      <div class="section-head">
        <div>
          <h2>Provider</h2>
          <p>统一抽象为固定 JSON 输出，避免前端依赖具体供应商。</p>
        </div>
      </div>
      <label class="field">
        <span class="field-label">服务类型</span>
        <select id="providerInput" class="text-input select-input">
          <option value="openAiResponses">OpenAI Responses</option>
          <option value="ollama">Ollama</option>
        </select>
      </label>
      <p id="providerHint" class="inline-hint"></p>
      <label class="field">
        <span class="field-label">base_url</span>
        <input id="baseUrlInput" class="text-input" type="url" spellcheck="false" />
      </label>
      <label class="field">
        <span class="field-label">API Key</span>
        <input id="apiKeyInput" class="text-input" type="password" spellcheck="false" placeholder="按需填写" />
      </label>
      <label class="field">
        <span class="field-label">模型</span>
        <div class="input-action-row">
          <input
            id="modelInput"
            class="text-input"
            type="text"
            spellcheck="false"
          />
          <button id="fetchModelsButton" class="ghost-button inline-button" type="button">
            拉取
          </button>
        </div>
        <div id="modelList" class="model-list" hidden></div>
        <span id="modelHint" class="field-help">可手动填写，或点击按钮拉取当前 Provider 的模型列表。</span>
      </label>
    </section>

    <section class="settings-card section-grid two-columns">
      <div class="section-head full-span">
        <div>
          <h2>实时参数</h2>
          <p>取自方案建议值，后续做去抖与稳定化时直接复用。</p>
        </div>
      </div>
      <label class="field">
        <span class="field-label">OCR 间隔 (ms)</span>
        <input id="ocrIntervalInput" class="text-input" type="number" min="250" max="4000" step="10" />
      </label>
      <label class="field">
        <span class="field-label">稳定帧数</span>
        <input id="stableFramesInput" class="text-input" type="number" min="1" max="10" step="1" />
      </label>
      <label class="field">
        <span class="field-label">相似度阈值</span>
        <input id="similarityThresholdInput" class="text-input" type="number" min="0.5" max="1" step="0.01" />
      </label>
      <label class="field">
        <span class="field-label">空白清空阈值 (ms)</span>
        <input id="blankClearInput" class="text-input" type="number" min="300" max="5000" step="10" />
      </label>
    </section>

    <footer class="footer-bar">
      <div id="statusText" class="status-text" data-tone="idle">已加载设置。</div>
      <div class="action-row">
        <button id="resetButton" class="ghost-button" type="button">恢复默认</button>
        <button id="closeButton" class="ghost-button" type="button">关闭</button>
        <button id="saveButton" class="primary-button" type="button">保存</button>
      </div>
    </footer>
  </main>
`;

function requireElement<T extends HTMLElement>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`Missing element: ${selector}`);
  }

  return element;
}

const realtimeInput = requireElement<HTMLInputElement>("#realtimeInput");
const providerInput = requireElement<HTMLSelectElement>("#providerInput");
const baseUrlInput = requireElement<HTMLInputElement>("#baseUrlInput");
const apiKeyInput = requireElement<HTMLInputElement>("#apiKeyInput");
const modelInput = requireElement<HTMLInputElement>("#modelInput");
const modelList = requireElement<HTMLDivElement>("#modelList");
const modelHint = requireElement<HTMLSpanElement>("#modelHint");
const ocrIntervalInput = requireElement<HTMLInputElement>("#ocrIntervalInput");
const stableFramesInput = requireElement<HTMLInputElement>("#stableFramesInput");
const similarityThresholdInput = requireElement<HTMLInputElement>("#similarityThresholdInput");
const blankClearInput = requireElement<HTMLInputElement>("#blankClearInput");
const providerHint = requireElement<HTMLParagraphElement>("#providerHint");
const statusText = requireElement<HTMLDivElement>("#statusText");
const saveButton = requireElement<HTMLButtonElement>("#saveButton");
const resetButton = requireElement<HTMLButtonElement>("#resetButton");
const closeButton = requireElement<HTMLButtonElement>("#closeButton");
const fetchModelsButton = requireElement<HTMLButtonElement>("#fetchModelsButton");

let currentSettings: SubtitleExplainSettings | null = null;
let isSaving = false;
let isFetchingModels = false;

async function init() {
  bindUi();
  const settings = await invoke<SubtitleExplainSettings>(
    "load_subtitle_explain_settings",
  );
  currentSettings = settings;
  render(settings);
}

function bindUi() {
  providerInput.addEventListener("change", () => {
    const provider = providerInput.value as SubtitleProvider;
    switchProvider(provider);
  });

  for (const input of [baseUrlInput, apiKeyInput, modelInput]) {
    input.addEventListener("input", () => {
      syncCurrentProviderFields();
    });
  }

  saveButton.addEventListener("click", () => {
    void saveSettings();
  });

  fetchModelsButton.addEventListener("click", () => {
    void fetchAvailableModels();
  });

  resetButton.addEventListener("click", () => {
    void resetSettings();
  });

  closeButton.addEventListener("click", () => {
    void dismissWindow();
  });

  window.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void saveSettings();
    }

    if (event.key === "Escape") {
      event.preventDefault();
      void dismissWindow();
    }
  });
}

function render(settings: SubtitleExplainSettings) {
  realtimeInput.checked = settings.realtimeEnabled;
  providerInput.value = settings.provider;
  applyProviderFields(settings, settings.provider);
  ocrIntervalInput.value = String(settings.ocrIntervalMs);
  stableFramesInput.value = String(settings.stableFrames);
  similarityThresholdInput.value = settings.similarityThreshold.toFixed(2);
  blankClearInput.value = String(settings.blankClearMs);
  updateProviderHint(settings.provider);
  clearModelSuggestions(settings.provider);
}

function updateProviderHint(provider: SubtitleProvider) {
  if (provider === "openAiResponses") {
    providerHint.textContent =
      "推荐填写 OpenAI 兼容地址，例如 https://api.openai.com/v1；API Key 一般必填。";
    baseUrlInput.placeholder = "https://api.openai.com/v1";
    modelInput.placeholder = "gpt-4.1-mini";
    apiKeyInput.placeholder = "sk-...";
    return;
  }

  providerHint.textContent =
    "本地 Ollama 常用地址为 http://127.0.0.1:11434；直连场景通常可以不填 API Key。";
  baseUrlInput.placeholder = "http://127.0.0.1:11434";
  modelInput.placeholder = "qwen2.5:7b";
  apiKeyInput.placeholder = "通常可留空";
}

function defaultModelHint(provider: SubtitleProvider) {
  if (provider === "openAiResponses") {
    return "可手动填写，也可用当前 base_url 和 API Key 拉取模型列表。";
  }

  return "可手动填写，也可从当前 Ollama 服务拉取已安装模型。";
}

function getProviderProfile(
  settings: SubtitleExplainSettings,
  provider: SubtitleProvider,
) {
  return settings.providerProfiles[provider];
}

function applyProviderFields(
  settings: SubtitleExplainSettings,
  provider: SubtitleProvider,
) {
  const profile = getProviderProfile(settings, provider);
  baseUrlInput.value = profile.baseUrl;
  apiKeyInput.value = profile.apiKey;
  modelInput.value = profile.model;
}

function syncCurrentProviderFields() {
  if (!currentSettings) {
    return;
  }

  const profile = getProviderProfile(currentSettings, currentSettings.provider);
  profile.baseUrl = baseUrlInput.value.trim();
  profile.apiKey = apiKeyInput.value;
  profile.model = modelInput.value.trim();
  currentSettings.baseUrl = profile.baseUrl;
  currentSettings.apiKey = profile.apiKey;
  currentSettings.model = profile.model;
}

function switchProvider(provider: SubtitleProvider) {
  if (!currentSettings) {
    updateProviderHint(provider);
    clearModelSuggestions(provider);
    return;
  }

  syncCurrentProviderFields();
  currentSettings.provider = provider;

  const profile = getProviderProfile(currentSettings, provider);
  currentSettings.baseUrl = profile.baseUrl;
  currentSettings.apiKey = profile.apiKey;
  currentSettings.model = profile.model;

  applyProviderFields(currentSettings, provider);
  updateProviderHint(provider);
  clearModelSuggestions(provider);
}

function setModelHint(message: string, tone: StatusTone = "idle") {
  modelHint.textContent = message;
  modelHint.dataset.tone = tone;
}

function renderModelSuggestions(models: string[]) {
  modelList.replaceChildren();

  for (const model of models) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "model-chip";
    button.textContent = model;
    button.title = `填入模型：${model}`;
    button.addEventListener("click", () => {
      modelInput.value = model;
      syncCurrentProviderFields();
      setModelHint(`已选中模型：${model}`, "success");
      setStatus(`已填入模型 ${model}。`, "success");
    });
    modelList.append(button);
  }

  modelList.hidden = models.length === 0;
}

function clearModelSuggestions(provider: SubtitleProvider) {
  renderModelSuggestions([]);
  modelList.hidden = true;
  setModelHint(defaultModelHint(provider), "idle");
}

function readNumber(input: HTMLInputElement, fallback: number) {
  const value = Number.parseFloat(input.value);
  return Number.isFinite(value) ? value : fallback;
}

function collectSettings(): SubtitleExplainSettings {
  if (!currentSettings) {
    throw new Error("Settings not initialized.");
  }

  currentSettings.provider = providerInput.value as SubtitleProvider;
  syncCurrentProviderFields();
  const activeProfile = getProviderProfile(currentSettings, currentSettings.provider);

  return {
    realtimeEnabled: realtimeInput.checked,
    provider: currentSettings.provider,
    explainMode: currentSettings.explainMode,
    baseUrl: activeProfile.baseUrl,
    apiKey: activeProfile.apiKey,
    model: activeProfile.model,
    providerProfiles: currentSettings.providerProfiles,
    ocrIntervalMs: readNumber(ocrIntervalInput, currentSettings.ocrIntervalMs),
    stableFrames: readNumber(stableFramesInput, currentSettings.stableFrames),
    similarityThreshold: readNumber(
      similarityThresholdInput,
      currentSettings.similarityThreshold,
    ),
    blankClearMs: readNumber(blankClearInput, currentSettings.blankClearMs),
  };
}

async function fetchAvailableModels() {
  if (isFetchingModels) {
    return;
  }

  const provider = providerInput.value as SubtitleProvider;

  try {
    isFetchingModels = true;
    fetchModelsButton.disabled = true;
    setStatus("正在拉取可用模型...", "idle");
    setModelHint("正在请求模型列表...", "idle");

    const models = await invoke<string[]>("fetch_available_subtitle_models", {
      provider,
      baseUrl: baseUrlInput.value.trim(),
      apiKey: apiKeyInput.value,
    });

    renderModelSuggestions(models);
    if (!modelInput.value.trim() && models.length > 0) {
      modelInput.value = models[0];
    }
    syncCurrentProviderFields();

    setModelHint(`已拉取 ${models.length} 个模型，可继续手动筛选或直接输入。`, "success");
    setStatus(`已拉取 ${models.length} 个可用模型。`, "success");
  } catch (error) {
    renderModelSuggestions([]);
    setModelHint(`拉取失败：${String(error)}`, "error");
    setStatus(`拉取模型失败：${String(error)}`, "error");
  } finally {
    isFetchingModels = false;
    fetchModelsButton.disabled = false;
  }
}

async function saveSettings() {
  if (isSaving) {
    return;
  }

  try {
    isSaving = true;
    setStatus("正在保存设置...", "idle");
    saveButton.disabled = true;
    const nextSettings = collectSettings();
    const saved = await invoke<SubtitleExplainSettings>(
      "save_subtitle_explain_settings",
      {
        nextSettings,
      },
    );
    currentSettings = saved;
    render(saved);
    setStatus("设置已保存。", "success");
  } catch (error) {
    setStatus(`保存失败：${String(error)}`, "error");
  } finally {
    isSaving = false;
    saveButton.disabled = false;
  }
}

async function resetSettings() {
  if (!window.confirm("确定恢复字幕解析相关默认配置吗？")) {
    return;
  }

  try {
    setStatus("正在恢复默认配置...", "idle");
    const settings = await invoke<SubtitleExplainSettings>(
      "reset_subtitle_explain_settings",
    );
    currentSettings = settings;
    render(settings);
    setStatus("已恢复默认配置。", "success");
  } catch (error) {
    setStatus(`恢复失败：${String(error)}`, "error");
  }
}

async function dismissWindow() {
  try {
    await appWindow.hide();
  } catch (error) {
    setStatus(`关闭失败：${String(error)}`, "error");
  }
}

function setStatus(message: string, tone: StatusTone) {
  statusText.textContent = message;
  statusText.dataset.tone = tone;
}

void init();
