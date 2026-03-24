# Sub Lens

基于 Tauri + Vanilla TS 的字幕遮挡 / 字幕解析工具。

## 当前能力

- 字幕遮挡主窗口
- OCR 区域独立编辑窗口
- 字幕解释独立展示窗口
- 右键菜单与全局快捷键
- Python sidecar 串联 OCR 与 LLM
- LLM 同时支持 OpenAI Responses 与 Ollama

## 本地开发

```bash
pnpm install
pnpm tauri dev
```

## Python sidecar 依赖

Python sidecar 位于 `src-tauri/sidecars/subtitle_sidecar.py`。

建议先准备 Python 3.10+ 环境，然后安装：

```bash
pip install -r src-tauri/sidecars/requirements.txt
```

此外还需要按平台安装 `paddlepaddle`，可参考 PaddleOCR 官方文档：

- https://www.paddleocr.ai/v3.0.2/en/quick_start.html

如果本机 Python 不是默认 `python` / `python3` / `py -3`，可通过环境变量指定：

```bash
SUB_LENS_PYTHON=/path/to/python
```
