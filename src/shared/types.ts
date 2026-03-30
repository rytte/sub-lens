export type OverlayAppearance = "pixelate" | "solid";

export type OverlayResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

export type SubtitleExplainResizeDirection =
  | "East"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

export type OverlayState = {
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

export type OcrRegionState = {
  visible: boolean;
  showBorderWhenIdle: boolean;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
};

export type SubtitleExplainWindowState = {
  visible: boolean;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
};

export type SubtitleProvider = "openAiResponses" | "ollama";
export type SubtitleExplainMode = "phrase" | "word";
export type SubtitleResponseMode = "structured" | "fast";
export type TrayClickAction = "overlay" | "subtitleExplain";
export type StyleMode = "default" | "contrast" | "contrastLight";
export type StatusTone = "idle" | "success" | "error";
export type OcrEngine =
  | "windowsOcr"
  | "tesseract"
  | "easyOcr"
  | "rapidOcr"
  | "paddleOcr";
export type OcrLanguage =
  | "auto"
  | "zhCn"
  | "zhTw"
  | "en"
  | "yue"
  | "ja"
  | "ko"
  | "fr"
  | "es"
  | "ru"
  | "de"
  | "it"
  | "tr"
  | "pt"
  | "ptBr"
  | "vi"
  | "id"
  | "th"
  | "ms"
  | "ar"
  | "hi"
  | "uk"
  | "he";

export type ProviderConnectionSettings = {
  baseUrl: string;
  apiKey: string;
  model: string;
};

export type TesseractOcrProfile = {
  pageSegMode: number;
};

export type OcrEngineProfiles = {
  tesseract: TesseractOcrProfile;
};

export type OcrSettings = {
  engine: OcrEngine;
  language: OcrLanguage;
  profiles: OcrEngineProfiles;
};

export type OcrEngineAvailability = {
  engine: OcrEngine;
  label: string;
  available: boolean;
  reason: string | null;
};

export type SubtitleProviderProfiles = {
  openAiResponses: ProviderConnectionSettings;
  ollama: ProviderConnectionSettings;
};

export type ShortcutSettings = {
  toggleOverlayVisibility: string;
  toggleOverlayLock: string;
  editOcrRegion: string;
  startStaticScreenSelection: string;
  analyzeCurrentSubtitle: string;
};

export type SubtitleExplainSettings = {
  realtimeEnabled: boolean;
  provider: SubtitleProvider;
  explainMode: SubtitleExplainMode;
  responseMode: SubtitleResponseMode;
  trayClickAction: TrayClickAction;
  baseUrl: string;
  apiKey: string;
  model: string;
  providerProfiles: SubtitleProviderProfiles;
  ocrSettings: OcrSettings;
  ocrIntervalMs: number;
  stableFrames: number;
  similarityThreshold: number;
  blankClearMs: number;
  shortcutSettings: ShortcutSettings;
};

export type SubtitlePhrase = {
  source: string;
  explanation: string;
};

export type SubtitleResult = {
  sourceText: string;
  responseMode: SubtitleResponseMode;
  translation: string;
  phrases: SubtitlePhrase[];
  rawText: string;
};

export type SubtitleStatus = {
  analyzing: boolean;
  stage: string;
  message: string;
  error: string | null;
};

export const FONT_SCALE_VALUES = ["1.0", "1.1", "1.2", "1.3", "1.4", "1.5"] as const;

export type FontScale = (typeof FONT_SCALE_VALUES)[number];

export const DEFAULT_FONT_SCALE: FontScale = FONT_SCALE_VALUES[0];

export const LEGACY_FONT_SCALE_MAP = {
  normal: "1.0",
  large: "1.1",
  xlarge: "1.2",
} as const;
