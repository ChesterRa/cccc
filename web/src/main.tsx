import React from "react";
import ReactDOM from "react-dom/client";
import { i18nReady } from "./i18n";
import "./index.css";
import "./styles/preview.css";
import { useBrandingStore } from "./stores";
import { applyBrandingToDocument, DEFAULT_WEB_BRANDING } from "./utils/branding";
import { applyTextScale, getStoredTextScale } from "./utils/textScale";
import { applyUiStylePrefs, getUiStylePrefs } from "./utils/uiStyle";
import { recoverDynamicImportError } from "./utils/vitePreloadRecovery";
import { cleanupLegacyWorkers } from "./features/pushNotifications/legacyWorkers";
import { WebRoot } from "./WebRoot";
import { reloadForNewBuild } from "./features/connect/reloadPage";

const reloadForNewModules = () => reloadForNewBuild(window.sessionStorage);

function reloadAfterStaleModuleError(error: unknown): boolean {
  return recoverDynamicImportError(error, reloadForNewModules);
}

// Without a reload the import fails with its own error, which the part that loads it can show.
window.addEventListener("vite:preloadError", (event) => {
  if (reloadForNewModules()) event.preventDefault();
});

// Development imports are not wrapped by Vite's production preload helper.
window.addEventListener("error", (event) => {
  if (reloadAfterStaleModuleError(event.error)) {
    event.preventDefault();
  }
});

window.addEventListener("unhandledrejection", (event) => {
  if (reloadAfterStaleModuleError(event.reason)) {
    event.preventDefault();
  }
});

function startWorkbench() {
  // Keep removing old page-controlling workers: their caches can serve stale UI.
  // The narrowly scoped push worker handles no fetches and is retained.
  if (
    "serviceWorker" in navigator &&
    typeof navigator.serviceWorker.getRegistrations === "function"
  ) {
    void navigator.serviceWorker
      .getRegistrations()
      .then(cleanupLegacyWorkers)
      .catch(() => {});
  }
  void useBrandingStore.getState().refreshBranding();
}

applyBrandingToDocument(DEFAULT_WEB_BRANDING);
applyTextScale(getStoredTextScale());
applyUiStylePrefs(getUiStylePrefs());

function renderApp() {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <WebRoot pathname={window.location.pathname} onWorkbenchEntry={startWorkbench} />,
  );
}

void i18nReady
  .catch((error: unknown) => {
    console.error("Failed to load initial locale resources:", error);
  })
  .then(renderApp);
