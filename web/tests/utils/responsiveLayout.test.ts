import { describe, expect, it } from "vitest";

import {
  getMobileMessageTopInsetPx,
  MOBILE_APP_HEADER_HEIGHT_PX,
  MOBILE_APP_HEADER_SAFE_TOP_INSET_PX,
  MOBILE_FLOATING_CONTROLS_MESSAGE_TOP_INSET_PX,
  MOBILE_FLOATING_CONTROLS_TOP_INSET_PX,
  MOBILE_VIEWPORT_MAX_WIDTH_PX,
  MOBILE_VIEWPORT_MEDIA_QUERY,
} from "../../src/utils/responsiveLayout";

describe("responsiveLayout", () => {
  it("keeps JS mobile state aligned with Tailwind's md breakpoint", () => {
    expect(MOBILE_VIEWPORT_MAX_WIDTH_PX).toBe(767);
    expect(MOBILE_VIEWPORT_MEDIA_QUERY).toBe("(max-width: 767px)");
  });

  it("keeps mobile chat content below the absolute app header", () => {
    expect(MOBILE_APP_HEADER_HEIGHT_PX).toBe(56);
    expect(MOBILE_APP_HEADER_SAFE_TOP_INSET_PX).toBe(64);
    expect(MOBILE_FLOATING_CONTROLS_TOP_INSET_PX).toBe(68);
    expect(MOBILE_FLOATING_CONTROLS_MESSAGE_TOP_INSET_PX).toBe(128);
    expect(getMobileMessageTopInsetPx(false)).toBe(64);
    expect(getMobileMessageTopInsetPx(true)).toBe(128);
  });
});
