from __future__ import annotations

import hashlib
import json
import os
import shutil
import socket
import ssl
import tempfile
import sys
import time
import traceback
import unicodedata
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from typing import Any


SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "translation": {"type": "string"},
        "phrases": {
            "type": "array",
            "maxItems": 18,
            "items": {
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "explanation": {"type": "string"},
                },
                "required": ["source", "explanation"],
                "additionalProperties": False,
            },
        },
    },
    "required": [
        "translation",
        "phrases",
    ],
    "additionalProperties": False,
}

TRIMMABLE_EDGE_PUNCTUATION = frozenset(
    "\"'“”‘’「」『』【】[]{}〈〉《》〔〕…！？!?,.;:，、。；：·"
)


def contains_japanese_kana(text: str) -> bool:
    for char in text:
        codepoint = ord(char)
        if 0x3040 <= codepoint <= 0x30FF or 0x31F0 <= codepoint <= 0x31FF:
            return True
    return False


def should_apply_japanese_rules(language: str, source_text: str) -> bool:
    normalized_language = language.strip() or "auto"
    return normalized_language == "ja" or contains_japanese_kana(source_text)


def trim_phrase_edge_punctuation(text: str) -> str:
    value = text.strip()
    while value and value[0] in TRIMMABLE_EDGE_PUNCTUATION:
        value = value[1:].lstrip()
    while value and value[-1] in TRIMMABLE_EDGE_PUNCTUATION:
        value = value[:-1].rstrip()
    return value


def is_punctuation_only(text: str) -> bool:
    if not text:
        return True

    for char in text:
        if char.isspace():
            continue
        if not unicodedata.category(char).startswith("P"):
            return False
    return True


def normalize_phrase_source(source: str, *, japanese_mode: bool) -> str:
    value = source.strip().replace("（", "(").replace("）", ")")
    value = trim_phrase_edge_punctuation(value)

    if japanese_mode:
        value = value.replace(" (", "(").replace("( ", "(").replace(" )", ")")

    return value.strip()


def build_system_prompt(explain_mode: str, *, japanese_mode: bool) -> str:
    if explain_mode == "word":
        rules = [
            "你是一个字幕学习助手。",
            "你会收到一行 OCR 后的字幕文本，请输出严格符合 JSON Schema 的中文解释结果。",
            "要求：",
            "1. translation：给出自然、简洁的整句中文翻译。",
            "2. phrases：按原句顺序，尽量逐词或逐个语法成分拆开解释，最多输出 18 项。",
            "3. 请优先拆出单词、助词、时态变化、固定搭配中的关键组成部分，而不是只挑宽泛短语。",
            "4. phrases 中每一项必须使用 source 和 explanation 作为字段名，不要使用其他键名。",
            "5. explanation 用简体中文，简洁说明词义、语法作用或在句中的含义。",
            "6. 如果原句是日语等不以空格分词的语言，需要主动完成合理切分。",
            "7. 忽略标点符号，不要把任何标点符号单独作为 phrases 项；如果条目首尾带标点，去掉标点后再解释。",
            "8. 所有内容使用简体中文，不要输出任何 JSON 之外的文字。",
        ]
    else:
        rules = [
            "你是一个字幕学习助手。",
            "你会收到一行 OCR 后的字幕文本，请输出严格符合 JSON Schema 的中文解释结果。",
            "要求：",
            "1. translation：给出自然、简洁的整句中文翻译。",
            "2. phrases：挑 0~18 个关键短语或搭配，逐个用中文解释。",
            "3. phrases 中每一项必须使用 source 和 explanation 作为字段名，不要使用其他键名。",
            "4. 忽略标点符号，不要把任何标点符号单独作为 phrases 项；如果条目首尾带标点，去掉标点后再解释。",
            "5. 所有内容使用简体中文。",
            "6. 不要输出任何 JSON 之外的文字。",
        ]

    if japanese_mode:
        rules.append(
            "9. 如果原句是日语，phrases 中凡是带汉字的 source，都要用半角小括号标注对应假名，例如 学校(がっこう)。"
            if explain_mode == "word"
            else "7. 如果原句是日语，phrases 中凡是带汉字的 source，都要用半角小括号标注对应假名，例如 学校(がっこう)。"
        )

    return "\n".join(rules)

