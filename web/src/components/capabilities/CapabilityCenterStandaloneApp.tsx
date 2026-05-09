import { lazy, Suspense } from "react";

import { useTextScale } from "../../hooks/useTextScale";
import { useTheme } from "../../hooks/useTheme";
import { useViewportHeight } from "../../hooks/useViewportHeight";
import { capabilityCenterGroupIdFromSearch } from "./capabilityCenterRoute";

const CapabilityCenterWorkspace = lazy(() =>
  import("./CapabilityCenterWorkspace").then((module) => ({ default: module.CapabilityCenterWorkspace }))
);

export function CapabilityCenterStandaloneApp() {
  const { isDark } = useTheme();
  useTextScale();
  useViewportHeight();
  const groupId = capabilityCenterGroupIdFromSearch(window.location.search);

  return (
    <Suspense fallback={<div className="min-h-dvh bg-[var(--color-bg-primary)] text-[var(--color-text-muted)]" />}>
      <CapabilityCenterWorkspace
        isOpen
        groupId={groupId}
        isDark={isDark}
        surface="page"
      />
    </Suspense>
  );
}
