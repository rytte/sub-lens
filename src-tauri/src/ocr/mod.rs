use serde::{Deserialize, Serialize};
use std::path::Path;

mod tesseract;
mod windows;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OcrEngine {
    WindowsOcr,
    Tesseract,
    EasyOcr,
    RapidOcr,
    PaddleOcr,
}

impl Default for OcrEngine {
    fn default() -> Self {
        if cfg!(target_os = "windows") {
            Self::WindowsOcr
        } else {
            Self::PaddleOcr
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OcrLanguage {
    Auto,
    ZhCn,
    ZhTw,
    En,
    Yue,
    Ja,
    Ko,
    Fr,
    Es,
    Ru,
    De,
    It,
    Tr,
    Pt,
    PtBr,
    Vi,
    Id,
    Th,
    Ms,
    Ar,
    Hi,
    Uk,
    He,
}

impl Default for OcrLanguage {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct TesseractOcrProfilePayload {
    pub page_seg_mode: u8,
}

impl Default for TesseractOcrProfilePayload {
    fn default() -> Self {
        Self { page_seg_mode: 6 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct OcrEngineProfilesPayload {
    pub tesseract: TesseractOcrProfilePayload,
}

impl Default for OcrEngineProfilesPayload {
    fn default() -> Self {
        Self {
            tesseract: TesseractOcrProfilePayload::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct OcrSettingsPayload {
    pub engine: OcrEngine,
    pub language: OcrLanguage,
    pub profiles: OcrEngineProfilesPayload,
}

impl Default for OcrSettingsPayload {
    fn default() -> Self {
        Self {
            engine: OcrEngine::default(),
            language: OcrLanguage::default(),
            profiles: OcrEngineProfilesPayload::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrEngineAvailabilityPayload {
    pub engine: OcrEngine,
    pub label: String,
    pub available: bool,
    pub reason: Option<String>,
}

pub fn sanitize_ocr_settings(mut settings: OcrSettingsPayload) -> OcrSettingsPayload {
    settings.profiles.tesseract.page_seg_mode =
        settings.profiles.tesseract.page_seg_mode.clamp(0, 13);

    if matches!(settings.engine, OcrEngine::WindowsOcr) && !cfg!(target_os = "windows") {
        settings.engine = OcrEngine::default();
    }

    settings
}

pub fn inspect_engines(_settings: &OcrSettingsPayload) -> Vec<OcrEngineAvailabilityPayload> {
    vec![
        windows::inspect_availability(),
        tesseract::inspect_availability(),
        OcrEngineAvailabilityPayload {
            engine: OcrEngine::EasyOcr,
            label: "EasyOCR".into(),
            available: true,
            reason: Some("运行时需要本地 Python、PyTorch 与 EasyOCR 环境。".into()),
        },
        OcrEngineAvailabilityPayload {
            engine: OcrEngine::RapidOcr,
            label: "RapidOCR".into(),
            available: true,
            reason: Some("运行时需要本地 Python 与 RapidOCR ONNX Runtime 环境。".into()),
        },
        OcrEngineAvailabilityPayload {
            engine: OcrEngine::PaddleOcr,
            label: "PaddleOCR".into(),
            available: true,
            reason: Some("运行时需要本地 Python 和 PaddleOCR 环境。".into()),
        },
    ]
}

pub fn recognize_text<F>(
    image_path: &Path,
    allow_empty: bool,
    settings: &OcrSettingsPayload,
    external_runner: F,
) -> Result<String, String>
where
    F: FnOnce(OcrEngine, &Path, bool) -> Result<String, String>,
{
    let raw_text = match settings.engine {
        OcrEngine::WindowsOcr => windows::recognize(image_path, &settings.language, allow_empty)?,
        OcrEngine::Tesseract => external_runner(OcrEngine::Tesseract, image_path, allow_empty)?,
        OcrEngine::EasyOcr | OcrEngine::RapidOcr | OcrEngine::PaddleOcr => {
            return external_runner(settings.engine, image_path, allow_empty);
        }
    };

    finalize_text(raw_text, &settings.language, allow_empty)
}

fn finalize_text(
    raw_text: String,
    language: &OcrLanguage,
    allow_empty: bool,
) -> Result<String, String> {
    let normalized = normalize_recognized_text(&raw_text, language);
    if normalized.is_empty() && !allow_empty {
        return Err("OCR 未识别到可用文本，请检查截图区域或切换 OCR 引擎。".into());
    }
    Ok(normalized)
}

fn normalize_recognized_text(raw_text: &str, language: &OcrLanguage) -> String {
    let compact = raw_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let compact = compact.split_whitespace().collect::<Vec<_>>().join(" ");

    if should_remove_spaces(language, &compact) {
        compact.replace(' ', "")
    } else {
        compact
    }
}

fn should_remove_spaces(language: &OcrLanguage, text: &str) -> bool {
    match language {
        OcrLanguage::ZhCn | OcrLanguage::ZhTw | OcrLanguage::Yue | OcrLanguage::Ja => true,
        OcrLanguage::Auto => looks_like_han_or_japanese(text),
        _ => false,
    }
}

fn looks_like_han_or_japanese(text: &str) -> bool {
    let mut cjk_count = 0usize;
    let mut visible_count = 0usize;

    for ch in text.chars() {
        if ch.is_whitespace() || ch.is_ascii_punctuation() {
            continue;
        }

        visible_count += 1;
        if matches!(
            ch as u32,
            0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xF900..=0xFAFF
                | 0x3040..=0x309F
                | 0x30A0..=0x30FF
        ) {
            cjk_count += 1;
        }
    }

    visible_count > 0 && cjk_count * 2 >= visible_count
}

pub(crate) fn windows_lang_code(language: &OcrLanguage) -> Option<&'static str> {
    match language {
        OcrLanguage::Auto => None,
        OcrLanguage::ZhCn => Some("zh-CN"),
        OcrLanguage::ZhTw => Some("zh-TW"),
        OcrLanguage::En => Some("en-US"),
        OcrLanguage::Yue => Some("zh-HK"),
        OcrLanguage::Ja => Some("ja-JP"),
        OcrLanguage::Ko => Some("ko-KR"),
        OcrLanguage::Fr => Some("fr-FR"),
        OcrLanguage::Es => Some("es-ES"),
        OcrLanguage::Ru => Some("ru-RU"),
        OcrLanguage::De => Some("de-DE"),
        OcrLanguage::It => Some("it-IT"),
        OcrLanguage::Tr => Some("tr-TR"),
        OcrLanguage::Pt => Some("pt-PT"),
        OcrLanguage::PtBr => Some("pt-BR"),
        OcrLanguage::Vi => Some("vi-VN"),
        OcrLanguage::Id => Some("id-ID"),
        OcrLanguage::Th => Some("th-TH"),
        OcrLanguage::Ms => Some("ms-MY"),
        OcrLanguage::Ar => Some("ar-SA"),
        OcrLanguage::Hi => Some("hi-IN"),
        OcrLanguage::Uk => Some("uk-UA"),
        OcrLanguage::He => Some("he-IL"),
    }
}

pub(crate) fn tesseract_langs(language: &OcrLanguage) -> Vec<&'static str> {
    match language {
        OcrLanguage::Auto => vec!["eng", "chi_sim", "jpn", "kor"],
        OcrLanguage::ZhCn => vec!["chi_sim"],
        OcrLanguage::ZhTw => vec!["chi_tra"],
        OcrLanguage::En => vec!["eng"],
        OcrLanguage::Yue => vec!["chi_sim"],
        OcrLanguage::Ja => vec!["jpn"],
        OcrLanguage::Ko => vec!["kor"],
        OcrLanguage::Fr => vec!["fra"],
        OcrLanguage::Es => vec!["spa"],
        OcrLanguage::Ru => vec!["rus"],
        OcrLanguage::De => vec!["deu"],
        OcrLanguage::It => vec!["ita"],
        OcrLanguage::Tr => vec!["tur"],
        OcrLanguage::Pt | OcrLanguage::PtBr => vec!["por"],
        OcrLanguage::Vi => vec!["vie"],
        OcrLanguage::Id => vec!["ind"],
        OcrLanguage::Th => vec!["tha"],
        OcrLanguage::Ms => vec!["msa"],
        OcrLanguage::Ar => vec!["ara"],
        OcrLanguage::Hi => vec!["hin"],
        OcrLanguage::Uk => vec!["ukr"],
        OcrLanguage::He => vec!["heb"],
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_recognized_text, OcrLanguage};

    #[test]
    fn removes_spaces_for_explicit_cjk_language() {
        let normalized = normalize_recognized_text("你 好 \n 世 界", &OcrLanguage::ZhCn);
        assert_eq!(normalized, "你好世界");
    }

    #[test]
    fn keeps_spaces_for_latin_language() {
        let normalized = normalize_recognized_text("hello   world", &OcrLanguage::En);
        assert_eq!(normalized, "hello world");
    }

    #[test]
    fn auto_mode_compacts_japanese_text() {
        let normalized = normalize_recognized_text("こ ん に ち は", &OcrLanguage::Auto);
        assert_eq!(normalized, "こんにちは");
    }
}