ocr_instances: dict[str, Any] = {}
easyocr_download_patched = False
OCR_SCORE_THRESHOLD = float(os.environ.get("SUB_LENS_OCR_SCORE_THRESHOLD", "0.20"))
HTTP_RETRY_COUNT = max(0, int(os.environ.get("SUB_LENS_HTTP_RETRY_COUNT", "2")))
HTTP_RETRY_DELAY_MS = max(0, int(os.environ.get("SUB_LENS_HTTP_RETRY_DELAY_MS", "400")))


def configure_stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if callable(reconfigure):
            reconfigure(encoding="utf-8", errors="replace")


def emit(payload: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def error_response(message: str) -> dict[str, Any]:
    return {"ok": False, "error": message}


def success_response(result: dict[str, Any]) -> dict[str, Any]:
    return {"ok": True, "result": result}


def normalize_base_url(base_url: str, suffix: str) -> str:
    return base_url.rstrip("/") + suffix


def resolve_cache_dir(env_key: str, subdir: str) -> Path:
    configured = str(os.environ.get(env_key) or "").strip()
    if configured:
        return Path(configured).expanduser().resolve()

    if sys.platform.startswith("win"):
        local_app_data = str(os.environ.get("LOCALAPPDATA") or "").strip()
        base_dir = Path(local_app_data) if local_app_data else (Path.home() / "AppData" / "Local")
    else:
        xdg_cache_home = str(os.environ.get("XDG_CACHE_HOME") or "").strip()
        base_dir = Path(xdg_cache_home) if xdg_cache_home else (Path.home() / ".cache")

    return (base_dir / "sub-lens" / subdir).resolve()


EASYOCR_MODEL_DIR = resolve_cache_dir("SUB_LENS_EASYOCR_MODEL_DIR", "easyocr")
RAPIDOCR_MODEL_DIR = resolve_cache_dir("SUB_LENS_RAPIDOCR_MODEL_DIR", "rapidocr")

RAPIDOCR_MODELS: dict[str, dict[str, str]] = {
    "ch_PP-OCRv4_det_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/det/ch_PP-OCRv4_det_infer.onnx",
        "sha256": "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
    },
    "en_PP-OCRv3_det_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/det/en_PP-OCRv3_det_infer.onnx",
        "sha256": "ea07c15d38ac40cd69da3c493444ec75b44ff23840553ff8ba102c1219ed39c2",
    },
    "Multilingual_PP-OCRv3_det_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/det/Multilingual_PP-OCRv3_det_infer.onnx",
        "sha256": "5475c6c7f4d84a6c4f32241b487435d59f126a40c023387af99732258844cdc3",
    },
    "ch_PP-OCRv4_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/ch_PP-OCRv4_rec_infer.onnx",
        "sha256": "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
    },
    "chinese_cht_PP-OCRv3_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/chinese_cht_PP-OCRv3_rec_infer.onnx",
        "sha256": "779656d044ce388045e02ea9244724616194e63928606436cdfc6dc3c9528cc6",
    },
    "en_PP-OCRv4_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/en_PP-OCRv4_rec_infer.onnx",
        "sha256": "e8770c967605983d1570cdf5352041dfb68fa0c21664f49f47b155abd3e0e318",
    },
    "japan_PP-OCRv4_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/japan_PP-OCRv4_rec_infer.onnx",
        "sha256": "e1075a67dba758ecfc7ebc78a10ae61c95ac8fb66a9c86fab5541e33f085cb7a",
    },
    "korean_PP-OCRv4_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/korean_PP-OCRv4_rec_infer.onnx",
        "sha256": "ab151ba9065eccd98f884cf4d927db091be86137276392072edd4f9d43ad7426",
    },
    "latin_PP-OCRv3_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/latin_PP-OCRv3_rec_infer.onnx",
        "sha256": "e9d7a33667e8aaa702862975186adf2012e3f390cc0f9422865957125f8071cf",
    },
    "cyrillic_PP-OCRv3_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/cyrillic_PP-OCRv3_rec_infer.onnx",
        "sha256": "1efb65bdc460af1c0e8733d005b20952b17ca5aac10ddb56c968333791c5eaa3",
    },
    "arabic_PP-OCRv4_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/arabic_PP-OCRv4_rec_infer.onnx",
        "sha256": "4a9011bef71687bb84288dc86ad2471bd5d37b717ddf672dd156f9e7a5601bac",
    },
    "devanagari_PP-OCRv4_rec_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/rec/devanagari_PP-OCRv4_rec_infer.onnx",
        "sha256": "a62b6148303187907aa0b0d3a0125bdc62557d07966468cab9056949e36035e8",
    },
    "th_PP-OCRv5_rec_mobile_infer.onnx": {
        "url": "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv5/rec/th_PP-OCRv5_rec_mobile_infer.onnx",
        "sha256": "de541dd83161c241ff426f7ecfd602a0ba77d686cf3ab9a6c255ea82fd08006e",
    },
}

