import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  applyBrandingToDocument,
  DEFAULT_DOCUMENT_TITLE,
  DEFAULT_WEB_BRANDING,
  resolveDocumentTitle,
} from "../../src/utils/branding";

type FakeLink = { rel: string; href: string };

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
        return { rel: "", href: "" };
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
    });

    expect(branding.product_name).toBe("Acme Console");
    expect(document.title).toBe("Acme Console");
    expect((document.querySelector('link[rel="icon"]') as HTMLLinkElement | null)?.href).toContain(
      "/api/v1/branding/assets/favicon?v=test",
    );
    expect(
      (document.querySelector('link[rel="apple-touch-icon"]') as HTMLLinkElement | null)?.href,
    ).toContain("/api/v1/branding/assets/favicon?v=test");
  });
});
