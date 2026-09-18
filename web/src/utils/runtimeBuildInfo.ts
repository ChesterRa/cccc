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

export function buildMismatch(info?: RuntimeBuildInfo): boolean {
  if (!info) return false;
  const sourceDiffers =
    !!info.webSource && !!info.daemonSource && info.webSource !== info.daemonSource;
  // Dev-server entry paths intentionally differ from the packaged Web bundle.
  const assetsDiffer =
    info.servedEntry.includes("/assets/") &&
    info.loadedEntry.includes("/assets/") &&
    info.servedEntry !== info.loadedEntry;
  return sourceDiffers || assetsDiffer;
}