RAPIDOCR_LANGUAGE_PROFILES: dict[str, dict[str, str]] = {
    "zhCn": {"det": "ch_PP-OCRv4_det_infer.onnx", "rec": "ch_PP-OCRv4_rec_infer.onnx"},
    "zhTw": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "chinese_cht_PP-OCRv3_rec_infer.onnx"},
    "yue": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "chinese_cht_PP-OCRv3_rec_infer.onnx"},
    "en": {"det": "en_PP-OCRv3_det_infer.onnx", "rec": "en_PP-OCRv4_rec_infer.onnx"},
    "ja": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "japan_PP-OCRv4_rec_infer.onnx"},
    "ko": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "korean_PP-OCRv4_rec_infer.onnx"},
    "fr": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "es": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "de": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "it": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "tr": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "pt": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "ptBr": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "vi": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "id": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "ms": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "latin_PP-OCRv3_rec_infer.onnx"},
    "ru": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "cyrillic_PP-OCRv3_rec_infer.onnx"},
    "uk": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "cyrillic_PP-OCRv3_rec_infer.onnx"},
    "ar": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "arabic_PP-OCRv4_rec_infer.onnx"},
    "hi": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "devanagari_PP-OCRv4_rec_infer.onnx"},
    "th": {"det": "Multilingual_PP-OCRv3_det_infer.onnx", "rec": "th_PP-OCRv5_rec_mobile_infer.onnx"},
}


def write_debug_file(prefix: str, content: str) -> str:
    safe_prefix = prefix.replace("/", "-")
    path = Path(tempfile.gettempdir()) / f"sub-lens-{safe_prefix}-{int(time.time() * 1000)}.txt"
    path.write_text(content, encoding="utf-8", errors="replace")
    return str(path)


def strip_code_fences(text: str) -> str:
    value = text.strip()
    if value.startswith("```"):
        lines = value.splitlines()
        if lines and lines[0].startswith("```"):
            lines = lines[1:]
        if lines and lines[-1].strip() == "```":
            lines = lines[:-1]
        value = "\n".join(lines).strip()
    return value


def extract_first_json_block(text: str) -> str:
    value = strip_code_fences(text)
    if not value:
        return ""

    start = -1
    opening = ""
    closing = ""
    for index, char in enumerate(value):
        if char == "{":
            start = index
            opening, closing = "{", "}"
            break
        if char == "[":
            start = index
            opening, closing = "[", "]"
            break

    if start < 0:
        return value

    depth = 0
    in_string = False
    escaped = False
    for index in range(start, len(value)):
        char = value[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue

        if char == '"':
            in_string = True
            continue

        if char == opening:
            depth += 1
        elif char == closing:
            depth -= 1
            if depth == 0:
                return value[start : index + 1]

    return value[start:]


def escape_invalid_json_backslashes(value: str) -> str:
    repaired: list[str] = []
    in_string = False
    index = 0
    valid_escapes = {'"', "\\", "/", "b", "f", "n", "r", "t", "u"}

    while index < len(value):
        char = value[index]

        if not in_string:
            repaired.append(char)
            if char == '"':
                in_string = True
            index += 1
            continue

        if char == '"':
            repaired.append(char)
            in_string = False
            index += 1
            continue

        if char == "\\":
            next_char = value[index + 1] if index + 1 < len(value) else ""
            if next_char in valid_escapes:
                repaired.append(char)
            else:
                repaired.append("\\\\")
            index += 1
            continue

        repaired.append(char)
        index += 1

    return "".join(repaired)


def parse_json_text(text: str, label: str) -> dict[str, Any]:
    candidate = extract_first_json_block(text)
    if not candidate.strip():
        raw_path = write_debug_file(label, text)
        raise RuntimeError(f"{label} 返回为空。原始内容已保存到：{raw_path}")

    try:
        parsed = json.loads(candidate)
    except json.JSONDecodeError as exc:
        repaired_candidate = escape_invalid_json_backslashes(candidate)
        if repaired_candidate != candidate:
            try:
                parsed = json.loads(repaired_candidate)
            except json.JSONDecodeError:
                parsed = None
            else:
                if not isinstance(parsed, dict):
                    raw_path = write_debug_file(label, text)
                    raise RuntimeError(f"{label} 返回的 JSON 不是对象。原始内容已保存到：{raw_path}")
                return parsed

        raw_path = write_debug_file(label, text)
        raise RuntimeError(
            f"{label} 返回的内容不是合法 JSON：{exc}。原始内容已保存到：{raw_path}"
        ) from exc

    if not isinstance(parsed, dict):
        raw_path = write_debug_file(label, text)
        raise RuntimeError(f"{label} 返回的 JSON 不是对象。原始内容已保存到：{raw_path}")
    return parsed


def post_json(url: str, payload: dict[str, Any], headers: dict[str, str]) -> dict[str, Any]:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", **headers},
        method="POST",
    )

    for attempt in range(HTTP_RETRY_COUNT + 1):
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                body = response.read().decode("utf-8", errors="replace")
                return parse_json_text(body, "接口响应")
        except urllib.error.HTTPError:
            raise
        except Exception as exc:
            if attempt >= HTTP_RETRY_COUNT or not should_retry_network_error(exc):
                raise RuntimeError(f"{url} 请求失败：{exc}") from exc
            time.sleep((HTTP_RETRY_DELAY_MS / 1000) * (attempt + 1))

    raise RuntimeError(f"{url} 请求失败：已超过最大重试次数。")


