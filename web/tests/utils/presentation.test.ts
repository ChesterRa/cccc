import { describe, expect, it } from "vite-plus/test";
import {
  isValidPresentationWebUrl,
  normalizePresentationUrlInput,
  shouldPreferPresentationLiveBrowser,
} from "../../src/utils/presentation";

describe("presentation utils", () => {
  it("prefers live browser for loopback and private hosts", () => {
    expect(shouldPreferPresentationLiveBrowser("http://127.0.0.1:3000")).toBe(true);
    expect(shouldPreferPresentationLiveBrowser("http://localhost:5173")).toBe(true);
    expect(shouldPreferPresentationLiveBrowser("https://192.168.1.20/dashboard")).toBe(true);
    expect(shouldPreferPresentationLiveBrowser("http://172.20.0.4")).toBe(true);
    expect(shouldPreferPresentationLiveBrowser("http://host.docker.internal:8080")).toBe(true);
    expect(shouldPreferPresentationLiveBrowser("https://demo.local")).toBe(true);
  });

  it("keeps preview as default for normal public urls", () => {
    expect(shouldPreferPresentationLiveBrowser("https://example.com")).toBe(false);
    expect(shouldPreferPresentationLiveBrowser("example.com")).toBe(false);
    expect(shouldPreferPresentationLiveBrowser("https://app.github.com/demo")).toBe(false);
    expect(shouldPreferPresentationLiveBrowser("")).toBe(false);
    expect(shouldPreferPresentationLiveBrowser("not-a-url")).toBe(false);
    expect(shouldPreferPresentationLiveBrowser("file:///tmp/demo.html")).toBe(false);
  });

  it("normalizes url-like input conservatively before publish", () => {
    expect(normalizePresentationUrlInput("example.com")).toBe("https://example.com");
    expect(normalizePresentationUrlInput("www.example.com/report")).toBe(
      "https://www.example.com/report",
    );
    expect(normalizePresentationUrlInput("localhost:3000")).toBe("http://localhost:3000");
    expect(normalizePresentationUrlInput("127.0.0.1:8848/ui")).toBe("http://127.0.0.1:8848/ui");
    expect(normalizePresentationUrlInput("192.168.1.10:5173")).toBe("http://192.168.1.10:5173");
    expect(normalizePresentationUrlInput("https://example.com/demo")).toBe(
      "https://example.com/demo",
    );
  });

  it("does not over-guess inputs that do not look like urls", () => {
    expect(normalizePresentationUrlInput("foo")).toBe("foo");
    expect(normalizePresentationUrlInput("foo bar")).toBe("foo bar");
    expect(normalizePresentationUrlInput("/tmp/report.html")).toBe("/tmp/report.html");
    expect(isValidPresentationWebUrl("https://example.com")).toBe(true);
    expect(isValidPresentationWebUrl("example.com")).toBe(false);
    expect(isValidPresentationWebUrl("foo bar")).toBe(false);
  });
});
