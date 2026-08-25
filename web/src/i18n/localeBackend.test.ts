import { describe, expect, it, vi } from "vite-plus/test";
import { createLocaleBackend, localeModuleKey } from "./localeBackend";

describe("localeBackend", () => {
  it("normalizes regional language codes to a lazy module key", () => {
    expect(localeModuleKey("zh-CN", "chat")).toBe("./locales/zh/chat.json");
    expect(localeModuleKey("ja-JP", "settings")).toBe("./locales/ja/settings.json");
  });

  it("loads only the requested language namespace", async () => {
    const load = vi.fn().mockResolvedValue({ default: { title: "设置" } });
    const backend = createLocaleBackend({ "./locales/zh/settings.json": load });
    const result = await new Promise<unknown>((resolve, reject) => {
      backend.read("zh-CN", "settings", (error, data) => {
        if (error) reject(error);
        else resolve(data);
      });
    });

    expect(load).toHaveBeenCalledTimes(1);
    expect(result).toEqual({ title: "设置" });
  });
});
