import React from "react";
import ReactDOM from "react-dom/client";
import { AuthGate } from "./components/AuthGate";
import App from "./App";
import { CapabilityCenterStandaloneApp } from "./components/capabilities/CapabilityCenterStandaloneApp";
import { isCapabilityCenterPath } from "./components/capabilities/capabilityCenterRoute";
import { i18nReady } from "./i18n";
import "./index.css";
import { useBrandingStore } from "./stores";
import { applyBrandingToDocument, DEFAULT_WEB_BRANDING } from "./utils/branding";
import { applyTextScale, getStoredTextScale } from "./utils/textScale";
import { ConnectEmbeddedApp } from "./features/connect/ConnectEmbeddedApp";
import { isConnectFramePath } from "./features/connect/protocol";
import { claimVitePreloadReload, recoverDynamicImportError } from "./utils/vitePreloadRecovery";
import { cleanupLegacyWorkers } from "./features/pushNotifications/legacyWorkers";

function reloadAfterStaleModuleError(error: unknown): boolean {
  return recoverDynamicImportError(error, window.sessionStorage, () => window.location.reload());
}

window.addEventListener("vite:preloadError", (event) => {
  event.preventDefault();
  if (claimVitePreloadReload(window.sessionStorage)) {
    window.location.reload();
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
void useBrandingStore.getState().refreshBranding();
const isCapabilityCenterPage = isCapabilityCenterPath(window.location.pathname);

function renderApp() {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    isConnectFramePath(window.location.pathname) ? (
      <ConnectEmbeddedApp />
    ) : (
      <AuthGate>{isCapabilityCenterPage ? <CapabilityCenterStandaloneApp /> : <App />}</AuthGate>
    ),
  );
}

void i18nReady
  .catch((error: unknown) => {
    console.error("Failed to load initial locale resources:", error);
  })
  .then(renderApp);
