// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { ImagePreview } from "./ImagePreview";
import { ImagePreviewFailure } from "./ImagePreviewFailure";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
const resource = {
  panelGroupId: "g1",
  blobGroupId: "g1",
  messageKey: "event-1",
  eventId: "event-1",
  index: 0,
  blobPath: "state/blobs/image",
};

describe("ImagePreview fixed layout", () => {
  it.each([
    ["hero", 224],
    ["grid", 128],
  ] as const)("keeps the %s preview at %dpx before image decode", (layout, height) => {
    const markup = renderToStaticMarkup(
      <ImagePreview
        resource={resource}
        href="/api/v1/groups/g1/blobs/image"
        downloadHref="/api/v1/groups/g1/blobs/image?download=true"
        downloadName="image.png"
        alt="attachment"
        isSvg={false}
        isUserMessage={false}
        isDark={false}
        layout={layout}
      />,
    );

    expect(markup).toContain(`height:${height}px`);
    expect(markup).toContain("object-contain");
    expect(markup).not.toContain("object-cover");
  });

  it("keeps the grid failure state inside the fixed preview height", () => {
    const markup = renderToStaticMarkup(
      <ImagePreviewFailure
        href="/api/v1/groups/g1/blobs/missing-image"
        downloadName="missing-image.png"
        alt="a-very-long-image-name-that-must-not-overlap.png"
        isUserMessage={false}
        isDark={false}
        layout="grid"
        height={128}
        title="Download image"
        unavailableLabel="Image preview unavailable"
        openOriginalLabel="Open the original image"
      />,
    );

    expect(markup).toContain("height:128px");
    expect(markup).toContain('download="missing-image.png"');
  });

  it("draws the transparency grid in panel tones and shows the picture without a lift", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const render = () =>
      act(async () =>
        root.render(
          <ImagePreview
            resource={resource}
            href="/api/v1/groups/g1/blobs/image"
            downloadHref="/api/v1/groups/g1/blobs/image?download=true"
            downloadName="image.png"
            alt="attachment"
            isSvg={false}
            isUserMessage={false}
            isDark={false}
          />,
        ),
      );
    try {
      await render();
      const image = host.querySelector("img");
      expect(image?.style.backgroundColor).toBe("var(--lcd-panel)");
      expect(image?.style.boxShadow).toBe("");
      expect(image?.style.filter).toBe("");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
