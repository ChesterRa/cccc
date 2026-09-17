// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { BrandingTab } from "./BrandingTab";
import { useBrandingStore } from "../../../stores/useBrandingStore";
import { DEFAULT_WEB_BRANDING } from "../../../utils/branding";
import * as api from "../../../services/api";
vi.mock("../../../services/api", () => ({
  updateWebBranding: vi.fn(),
  uploadWebBrandingAsset: vi.fn(),
  clearWebBrandingAsset: vi.fn(),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let root: ReturnType<typeof createRoot>, host: HTMLDivElement;
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  useBrandingStore.setState({ branding: DEFAULT_WEB_BRANDING });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<BrandingTab isDark={false} isActive={false} />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const name = () => host.querySelector<HTMLInputElement>("#branding-product-name")!;
async function changeName(value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(name(), value);
    name().dispatchEvent(new Event("input", { bubbles: true }));
  });
}
it("saves the product name explicitly and exposes success beside the form", async () => {
  await changeName("Studio");
  expect(api.updateWebBranding).not.toHaveBeenCalled();
  vi.mocked(api.updateWebBranding).mockResolvedValue({
    ok: true,
    result: { branding: { ...DEFAULT_WEB_BRANDING, product_name: "Studio" } },
  });
  await act(async () =>
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(api.updateWebBranding).toHaveBeenCalledWith({ productName: "Studio" });
  expect(host.querySelector('[role="status"]')?.textContent).toBe("branding.saved");
  expect(host.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
});
it("uploads each icon to its own endpoint without losing the unsaved name", async () => {
  await changeName("Unsaved name");
  const file = new File(["image"], "icon.png", { type: "image/png" });
  for (const [index, kind] of ["logo_icon", "favicon"].entries()) {
    vi.mocked(api.uploadWebBrandingAsset).mockResolvedValueOnce({
      ok: true,
      result: {
        branding: { ...DEFAULT_WEB_BRANDING, has_custom_logo_icon: true, has_custom_favicon: true },
      },
    });
    const input = host.querySelectorAll<HTMLInputElement>('input[type="file"]')[index];
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(api.uploadWebBrandingAsset).toHaveBeenLastCalledWith(kind, file);
    expect(name().value).toBe("Unsaved name");
  }
  vi.mocked(api.clearWebBrandingAsset).mockResolvedValueOnce({
    ok: true,
    result: { branding: DEFAULT_WEB_BRANDING },
  });
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "branding.followLogo")!
      .click(),
  );
  expect(api.clearWebBrandingAsset).toHaveBeenCalledWith("favicon");
  expect(name().value).toBe("Unsaved name");
});
