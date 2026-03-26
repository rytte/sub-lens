use std::path::Path;

use super::{windows_lang_code, OcrEngine, OcrEngineAvailabilityPayload, OcrLanguage};

#[cfg(target_os = "windows")]
pub(crate) fn inspect_availability() -> OcrEngineAvailabilityPayload {
    OcrEngineAvailabilityPayload {
        engine: OcrEngine::WindowsOcr,
        label: "Windows OCR".into(),
        available: true,
        reason: Some("需要系统已安装对应语言包。".into()),
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn inspect_availability() -> OcrEngineAvailabilityPayload {
    OcrEngineAvailabilityPayload {
        engine: OcrEngine::WindowsOcr,
        label: "Windows OCR".into(),
        available: false,
        reason: Some("仅 Windows 平台支持。".into()),
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn recognize(
    image_path: &Path,
    language: &OcrLanguage,
    allow_empty: bool,
) -> Result<String, String> {
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    let path = image_path.to_string_lossy().replace("\\\\?\\", "");
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))
        .map_err(|err| format!("Windows OCR 打开图片失败：{err}"))?
        .get()
        .map_err(|err| format!("Windows OCR 读取图片失败：{err}"))?;

    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .map_err(|err| format!("Windows OCR 打开图片流失败：{err}"))?
        .get()
        .map_err(|err| format!("Windows OCR 读取图片流失败：{err}"))?;

    let bitmap = BitmapDecoder::CreateWithIdAsync(
        BitmapDecoder::PngDecoderId().map_err(|err| err.to_string())?,
        &stream,
    )
    .map_err(|err| format!("Windows OCR 创建位图解码器失败：{err}"))?
    .get()
    .map_err(|err| format!("Windows OCR 解码图片失败：{err}"))?
    .GetSoftwareBitmapAsync()
    .map_err(|err| format!("Windows OCR 获取位图失败：{err}"))?
    .get()
    .map_err(|err| format!("Windows OCR 获取位图失败：{err}"))?;

    // 官方建议 OCR 输入使用 Gray8 或 Bgra8；这里统一转成 Bgra8 以减少格式兼容问题。
    let bitmap = SoftwareBitmap::Convert(&bitmap, BitmapPixelFormat::Bgra8)
        .map_err(|err| format!("Windows OCR 位图格式转换失败：{err}"))?;

    let engine = match windows_lang_code(language) {
        Some(lang) => {
            let language = Language::CreateLanguage(&HSTRING::from(lang))
                .map_err(|err| format!("Windows OCR 语言配置无效：{err}"))?;
            OcrEngine::TryCreateFromLanguage(&language)
        }
        None => OcrEngine::TryCreateFromUserProfileLanguages(),
    }
    .map_err(|err| {
        let message = err.to_string();
        if message.contains("0x00000000") {
            "Windows OCR 缺少对应语言包，请在系统语言设置里安装 OCR 语言支持。".into()
        } else {
            format!("Windows OCR 初始化失败：{message}")
        }
    })?;

    let text = engine
        .RecognizeAsync(&bitmap)
        .map_err(|err| format!("Windows OCR 启动识别失败：{err}"))?
        .get()
        .map_err(|err| format!("Windows OCR 等待识别结果失败：{err}"))?
        .Text()
        .map_err(|err| format!("Windows OCR 读取结果失败：{err}"))?
        .to_string_lossy();

    if text.trim().is_empty() && !allow_empty {
        return Err("Windows OCR 未识别到可用文本。已自动放大图片；如果仍为空，通常是语言包不匹配、字幕太小，或当前边框没有框到正文。建议先显式切到字幕语言再试。".into());
    }

    Ok(text)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn recognize(
    _image_path: &Path,
    _language: &OcrLanguage,
    _allow_empty: bool,
) -> Result<String, String> {
    Err("当前系统不支持 Windows OCR。".into())
}
