import { describe, expect, it } from "vitest";

import {
  MOBILE_VIEWPORT_MAX_WIDTH_PX,
  MOBILE_VIEWPORT_MEDIA_QUERY,
} from "../../src/utils/responsiveLayout";

describe("responsiveLayout", () => {
  it("keeps JS mobile state aligned with Tailwind's md breakpoint", () => {
    expect(MOBILE_VIEWPORT_MAX_WIDTH_PX).toBe(767);
    expect(MOBILE_VIEWPORT_MEDIA_QUERY).toBe("(max-width: 767px)");
  });
});