def should_retry_network_error(exc: BaseException) -> bool:
    if isinstance(exc, (socket.timeout, TimeoutError, ConnectionResetError, ssl.SSLError)):
        return True

    if isinstance(exc, urllib.error.URLError):
        return should_retry_network_error(exc.reason)

    message = str(exc)
    return any(
        token in message
        for token in (
            "UNEXPECTED_EOF_WHILE_READING",
            "EOF occurred in violation of protocol",
            "timed out",
            "Connection reset",
            "Remote end closed connection",
        )
    )


def merge_ocr_candidates(
    entries: list[tuple[str, float | None]],
    *,
    allow_empty: bool,
    image_path: str,
    engine_label: str,
) -> str:
    if not entries:
        if allow_empty:
            return ""
        raise RuntimeError(
            f"{engine_label} 未识别到可用字幕文本。未检测到任何文字框。请检查 OCR 区域是否框准字幕。调试截图：{image_path}"
        )

    collected: list[str] = []
    candidates: list[str] = []
    for raw_text, score in entries:
        text = str(raw_text or "").strip()
        if not text:
            continue

        if score is not None:
            candidates.append(f"{text} ({score:.2f})")
        else:
            candidates.append(text)

        if score is not None and score < OCR_SCORE_THRESHOLD:
            continue
        collected.append(text)

    merged = " ".join(collected)
    merged = " ".join(merged.split())
    if merged:
        return merged

    if candidates:
        if allow_empty:
            return ""
        preview = "；".join(candidates[:8])
        raise RuntimeError(
            f"{engine_label} 未识别到可用字幕文本。当前候选均低于阈值 {OCR_SCORE_THRESHOLD:.2f}。"
            f"候选：{preview}。调试截图：{image_path}"
        )

    if allow_empty:
        return ""

    raise RuntimeError(
        f"{engine_label} 未识别到可用字幕文本。未检测到任何文字框。请检查 OCR 区域是否框准字幕。调试截图：{image_path}"
    )


def normalize_sidecar_ocr_engine(engine: str) -> str:
    normalized = engine.strip()
    if normalized in {"easyOcr", "rapidOcr", "paddleOcr"}:
        return normalized
    raise RuntimeError(f"Python sidecar 不支持的 OCR 引擎：{engine}")


def easyocr_language_list(language: str) -> list[str]:
    mapping = {
        "auto": ["ch_sim", "en"],
        "zhCn": ["ch_sim", "en"],
        "zhTw": ["ch_tra", "en"],
        "en": ["en"],
        "yue": ["ch_tra", "en"],
        "ja": ["ja", "en"],
        "ko": ["ko", "en"],
        "fr": ["fr", "en"],
        "es": ["es", "en"],
        "ru": ["ru", "en"],
        "de": ["de", "en"],
        "it": ["it", "en"],
        "tr": ["tr", "en"],
        "pt": ["pt", "en"],
        "ptBr": ["pt", "en"],
        "vi": ["vi", "en"],
        "id": ["id", "en"],
        "th": ["th", "en"],
        "ms": ["ms", "en"],
        "ar": ["ar", "en"],
        "hi": ["hi", "en"],
        "uk": ["uk", "en"],
        # EasyOCR 当前不支持 Hebrew，退回英文模型避免初始化直接失败。
        "he": ["en"],
    }
    return mapping.get(language, ["ch_sim", "en"])


