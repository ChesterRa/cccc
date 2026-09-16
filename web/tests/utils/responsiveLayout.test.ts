import { describe, expect, it } from "vite-plus/test";

import {
  MOBILE_APP_HEADER_HEIGHT_PX,
  MOBILE_VIEWPORT_MAX_WIDTH_PX,
  MOBILE_VIEWPORT_MEDIA_QUERY,
} from "../../src/utils/responsiveLayout";

describe("responsiveLayout", () => {
  it("keeps JS mobile state aligned with Tailwind's md breakpoint", () => {
    expect(MOBILE_VIEWPORT_MAX_WIDTH_PX).toBe(767);
    expect(MOBILE_VIEWPORT_MEDIA_QUERY).toBe("(max-width: 767px)");
  });

  it("reserves the absolute mobile app header once", () => {
    expect(MOBILE_APP_HEADER_HEIGHT_PX).toBe(56);
  });
});
