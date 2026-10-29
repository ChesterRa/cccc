import { expect, it } from "vite-plus/test";
import { formatElapsed } from "./time";

it("names elapsed seconds in their largest whole unit and the page's language", () => {
  expect(formatElapsed(59, "en")).toBe("59 seconds ago");
  expect(formatElapsed(60, "en")).toBe("1 minute ago");
  expect(formatElapsed(3599, "en")).toBe("59 minutes ago");
  expect(formatElapsed(3600, "en")).toBe("1 hour ago");
  expect(formatElapsed(86399, "en")).toBe("23 hours ago");
  expect(formatElapsed(86400 * 3, "en-US")).toBe("3 days ago");
  expect(formatElapsed(7200, "zh-CN")).toBe("2小时前");
  expect(formatElapsed(7200, "ja")).toBe("2 時間前");
  expect(formatElapsed(7200, undefined)).toBe("2 hours ago");
});
