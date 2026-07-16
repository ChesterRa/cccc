import { describe, expect, it } from "vite-plus/test";
import {
  appendQuotedOriginalPerspective,
  appendSenderPerspective,
  getMessageInsight,
} from "./messagePerspective";

describe("message perspective projection", () => {
  it("normalizes only a string insight", () => {
    expect(getMessageInsight({ insight: "  provisional view  " })).toBe("provisional view");
    expect(getMessageInsight({ insight: 42 })).toBe("");
    expect(getMessageInsight(null)).toBe("");
  });

  it("appends one clearly labeled plain-text section", () => {
    expect(appendSenderPerspective("Main body", "Provisional view", "Sender perspective")).toBe(
      "Main body\n\nSender perspective:\nProvisional view",
    );
    expect(appendSenderPerspective("Main body", "", "Sender perspective")).toBe("Main body");
  });

  it("quotes inherited perspective during a manual relay instead of creating a new claim", () => {
    expect(
      appendQuotedOriginalPerspective("Relay note\n\nMain body", "Original view", "peer1"),
    ).toBe(
      "Relay note\n\nMain body\n\nOriginal sender perspective (quoted from peer1):\nOriginal view",
    );
  });
});
