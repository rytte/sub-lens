import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";

export function mountWindowApp(app: ReactNode) {
  const container = document.querySelector<HTMLDivElement>("#app");

  if (!container) {
    throw new Error("App root not found.");
  }

  createRoot(container).render(app);
}
