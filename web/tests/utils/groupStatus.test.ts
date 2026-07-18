import { describe, expect, it } from "vite-plus/test";

import { getGroupStatusUnified } from "../../src/utils/groupStatus";
import {
  QUIET_RUN_INDICATOR_DOT_CLASS,
  STOPPED_INDICATOR_DOT_CLASS,
} from "../../src/utils/statusIndicators";

describe("getGroupStatusUnified", () => {
  it("maps quiet running to the shared hollow-ring indicator", () => {
    expect(getGroupStatusUnified(true, "active").dotClass).toBe(QUIET_RUN_INDICATOR_DOT_CLASS);
  });

  it("keeps stop state on the muted stopped indicator", () => {
    expect(getGroupStatusUnified(false, "active").dotClass).toBe(STOPPED_INDICATOR_DOT_CLASS);
  });
});
