import React from "react";
import ReactDOM from "react-dom/client";
import { i18nReady } from "./i18n";
import "./index.css";
import { useBrandingStore } from "./stores";
import { applyBrandingToDocument, DEFAULT_WEB_BRANDING } from "./utils/branding";
import { applyTextScale, getStoredTextScale } from "./utils/textScale";
import { applyUiStylePrefs, getUiStylePrefs } from "./utils/uiStyle";
import { claimVitePreloadReload, recoverDynamicImportError } from "./utils/vitePreloadRecovery";
import { cleanupLegacyWorkers } from "./features/pushNotifications/legacyWorkers";
import { WebRoot } from "./WebRoot";
import { reloadPage } from "./features/connect/reloadPage";

function reloadAfterStaleModuleError(error: unknown): boolean {
  return recoverDynamicImportError(error, window.sessionStorage, reloadPage);
}

window.addEventListener("vite:preloadError", (event) => {
  event.preventDefault();
  if (claimVitePreloadReload(window.sessionStorage)) {
    reloadPage();
  }
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

applyBrandingToDocument(DEFAULT_WEB_BRANDING);
applyTextScale(getStoredTextScale());
applyUiStylePrefs(getUiStylePrefs());
void useBrandingStore.getState().refreshBranding();

function renderApp() {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <WebRoot pathname={window.location.pathname} />,
  );
}

void i18nReady
  .catch((error: unknown) => {
    console.error("Failed to load initial locale resources:", error);
  })
  .then(renderApp);
