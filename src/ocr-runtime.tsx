import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { createWorker, OEM, PSM, setLogging } from "tesseract.js";
import workerUrl from "tesseract.js/dist/worker.min.js?url";

import { mountWindowApp } from "./main";
import type { OcrEngine, OcrLanguage } from "./shared/types";

const EVENT_OCR_RUNTIME_REQUEST = "ocr-runtime://request";

type OcrRuntimeRequest = {
  requestId: string;
  engine: OcrEngine;
  language: OcrLanguage;
  imageDataUrl: string;
  tesseract: {
    languages: string[];
    pageSegMode: number;
  };
};

type TesseractWorker = Awaited<ReturnType<typeof createWorker>>;

const PSM_MAP: Record<number, PSM> = {
  0: PSM.OSD_ONLY,
  1: PSM.AUTO_OSD,
  2: PSM.AUTO_ONLY,
  3: PSM.AUTO,
  4: PSM.SINGLE_COLUMN,
  5: PSM.SINGLE_BLOCK_VERT_TEXT,
  6: PSM.SINGLE_BLOCK,
  7: PSM.SINGLE_LINE,
  8: PSM.SINGLE_WORD,
  9: PSM.CIRCLE_WORD,
  10: PSM.SINGLE_CHAR,
  11: PSM.SPARSE_TEXT,
  12: PSM.SPARSE_TEXT_OSD,
  13: PSM.RAW_LINE,
};

const workerCache = new Map<string, Promise<TesseractWorker>>();
let requestChain = Promise.resolve();

setLogging(false);

function resolvePsm(pageSegMode: number) {
  return PSM_MAP[pageSegMode] ?? PSM.SINGLE_BLOCK;
}

function normalizeLanguages(languages: string[]) {
  return Array.from(
    new Set(
      languages
        .map((value) => value.trim())
        .filter(Boolean),
    ),
  );
}

async function getTesseractWorker(languages: string[]) {
  const normalizedLanguages = normalizeLanguages(languages);
  const cacheKey = normalizedLanguages.join("+");

  const existingWorker = workerCache.get(cacheKey);
  if (existingWorker) {
    return existingWorker;
  }

  const workerPromise = (async () => {
    return createWorker(normalizedLanguages, OEM.LSTM_ONLY, {
      workerPath: workerUrl,
      logger: () => undefined,
      errorHandler: (error) => {
        console.error("tesseract.js worker error:", error);
      },
    });
  })();

  workerCache.set(cacheKey, workerPromise);

  try {
    return await workerPromise;
  } catch (error) {
    workerCache.delete(cacheKey);
    throw error;
  }
}

async function recognizeWithTesseract(request: OcrRuntimeRequest) {
  const worker = await getTesseractWorker(request.tesseract.languages);
  await worker.setParameters({
    tessedit_pageseg_mode: resolvePsm(request.tesseract.pageSegMode),
  });

  const {
    data: { text },
  } = await worker.recognize(request.imageDataUrl);

  return text;
}

function describeError(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

async function processRequest(request: OcrRuntimeRequest) {
  try {
    if (request.engine !== "tesseract") {
      throw new Error(`Unsupported OCR engine in runtime: ${request.engine}`);
    }

    const text = await recognizeWithTesseract(request);
    await invoke("submit_ocr_runtime_result", {
      requestId: request.requestId,
      text,
    });
  } catch (error) {
    await invoke("submit_ocr_runtime_error", {
      requestId: request.requestId,
      error: describeError(error),
    });
  }
}

async function terminateAllWorkers() {
  const workers = await Promise.allSettled(workerCache.values());
  workerCache.clear();

  await Promise.allSettled(
    workers.flatMap((result) =>
      result.status === "fulfilled" ? [result.value.terminate()] : [],
    ),
  );
}

function OcrRuntimeWindow() {
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;

    const setup = async () => {
      unlistenFn = await listen<OcrRuntimeRequest>(
        EVENT_OCR_RUNTIME_REQUEST,
        ({ payload }) => {
          requestChain = requestChain
            .then(() => processRequest(payload))
            .catch((error) => {
              console.error("ocr runtime queue failed:", error);
            });
        },
      );

      await invoke("notify_ocr_runtime_ready");
    };

    void setup();

    return () => {
      unlistenFn?.();
      void terminateAllWorkers();
    };
  }, []);

  return null;
}

mountWindowApp(<OcrRuntimeWindow />);
