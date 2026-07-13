import type { WebBranding } from "../types";

export const DEFAULT_PRODUCT_NAME = "CCCC";
export const DEFAULT_LOGO_ICON_URL = "/ui/logo.svg";
export const DEFAULT_DARK_LOGO_ICON_URL = "/ui/logo-dark.svg";
export const DEFAULT_FAVICON_URL = "/ui/logo.svg";
export const DEFAULT_DOCUMENT_TITLE = "CCCC - AI Agent Collaboration";

export const DEFAULT_WEB_BRANDING: WebBranding = {
  product_name: DEFAULT_PRODUCT_NAME,
  logo_icon_url: DEFAULT_LOGO_ICON_URL,
  favicon_url: DEFAULT_FAVICON_URL,
  has_custom_logo_icon: false,
  has_custom_favicon: false,
  updated_at: null,
};

export function normalizeWebBranding(value: Partial<WebBranding> | null | undefined): WebBranding {
  const productName = String(value?.product_name || "").trim() || DEFAULT_PRODUCT_NAME;
  const logoIconUrl = String(value?.logo_icon_url || "").trim() || DEFAULT_LOGO_ICON_URL;
  const faviconUrl = String(value?.favicon_url || "").trim() || logoIconUrl || DEFAULT_FAVICON_URL;
  return {
    product_name: productName,
    logo_icon_url: logoIconUrl,
    favicon_url: faviconUrl,
    has_custom_logo_icon: Boolean(value?.has_custom_logo_icon),
    has_custom_favicon: Boolean(value?.has_custom_favicon),
    updated_at: value?.updated_at ? String(value.updated_at) : null,
  };
}

export function resolveDocumentTitle(productName: string): string {
  const normalized = String(productName || "").trim() || DEFAULT_PRODUCT_NAME;
  return normalized === DEFAULT_PRODUCT_NAME ? DEFAULT_DOCUMENT_TITLE : normalized;
}

function normalizeAssetUrl(url: string | null | undefined): string {
  return String(url || "").trim();
}

function isDefaultLogoAsset(url: string | null | undefined): boolean {
  const normalized = normalizeAssetUrl(url);
  return normalized === DEFAULT_LOGO_ICON_URL || normalized === DEFAULT_DARK_LOGO_ICON_URL;
}

export function resolveThemeAwareLogoUrl(url: string | null | undefined, isDark: boolean): string {
  const normalized = normalizeAssetUrl(url) || DEFAULT_LOGO_ICON_URL;
  if (!isDefaultLogoAsset(normalized)) return normalized;
  return isDark ? DEFAULT_DARK_LOGO_ICON_URL : DEFAULT_LOGO_ICON_URL;
}

export function resolveThemeAwareFaviconUrl(
  url: string | null | undefined,
  isDark: boolean,
): string {
  const normalized = normalizeAssetUrl(url) || DEFAULT_FAVICON_URL;
  if (!isDefaultLogoAsset(normalized) && normalized !== DEFAULT_FAVICON_URL) return normalized;
  return isDark ? DEFAULT_DARK_LOGO_ICON_URL : DEFAULT_FAVICON_URL;
}

function ensureLink(rel: string): HTMLLinkElement {
  let el = document.querySelector(`link[rel="${rel}"]`) as HTMLLinkElement | null;
  if (!el) {
    el = document.createElement("link");
    el.rel = rel;
    document.head.appendChild(el);
  }
  return el;
}

export function applyBrandingToDocument(
  value: Partial<WebBranding> | null | undefined,
): WebBranding {
  const branding = normalizeWebBranding(value);
  document.title = resolveDocumentTitle(branding.product_name);
  const isDark = Boolean(document.documentElement?.classList?.contains("dark"));
  const iconHref = resolveThemeAwareFaviconUrl(branding.favicon_url, isDark);
  ensureLink("icon").href = iconHref;
  ensureLink("apple-touch-icon").href = iconHref;
  return branding;
}

export function syncDocumentBrandingTheme(): void {
  const icon = document.querySelector('link[rel="icon"]') as HTMLLinkElement | null;
  const apple = document.querySelector('link[rel="apple-touch-icon"]') as HTMLLinkElement | null;
  const isDark = Boolean(document.documentElement?.classList?.contains("dark"));
  if (icon) {
    icon.href = resolveThemeAwareFaviconUrl(icon.getAttribute("href"), isDark);
  }
  if (apple) {
    apple.href = resolveThemeAwareFaviconUrl(apple.getAttribute("href"), isDark);
  }
}
