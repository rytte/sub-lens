use super::{OcrEngine, OcrEngineAvailabilityPayload};

pub(crate) fn inspect_availability() -> OcrEngineAvailabilityPayload {
    OcrEngineAvailabilityPayload {
        engine: OcrEngine::Tesseract,
        label: "Tesseract.js".into(),
        available: true,
        reason: Some("内置浏览器端 OCR，首次使用会下载 WASM 和语言数据。".into()),
    }
}
