const PRELOAD_RELOAD_KEY = "cccc_vite_preload_reload_at";
const PRELOAD_RELOAD_COOLDOWN_MS = 15_000;
const DYNAMIC_IMPORT_ERROR_PATTERNS = [
  "failed to fetch dynamically imported module",
  "error loading dynamically imported module",
  "importing a module script failed",
];

export function isDynamicImportError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error || "");
  const normalized = message.trim().toLowerCase();
  return DYNAMIC_IMPORT_ERROR_PATTERNS.some((pattern) => normalized.includes(pattern));
}

export function claimVitePreloadReload(storage: Storage, now: number = Date.now()): boolean {
  try {
    const previous = Number(storage.getItem(PRELOAD_RELOAD_KEY));
    if (Number.isFinite(previous) && previous > 0 && now - previous < PRELOAD_RELOAD_COOLDOWN_MS) {
      return false;
    }
    storage.setItem(PRELOAD_RELOAD_KEY, String(now));
  } catch {
    return false;
  }
  return true;
}

export function recoverDynamicImportError(
  error: unknown,
  storage: Storage,
  reload: () => void,
  now: number = Date.now(),
): boolean {
  if (!isDynamicImportError(error)) return false;
  if (claimVitePreloadReload(storage, now)) reload();
  return true;
}