def patch_easyocr_download() -> None:
    global easyocr_download_patched
    if easyocr_download_patched:
        return

    import easyocr.easyocr as easyocr_impl

    def download_and_unzip(
        url: str,
        filename: str,
        model_storage_directory: str,
        verbose: bool = True,
    ) -> None:
        del verbose
        model_dir = Path(model_storage_directory)
        model_dir.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(suffix=".zip", delete=False, dir=model_dir) as tmp_file:
            tmp_zip_path = Path(tmp_file.name)
        try:
            with urllib.request.urlopen(url, timeout=300) as response, tmp_zip_path.open("wb") as output:
                shutil.copyfileobj(response, output)
            with zipfile.ZipFile(tmp_zip_path, "r") as zip_obj:
                zip_obj.extract(filename, model_dir)
        finally:
            if tmp_zip_path.exists():
                tmp_zip_path.unlink()

    easyocr_impl.download_and_unzip = download_and_unzip
    easyocr_download_patched = True


def compute_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_file(url: str, destination: Path, expected_sha256: str | None = None) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() and destination.is_file():
        if expected_sha256 is None or compute_sha256(destination).lower() == expected_sha256.lower():
            return destination
        destination.unlink()

    with tempfile.NamedTemporaryFile(
        suffix=destination.suffix or ".tmp",
        delete=False,
        dir=destination.parent,
    ) as tmp_file:
        tmp_path = Path(tmp_file.name)

    try:
        print(f"subtitle sidecar: 正在下载 RapidOCR 模型 {destination.name}", file=sys.stderr)
        with urllib.request.urlopen(url, timeout=300) as response, tmp_path.open("wb") as output:
            shutil.copyfileobj(response, output)

        if expected_sha256 is not None:
            actual_sha256 = compute_sha256(tmp_path)
            if actual_sha256.lower() != expected_sha256.lower():
                raise RuntimeError(
                    f"RapidOCR 模型校验失败：{destination.name}，期望 {expected_sha256}，实际 {actual_sha256}"
                )

        tmp_path.replace(destination)
        return destination
    finally:
        if tmp_path.exists():
            tmp_path.unlink(missing_ok=True)


def rapidocr_profile(language: str) -> dict[str, str] | None:
    return RAPIDOCR_LANGUAGE_PROFILES.get(language)


def ensure_rapidocr_model(model_name: str) -> Path:
    metadata = RAPIDOCR_MODELS.get(model_name)
    if metadata is None:
        raise RuntimeError(f"未找到 RapidOCR 官方模型配置：{model_name}")
    return download_file(
        metadata["url"],
        RAPIDOCR_MODEL_DIR / model_name,
        metadata.get("sha256"),
    )


