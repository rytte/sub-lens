import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "pages/overlay.html"),
        settings: resolve(__dirname, "pages/settings.html"),
        "ocr-region": resolve(__dirname, "pages/ocr-region.html"),
        "subtitle-explain": resolve(__dirname, "pages/subtitle-explain.html"),
      },
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  plugins: [react()],
}));
