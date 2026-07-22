import { describe, expect, it } from "vitest";
import { mapContainedImagePoint } from "./projectedBrowserCoordinates";

describe("mapContainedImagePoint", () => {
  it("removes object-contain letterboxing before mapping browser coordinates", () => {
    expect(
      mapContainedImagePoint(
        { x: 500, y: 300 },
        { left: 0, top: 0, width: 1000, height: 600 },
        { width: 1000, height: 500 },
      ),
    ).toEqual({ x: 500, y: 250 });
  });

  it("ignores clicks in the letterbox instead of clicking the page edge", () => {
    expect(
      mapContainedImagePoint(
        { x: 500, y: 20 },
        { left: 0, top: 0, width: 1000, height: 600 },
        { width: 1000, height: 500 },
      ),
    ).toBeNull();
  });
});