def ensure_paddle_ocr():
    cache_key = "paddleOcr"
    existing = ocr_instances.get(cache_key)
    if existing is not None:
        return existing

    os.environ.setdefault("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
    try:
        from paddleocr import PaddleOCR
    except Exception as exc:  # pragma: no cover - runtime dependency
        raise RuntimeError(
            "无法导入 paddleocr。请先执行 `pip install -r src-tauri/sidecars/requirements.txt`，"
            "再按 PaddleOCR 官方文档安装 paddlepaddle。"
        ) from exc

    instance = PaddleOCR(
        use_doc_orientation_classify=False,
        use_doc_unwarping=False,
        use_textline_orientation=False,
        # PaddleOCR 3.x on Windows CPU can fail inside oneDNN during predict().
        enable_mkldnn=False,
    )
    ocr_instances[cache_key] = instance
    return instance


def ensure_easyocr(language: str):
    languages = easyocr_language_list(language)
    cache_key = f"easyOcr:{'+'.join(languages)}"
    existing = ocr_instances.get(cache_key)
    if existing is not None:
        return existing

    try:
        import easyocr
    except Exception as exc:  # pragma: no cover - runtime dependency
        raise RuntimeError(
            "无法导入 easyocr。请先执行 `pip install -r src-tauri/sidecars/requirements.txt`，"
            "并按 PyTorch 官方文档安装与本机匹配的 torch / torchvision。"
        ) from exc

    EASYOCR_MODEL_DIR.mkdir(parents=True, exist_ok=True)
    patch_easyocr_download()
    instance = easyocr.Reader(
        languages,
        gpu=False,
        model_storage_directory=str(EASYOCR_MODEL_DIR),
        detect_network="craft",
        recog_network="standard",
        download_enabled=True,
        verbose=False,
    )
    ocr_instances[cache_key] = instance
    return instance


def ensure_rapidocr(language: str):
    profile = rapidocr_profile(language)
    cache_key = f"rapidOcr:{language}"
    existing = ocr_instances.get(cache_key)
    if existing is not None:
        return existing

    try:
        from rapidocr_onnxruntime import RapidOCR
    except Exception as exc:  # pragma: no cover - runtime dependency
        raise RuntimeError(
            "无法导入 rapidocr_onnxruntime。请先执行 `pip install -r src-tauri/sidecars/requirements.txt`。"
        ) from exc

    kwargs: dict[str, Any] = {
        "print_verbose": False,
        "text_score": OCR_SCORE_THRESHOLD,
    }
    if profile is not None:
        kwargs["det_model_path"] = str(ensure_rapidocr_model(profile["det"]))
        kwargs["rec_model_path"] = str(ensure_rapidocr_model(profile["rec"]))
    else:
        print(
            f"subtitle sidecar: RapidOCR 当前语言 {language} 没有专用模型，回退默认配置。",
            file=sys.stderr,
        )

    instance = RapidOCR(**kwargs)
    ocr_instances[cache_key] = instance
    return instance


def extract_with_paddleocr(image_path: str, *, allow_empty: bool = False) -> str:
    ocr = ensure_paddle_ocr()
    result = ocr.predict(image_path)
    if not result:
        if allow_empty:
            return ""
        raise RuntimeError(f"PaddleOCR 未返回任何结果。请检查 OCR 区域是否框准字幕。调试截图：{image_path}")

    first = result[0]
    if hasattr(first, "res"):
        res = getattr(first, "res", None) or {}
    elif isinstance(first, dict):
        res = first
    elif hasattr(first, "items"):
        try:
            res = dict(first.items())
        except Exception:
            res = {}
    else:
        try:
            res = dict(first)
        except Exception:
            res = {}

    texts = res.get("rec_texts") or []
    scores = res.get("rec_scores") or []
    entries: list[tuple[str, float | None]] = []
    for index, raw_text in enumerate(texts):
        score = None
        if index < len(scores):
            try:
                score = float(scores[index])
            except Exception:
                score = None
        entries.append((str(raw_text or ""), score))

    return merge_ocr_candidates(
        entries,
        allow_empty=allow_empty,
        image_path=image_path,
        engine_label="PaddleOCR",
    )


def extract_with_easyocr(image_path: str, language: str, *, allow_empty: bool = False) -> str:
    reader = ensure_easyocr(language)
    results = reader.readtext(str(image_path), detail=1, paragraph=False)
    if not results:
        if allow_empty:
            return ""
        raise RuntimeError(f"EasyOCR 未返回任何结果。请检查 OCR 区域是否框准字幕。调试截图：{image_path}")

    entries: list[tuple[str, float | None]] = []
    for item in results:
        if not isinstance(item, (list, tuple)) or len(item) < 2:
            continue
        score = None
        if len(item) >= 3 and item[2] is not None:
            try:
                score = float(item[2])
            except Exception:
                score = None
        entries.append((str(item[1] or ""), score))

    return merge_ocr_candidates(
        entries,
        allow_empty=allow_empty,
        image_path=image_path,
        engine_label="EasyOCR",
    )


def extract_with_rapidocr_language(
    image_path: str,
    language: str,
    *,
    allow_empty: bool = False,
) -> str:
    engine = ensure_rapidocr(language)
    results, _elapsed = engine(str(image_path))
    if not results:
        if allow_empty:
            return ""
        raise RuntimeError(f"RapidOCR 未返回任何结果。请检查 OCR 区域是否框准字幕。调试截图：{image_path}")

    entries: list[tuple[str, float | None]] = []
    for item in results:
        if not isinstance(item, (list, tuple)) or len(item) < 2:
            continue
        score = None
        if len(item) >= 3 and item[2] is not None:
            try:
                score = float(item[2])
            except Exception:
                score = None
        entries.append((str(item[1] or ""), score))

    return merge_ocr_candidates(
        entries,
        allow_empty=allow_empty,
        image_path=image_path,
        engine_label="RapidOCR",
    )


def extract_ocr_text(
    image_path: str,
    *,
    engine: str,
    language: str,
    allow_empty: bool = False,
) -> str:
    sidecar_engine = normalize_sidecar_ocr_engine(engine)
    if sidecar_engine == "easyOcr":
        return extract_with_easyocr(image_path, language, allow_empty=allow_empty)
    if sidecar_engine == "rapidOcr":
        return extract_with_rapidocr_language(image_path, language, allow_empty=allow_empty)
    if sidecar_engine == "paddleOcr":
        return extract_with_paddleocr(image_path, allow_empty=allow_empty)
    raise RuntimeError(f"Python sidecar 不支持的 OCR 引擎：{engine}")


def subtitle_explain_language(settings: dict[str, Any]) -> str:
    ocr_settings = settings.get("ocrSettings") or {}
    return str(ocr_settings.get("language") or "auto").strip() or "auto"


def build_user_prompt(source_text: str, explain_mode: str, *, japanese_mode: bool) -> str:
    if explain_mode == "word":
        prompt = (
            "请解释下面这句字幕，并尽量按原句顺序逐词或逐个语法成分拆开说明，"
            "严格按约定 JSON 输出。"
        )
    else:
        prompt = "请解释下面这句字幕，并严格按约定 JSON 输出。"

    extra_requirements = [
        "忽略标点符号，不要解释单独的标点。",
    ]
    if japanese_mode:
        extra_requirements.append(
            "如果是日语，带汉字的 source 请用半角小括号标注假名，例如 学校(がっこう)。"
        )

    return f"{prompt}\n" + "\n".join(extra_requirements) + f"\n\n字幕：{source_text}"


def extract_openai_output_text(response: dict[str, Any]) -> str:
    top_level = response.get("output_text")
    if isinstance(top_level, str) and top_level.strip():
        return top_level

    choices = response.get("choices")
    if isinstance(choices, list) and choices:
        message = choices[0].get("message", {}) if isinstance(choices[0], dict) else {}
        content = message.get("content")
        if isinstance(content, str) and content.strip():
            return content
        if isinstance(content, list):
            for item in content:
                if isinstance(item, dict):
                    text = item.get("text") or item.get("content")
                    if isinstance(text, str) and text.strip():
                        return text

    for item in response.get("output", []):
        if item.get("type") != "message":
            continue
        for content in item.get("content", []):
            if content.get("type") in {"output_text", "text"} and content.get("text"):
                return str(content["text"])

    raise RuntimeError("OpenAI Responses 未返回 output_text。")


def call_openai(settings: dict[str, Any], source_text: str) -> dict[str, Any]:
    api_key = str(settings.get("apiKey") or "").strip()
    if not api_key:
        raise RuntimeError("OpenAI Responses 需要 API Key。")

    explain_mode = str(settings.get("explainMode") or "phrase")
    language = subtitle_explain_language(settings)
    japanese_mode = should_apply_japanese_rules(language, source_text)
    payload = {
        "model": settings["model"],
        "input": [
            {
                "role": "system",
                "content": [
                    {
                        "type": "input_text",
                        "text": build_system_prompt(explain_mode, japanese_mode=japanese_mode),
                    }
                ],
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": build_user_prompt(
                            source_text,
                            explain_mode,
                            japanese_mode=japanese_mode,
                        ),
                    }
                ],
            },
        ],
        "text": {
            "format": {
                "type": "json_schema",
                "name": "subtitle_explanation",
                "strict": True,
                "schema": SCHEMA,
            }
        },
        "store": False,
    }
    response = post_json(
        normalize_base_url(str(settings["baseUrl"]), "/responses"),
        payload,
        {"Authorization": f"Bearer {api_key}"},
    )
    output_text = extract_openai_output_text(response)
    return parse_json_text(output_text, "OpenAI Responses 输出")


