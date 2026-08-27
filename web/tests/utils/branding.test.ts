import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  applyBrandingToDocument,
  APPLE_TOUCH_ICON_URL,
  DEFAULT_DOCUMENT_TITLE,
  DEFAULT_WEB_BRANDING,
  resolveDocumentTitle,
  resolveWebAppManifestUrl,
  syncDocumentBrandingTheme,
} from "../../src/utils/branding";

type FakeLink = { rel: string; href: string; getAttribute: (name: string) => string | null };

describe("branding utils", () => {
  const links = new Map<string, FakeLink>();

  beforeEach(() => {
    links.clear();
    const head = {
      appendChild(node: FakeLink) {
        links.set(String(node.rel || ""), node);
        return node;
      },
    };
    const documentStub = {
      head,
      title: "",
      querySelector(selector: string) {
        const match = selector.match(/^link\[rel="(.+)"\]$/);
        if (!match) return null;
        return links.get(match[1]) || null;
      },
      createElement(_tag: string) {
        const link: FakeLink = {
          rel: "",
          href: "",
          getAttribute(name: string) {
            return name === "href" ? link.href : null;
          },
        };
        return link;
      },
    };
    vi.stubGlobal("document", documentStub);
  });

  it("keeps the default descriptive title for the built-in product name", () => {
    expect(resolveDocumentTitle("CCCC")).toBe(DEFAULT_DOCUMENT_TITLE);
  });

  it("uses the custom product name as the document title", () => {
    expect(resolveDocumentTitle("Acme Console")).toBe("Acme Console");
  });

  it("updates document title and icon links", () => {
    const branding = applyBrandingToDocument({
      ...DEFAULT_WEB_BRANDING,
      product_name: "Acme Console",
      favicon_url: "/api/v1/branding/assets/favicon?v=test",
      updated_at: "2026-08-10T12:00:00Z",
    });

    expect(branding.product_name).toBe("Acme Console");
    expect(document.title).toBe("Acme Console");
    expect((document.querySelector('link[rel="icon"]') as HTMLLinkElement | null)?.href).toContain(
      "/api/v1/branding/assets/favicon?v=test",
    );
    expect(
      (document.querySelector('link[rel="apple-touch-icon"]') as HTMLLinkElement | null)?.href,
    ).toBe(APPLE_TOUCH_ICON_URL);
    expect(
      (document.querySelector('link[rel="manifest"]') as HTMLLinkElement | null)?.href,
    ).toContain("/ui/manifest.webmanifest?v=2026-08-10T12%3A00%3A00Z");
  });

  it("keeps a stable manifest URL until branding has a version", () => {
    expect(resolveWebAppManifestUrl(null)).toBe("/ui/manifest.webmanifest");
  });

  it("keeps the Apple icon endpoint stable when the color theme changes", () => {
    applyBrandingToDocument({
      ...DEFAULT_WEB_BRANDING,
      favicon_url: "/api/v1/branding/assets/favicon?v=test",
    });

    syncDocumentBrandingTheme();

    expect(
      (document.querySelector('link[rel="apple-touch-icon"]') as HTMLLinkElement | null)?.href,
    ).toBe(APPLE_TOUCH_ICON_URL);
  });
});
