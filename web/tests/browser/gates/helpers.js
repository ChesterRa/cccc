import { test as base, expect } from "@playwright/test";

export const test = base.extend({
  page: async ({ page, baseURL }, runTest) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.context().route("**/*", (route) => {
      const url = new URL(route.request().url());
      if (url.origin === new URL(baseURL).origin || !["http:", "https:"].includes(url.protocol)) {
        return route.continue();
      }
      errors.push(`Unexpected external request: ${url.origin}${url.pathname}`);
      return route.abort();
    });
    page.on("response", (response) => {
      if (response.status() === 503 && new URL(response.url()).pathname.startsWith("/api/")) {
        errors.push(`Unmocked API: ${new URL(response.url()).pathname}`);
      }
    });
    await runTest(page);
    expect(errors).toEqual([]);
  },
});
export { expect };

export async function groupPage(page) {
  await page.goto("/ui/tests/browser/group-work.html");
  await expect(page.locator("textarea")).toBeVisible();
  await expect.poll(() => page.evaluate("!!window.groupWorkProbe")).toBe(true);
}

// Unlike locator.click/scrollIntoView, wheel scrolling cannot make an
// overflow:hidden ancestor move. This detects the short-screen regression.
export async function wheelTo(page, locator) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const point = await locator.evaluate((element) => {
      const r = element.getBoundingClientRect();
      const x = r.x + r.width / 2;
      const y = r.y + r.height / 2;
      return element.contains(document.elementFromPoint(x, y)) ? { x, y } : null;
    });
    if (point) return point;
    const scroll = await locator.evaluate((element) => {
      const r = element.getBoundingClientRect();
      const middle = r.y + r.height / 2;
      const chain = [];
      for (let p = element.parentElement; p; p = p.parentElement) {
        if (
          /auto|scroll/.test(getComputedStyle(p).overflowY) &&
          p.scrollHeight > p.clientHeight + 1
        )
          chain.push(p);
      }
      for (const p of chain.reverse()) {
        const box = p.getBoundingClientRect();
        let top = Math.max(0, box.top),
          bottom = Math.min(innerHeight, box.bottom);
        for (let a = p.parentElement; a; a = a.parentElement) {
          if (/auto|scroll|hidden/.test(getComputedStyle(a).overflowY)) {
            const q = a.getBoundingClientRect();
            top = Math.max(top, q.top);
            bottom = Math.min(bottom, q.bottom);
          }
        }
        const delta = middle > (top + bottom) / 2 ? 100 : -100;
        if (
          bottom <= top ||
          (middle >= top && middle <= bottom) ||
          (delta > 0 && p.scrollTop + p.clientHeight >= p.scrollHeight - 1) ||
          (delta < 0 && p.scrollTop === 0)
        )
          continue;
        return { x: box.right - 3, y: (top + bottom) / 2, delta };
      }
      return null;
    });
    if (!scroll) break;
    await page.mouse.move(scroll.x, scroll.y);
    await page.mouse.wheel(0, scroll.delta);
    // Wheel delivery is asynchronous; wait for the browser's next painted frame.
    await page.evaluate(
      () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))),
    );
  }
  throw new Error("Control is not reachable by user scrolling");
}