def call_ollama(settings: dict[str, Any], source_text: str) -> dict[str, Any]:
    headers: dict[str, str] = {}
    api_key = str(settings.get("apiKey") or "").strip()
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"

    explain_mode = str(settings.get("explainMode") or "phrase")
    language = subtitle_explain_language(settings)
    japanese_mode = should_apply_japanese_rules(language, source_text)
    payload = {
        "model": settings["model"],
        "messages": [
            {
                "role": "system",
                "content": build_system_prompt(explain_mode, japanese_mode=japanese_mode),
            },
            {
                "role": "user",
                "content": build_user_prompt(
                    source_text,
                    explain_mode,
                    japanese_mode=japanese_mode,
                ),
            },
        ],
        "stream": False,
        "think": False,
        "format": SCHEMA,
    }
    response = post_json(
        normalize_base_url(str(settings["baseUrl"]), "/api/chat"),
        payload,
        headers,
    )
    message = response.get("message", {})
    content = message.get("content")
    if not isinstance(content, str) or not content.strip():
        raise RuntimeError("Ollama 未返回可解析的 message.content。")
    return parse_json_text(content, "Ollama 输出")


def normalize_result(
    source_text: str,
    llm_output: dict[str, Any],
    *,
    language: str,
) -> dict[str, Any]:
    japanese_mode = should_apply_japanese_rules(language, source_text)
    phrases = []
    for item in llm_output.get("phrases") or []:
        if not isinstance(item, dict):
            continue
        source = str(
            item.get("source")
            or item.get("english")
            or item.get("term")
            or item.get("phrase")
            or ""
        ).strip()
        explanation = str(
            item.get("explanation")
            or item.get("chinese")
            or item.get("meaning")
            or item.get("translation")
            or ""
        ).strip()
        source = normalize_phrase_source(source, japanese_mode=japanese_mode)
        if source and explanation and not is_punctuation_only(source):
            phrases.append({"source": source, "explanation": explanation})

    return {
        "sourceText": source_text,
        "translation": str(llm_output.get("translation") or "").strip(),
        "phrases": phrases[:18],
    }


