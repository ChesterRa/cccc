import { test, expect, groupPage, wheelTo } from "./helpers.js";

for (const [width, height, scale] of [
  [1440, 1000, 100],
  [390, 667, 100],
  [320, 568, 125],
]) {
  test(`settings and Voice remain operable at ${width}x${height} / ${scale}%`, async ({
    page,
  }, info) => {
    const { locale, theme } = info.project.metadata;
    await page.setViewportSize({ width, height });
    await groupPage(page);
    await page.evaluate(
      async ({ locale, theme, scale }) => {
        await groupWorkProbe.language(locale);
        groupWorkProbe.setDark(theme === "dark");
        groupWorkProbe.setTextScale(scale);
        groupWorkProbe.openSettings("global", "developer");
      },
      { locale, theme, scale },
    );
    const dialog = page.locator('[role="dialog"][aria-modal="true"]');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("a".repeat(64), { exact: true }).first()).toBeVisible();
    expect(await dialog.evaluate((e) => e.scrollWidth <= e.clientWidth + 1)).toBe(true);
    if (locale === "ja" && theme === "dark" && width === 320) {
      await wheelTo(page, dialog.getByText("a".repeat(64), { exact: true }).first());
      await test
        .info()
        .attach("narrow-build-info", { body: await page.screenshot(), contentType: "image/png" });
    }
    await page.keyboard.press("Tab");
    expect(await page.evaluate("!!document.activeElement.closest('[role=dialog]')")).toBe(true);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await page.evaluate("groupWorkProbe.setConnectionStatus('disconnected')");
    const status = page.locator("header [role=status]:visible");
    await expect(status).toBeVisible();
    if (locale === "ja" && theme === "dark" && width === 320) {
      await test
        .info()
        .attach("narrow-connection-state", {
          body: await page.screenshot(),
          contentType: "image/png",
        });
    }
    expect(
      await status.evaluate((e) => {
        const r = e.getBoundingClientRect(),
          header = e.closest("header").getBoundingClientRect();
        return (
          r.top >= header.top && r.bottom <= header.bottom && e.scrollWidth <= e.clientWidth + 1
        );
      }),
    ).toBe(true);

    await page.goto(
      `/ui/tests/browser/voice-workspace-mobile.html?mode=prompt&theme=${theme}&lang=${locale}&scale=${scale}&extra=15`,
    );
    const toggle = page.locator(".voice-mobile-prompt-toggle");
    await expect(page.locator("[data-voice-mobile-sheet]")).toBeVisible();
    if (width < 1024) await toggle.click();
    const point = await wheelTo(page, page.locator("[data-voice-workspace-optimize]"));
    await page.mouse.click(point.x, point.y);
    await expect
      .poll(() =>
        page.evaluate("voiceWorkspaceProbe.writes.some(r=>r.body.kind==='prompt_refine')"),
      )
      .toBe(true);
    expect(
      await page
        .locator("[data-voice-mobile-sheet]")
        .evaluate((e) => e.scrollWidth <= e.clientWidth + 1),
    ).toBe(true);
  });
}
