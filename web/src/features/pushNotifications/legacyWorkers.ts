import { isPushWorkerRegistration } from "./subscription";

/** Remove older UI workers without unregistering the dedicated push worker. */
export function cleanupLegacyWorkers(registrations: readonly ServiceWorkerRegistration[]): void {
  for (const registration of registrations) {
    try {
      const scope = new URL(registration.scope);
      if (scope.origin !== window.location.origin || !scope.pathname.startsWith("/ui/")) continue;
      if (isPushWorkerRegistration(registration)) {
        // No UI page is inside this scope, so page navigations cannot be
        // relied on to check for worker updates.
        void registration.update().catch(() => {});
      } else {
        void registration.unregister().catch(() => {});
      }
    } catch {
      // A stale registration must not prevent the workbench from starting.
    }
  }
}
