import { apiJson, withAuthToken } from "../../services/api/base";

export type WebModFile = { path: string; sha256: string };

/** One installed mod as the server lists it. Mods are instance-wide. */
export type WebMod = {
  id: string;
  name: string;
  version: string;
  api: number;
  description: string;
  author: string;
  enabled: boolean;
  style: WebModFile | null;
  /** Listed only; scripts do not load until script approval exists. */
  script: WebModFile | null;
  error: string | null;
};

export type WebModsListing = { api: number; revision: string; mods: WebMod[] };

/** Marks the `<link>` a mod's style is loaded through. */
export const MOD_LINK_ATTR = "data-cccc-mod";

/** `?safe=1` loads no mod at all, so a broken mod can always be switched off. */
export function isModSafeMode(search: string): boolean {
  return new URLSearchParams(search).get("safe") === "1";
}

export function modFileUrl(id: string, file: WebModFile): string {
  const path = file.path.split("/").map(encodeURIComponent).join("/");
  // The content hash makes an edited file a new URL, so the browser never keeps a stale copy.
  return withAuthToken(
    `/api/v1/web_mods/files/${encodeURIComponent(id)}/${path}?h=${file.sha256.slice(0, 16)}`,
  );
}

/** Mods whose style should be on the page, in the server's order. */
export function activeStyleMods(listing: WebModsListing): WebMod[] {
  return listing.mods.filter((mod) => mod.enabled && !mod.error && mod.style);
}

/**
 * Makes the page carry exactly one stylesheet per active style mod, after the app's
 * own styles so a mod wins at equal specificity. Links of mods that went away or
 * were switched off are removed; an edited style gets its new URL in place.
 */
export function reconcileModStyles(
  doc: Document,
  listing: WebModsListing,
  urlFor: (id: string, file: WebModFile) => string = modFileUrl,
): void {
  const wanted = activeStyleMods(listing);
  const existing = new Map<string, HTMLLinkElement>();
  for (const link of doc.head.querySelectorAll<HTMLLinkElement>(`link[${MOD_LINK_ATTR}]`)) {
    existing.set(link.getAttribute(MOD_LINK_ATTR) || "", link);
  }
  for (const mod of wanted) {
    const href = urlFor(mod.id, mod.style as WebModFile);
    let link = existing.get(mod.id);
    existing.delete(mod.id);
    if (!link) {
      link = doc.createElement("link");
      link.rel = "stylesheet";
      link.setAttribute(MOD_LINK_ATTR, mod.id);
    }
    if (link.getAttribute("href") !== href) link.setAttribute("href", href);
    // Appending keeps the server's order and keeps mods after the app's styles.
    doc.head.appendChild(link);
  }
  for (const stale of existing.values()) stale.remove();
}

export async function fetchWebMods(): Promise<WebModsListing | null> {
  const response = await apiJson<WebModsListing>("/api/v1/web_mods");
  return response.ok ? response.result : null;
}