def ocr_subtitle(payload: dict[str, Any]) -> dict[str, Any]:
    image_path = Path(str(payload.get("imagePath") or ""))
    engine = str(payload.get("engine") or "paddleOcr")
    language = str(payload.get("language") or "auto")
    allow_empty = bool(payload.get("allowEmpty"))
    if not image_path.exists():
        raise RuntimeError(f"OCR 输入图片不存在：{image_path}")

    return {
        "sourceText": extract_ocr_text(
            str(image_path),
            engine=engine,
            language=language,
            allow_empty=allow_empty,
        ),
    }


def explain_subtitle(payload: dict[str, Any]) -> dict[str, Any]:
    source_text = str(payload.get("sourceText") or "").strip()
    settings = payload.get("settings") or {}
    if not source_text:
        raise RuntimeError("字幕文本不能为空。")

    provider = str(settings.get("provider") or "")
    if provider == "openAiResponses":
        llm_output = call_openai(settings, source_text)
    elif provider == "ollama":
        llm_output = call_ollama(settings, source_text)
    else:
        raise RuntimeError(f"不支持的 provider：{provider}")

    return normalize_result(
        source_text,
        llm_output,
        language=subtitle_explain_language(settings),
    )


def analyze_subtitle(payload: dict[str, Any]) -> dict[str, Any]:
    image_path = Path(str(payload.get("imagePath") or ""))
    settings = payload.get("settings") or {}
    ocr_settings = settings.get("ocrSettings") or {}
    engine = str(ocr_settings.get("engine") or "paddleOcr")
    language = str(ocr_settings.get("language") or "auto")
    if not image_path.exists():
        raise RuntimeError(f"OCR 输入图片不存在：{image_path}")

    source_text = extract_ocr_text(
        str(image_path),
        engine=engine,
        language=language,
        allow_empty=False,
    )
    return explain_subtitle({"sourceText": source_text, "settings": settings})


def handle_line(line: str) -> dict[str, Any]:
    request = json.loads(line)
    command = request.get("command")
    payload = request.get("payload") or {}

    if command == "ocr_subtitle":
        return success_response(ocr_subtitle(payload))
    if command == "explain_subtitle":
        return success_response(explain_subtitle(payload))
    if command == "analyze_subtitle":
        return success_response(analyze_subtitle(payload))
    return error_response(f"未知命令：{command}")


def main() -> int:
    configure_stdio()

    for raw_line in sys.stdin:
        line = raw_line.strip()
        if not line:
            continue

        try:
            emit(handle_line(line))
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            emit(error_response(f"HTTP {exc.code}: {detail}"))
        except Exception as exc:  # pragma: no cover - runtime behavior
            if os.environ.get("SUB_LENS_DEBUG_SIDECAR") == "1":
                traceback.print_exc(file=sys.stderr)
            else:
                print(f"sidecar error: {exc}", file=sys.stderr)
            emit(error_response(str(exc)))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
