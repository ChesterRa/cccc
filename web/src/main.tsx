import React from "react";
import ReactDOM from "react-dom/client";
import { AuthGate } from "./components/AuthGate";
import App from "./App";
import { CapabilityCenterStandaloneApp } from "./components/capabilities/CapabilityCenterStandaloneApp";
import { isCapabilityCenterPath } from "./components/capabilities/capabilityCenterRoute";
import "./i18n";
import "./index.css";
import { useBrandingStore } from "./stores";
import { applyBrandingToDocument, DEFAULT_WEB_BRANDING } from "./utils/branding";
import { applyTextScale, getStoredTextScale } from "./utils/textScale";

// v0.4: We intentionally do NOT use Service Workers.
// Reason: SW caching frequently causes "stale UI" bugs in an ops/admin console.
//
// Best-effort cleanup: unregister any legacy SWs scoped to `/ui/` so clients
// recover automatically after upgrades.
if ("serviceWorker" in navigator && typeof navigator.serviceWorker.getRegistrations === "function") {
  void navigator.serviceWorker.getRegistrations().then((registrations) => {
    for (const r of registrations) {
      try {
        const scope = String(r.scope || "");
        // Only touch our own scope to avoid impacting unrelated SWs on the same origin.
        if (scope.includes("/ui/")) {
          void r.unregister();
        }
      } catch {
        // ignore
      }
    }
  });
}

applyBrandingToDocument(DEFAULT_WEB_BRANDING);
applyTextScale(getStoredTextScale());
void useBrandingStore.getState().refreshBranding();
const isCapabilityCenterPage = isCapabilityCenterPath(window.location.pathname);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <AuthGate>
    {isCapabilityCenterPage ? <CapabilityCenterStandaloneApp /> : <App />}
  </AuthGate>,
);
