from __future__ import annotations

import json
import os
import tempfile
import sys
import time
import traceback
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "sourceText": {"type": "string"},
        "translation": {"type": "string"},
        "summary": {"type": "string"},
        "phrases": {
            "type": "array",
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
        "grammarPoints": {
            "type": "array",
            "items": {"type": "string"},
        },
        "provider": {"type": "string"},
        "model": {"type": "string"},
        "latencyMs": {"type": "number"},
        "error": {"type": ["string", "null"]},
    },
    "required": [
        "sourceText",
        "translation",
        "summary",
        "phrases",
        "grammarPoints",
        "provider",
        "model",
        "latencyMs",
        "error",
    ],
    "additionalProperties": False,
}

SYSTEM_PROMPT = """你是一个字幕学习助手。
你会收到一行 OCR 后的字幕文本，请输出严格符合 JSON Schema 的中文解释结果。
要求：
1. translation：给出自然、简洁的整句中文翻译。
2. summary：用 1~2 句解释这句字幕的语气、场景或隐含含义。
3. phrases：挑 2~5 个关键短语或搭配，逐个用中文解释。
4. grammarPoints：列 1~4 个值得注意的语法点或表达习惯。
5. 所有内容使用简体中文。
6. 不要输出任何 JSON 之外的文字。"""

ocr_instance = None
OCR_SCORE_THRESHOLD = float(os.environ.get("SUB_LENS_OCR_SCORE_THRESHOLD", "0.20"))


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


def parse_json_text(text: str, label: str) -> dict[str, Any]:
    candidate = extract_first_json_block(text)
    if not candidate.strip():
        raw_path = write_debug_file(label, text)
        raise RuntimeError(f"{label} 返回为空。原始内容已保存到：{raw_path}")

    try:
        parsed = json.loads(candidate)
    except json.JSONDecodeError as exc:
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

    with urllib.request.urlopen(request, timeout=120) as response:
        body = response.read().decode("utf-8", errors="replace")
        return parse_json_text(body, "接口响应")


def ensure_ocr():
    global ocr_instance
    if ocr_instance is not None:
        return ocr_instance

    try:
        from paddleocr import PaddleOCR
    except Exception as exc:  # pragma: no cover - runtime dependency
        raise RuntimeError(
            "无法导入 paddleocr。请先执行 `pip install -r src-tauri/sidecars/requirements.txt`，"
            "再按 PaddleOCR 官方文档安装 paddlepaddle。"
        ) from exc

    ocr_instance = PaddleOCR(
        use_doc_orientation_classify=False,
        use_doc_unwarping=False,
        use_textline_orientation=False,
        # PaddleOCR 3.x on Windows CPU can fail inside oneDNN during predict().
        enable_mkldnn=False,
    )
    return ocr_instance


def extract_ocr_text(image_path: str) -> str:
    ocr = ensure_ocr()
    result = ocr.predict(image_path)
    if not result:
        raise RuntimeError(f"OCR 未返回任何结果。请检查 OCR 区域是否框准字幕。调试截图：{image_path}")

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

    collected: list[str] = []
    candidates: list[str] = []
    for index, raw_text in enumerate(texts):
        text = str(raw_text or "").strip()
        if not text:
            continue

        score = None
        if index < len(scores):
            try:
                score = float(scores[index])
            except Exception:
                score = None

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
        preview = "；".join(candidates[:8])
        raise RuntimeError(
            f"OCR 未识别到可用字幕文本。当前候选均低于阈值 {OCR_SCORE_THRESHOLD:.2f}。"
            f"候选：{preview}。调试截图：{image_path}"
        )

    raise RuntimeError(
        f"OCR 未识别到可用字幕文本。未检测到任何文字框。请检查 OCR 区域是否框准字幕。调试截图：{image_path}"
    )


def build_user_prompt(source_text: str) -> str:
    return (
        "请解释下面这句字幕，并严格按约定 JSON Schema 输出。\n\n"
        f"字幕：{source_text}"
    )


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

    payload = {
        "model": settings["model"],
        "input": [
            {
                "role": "system",
                "content": [{"type": "input_text", "text": SYSTEM_PROMPT}],
            },
            {
                "role": "user",
                "content": [{"type": "input_text", "text": build_user_prompt(source_text)}],
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

    prompt_with_schema = (
        f"{build_user_prompt(source_text)}\n\n"
        f"输出 JSON Schema：{json.dumps(SCHEMA, ensure_ascii=False)}"
    )
    payload = {
        "model": settings["model"],
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt_with_schema},
        ],
        "stream": False,
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
    settings: dict[str, Any],
    source_text: str,
    latency_ms: int,
    llm_output: dict[str, Any],
) -> dict[str, Any]:
    phrases = []
    for item in llm_output.get("phrases") or []:
        if not isinstance(item, dict):
            continue
        source = str(item.get("source") or "").strip()
        explanation = str(item.get("explanation") or "").strip()
        if source and explanation:
            phrases.append({"source": source, "explanation": explanation})

    grammar_points = [
        str(item).strip()
        for item in (llm_output.get("grammarPoints") or [])
        if str(item).strip()
    ]

    return {
        "sourceText": source_text,
        "translation": str(llm_output.get("translation") or "").strip(),
        "summary": str(llm_output.get("summary") or "").strip(),
        "phrases": phrases[:5],
        "grammarPoints": grammar_points[:4],
        "provider": settings["provider"],
        "model": settings["model"],
        "latencyMs": latency_ms,
        "error": None,
    }


def analyze_subtitle(payload: dict[str, Any]) -> dict[str, Any]:
    image_path = Path(str(payload.get("imagePath") or ""))
    settings = payload.get("settings") or {}
    if not image_path.exists():
        raise RuntimeError(f"OCR 输入图片不存在：{image_path}")

    source_text = extract_ocr_text(str(image_path))

    started = time.perf_counter()
    provider = str(settings.get("provider") or "")
    if provider == "openAiResponses":
        llm_output = call_openai(settings, source_text)
    elif provider == "ollama":
        llm_output = call_ollama(settings, source_text)
    else:
        raise RuntimeError(f"不支持的 provider：{provider}")

    latency_ms = int((time.perf_counter() - started) * 1000)
    return normalize_result(settings, source_text, latency_ms, llm_output)


def handle_line(line: str) -> dict[str, Any]:
    request = json.loads(line)
    command = request.get("command")
    payload = request.get("payload") or {}

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
