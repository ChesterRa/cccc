export interface RuntimeBuildInfo {
  webSource: string;
  daemonSource: string;
  webAssets: string;
  servedEntry: string;
  loadedEntry: string;
}

export function loadedWebEntry(): string {
  const src = document.querySelector<HTMLScriptElement>('script[type="module"][src]')?.src;
  return src ? new URL(src, location.href).pathname : "";
}

/** A missing identity or a dev-server entry cannot establish a difference. */
export function webEntryDifference(servedEntry: string, loadedEntry: string): boolean | null {
  if (!servedEntry.includes("/assets/") || !loadedEntry.includes("/assets/")) return null;
  return servedEntry !== loadedEntry;
}

export function buildMismatch(info?: RuntimeBuildInfo): boolean {
  if (!info) return false;
  const sourceDiffers =
    !!info.webSource && !!info.daemonSource && info.webSource !== info.daemonSource;
  // Dev-server entry paths intentionally differ from the packaged Web bundle.
  const assetsDiffer = webEntryDifference(info.servedEntry, info.loadedEntry) === true;
  return sourceDiffers || assetsDiffer;
}
