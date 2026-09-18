import { test, expect, groupPage } from "./helpers.js";

// A bounded fixture measurement, not a real-provider/load benchmark. Run nightly
// or on demand; memory is recorded rather than judged against machine-specific limits.
test("retained terminals stay bounded across repeated Group and page navigation", async ({
  page,
}) => {
  await groupPage(page);
  const session = await page.context().newCDPSession(page);
  await session.send("Performance.enable");
  const measure = async () => {
    await session.send("HeapProfiler.collectGarbage");
    const { metrics } = await session.send("Performance.getMetrics");
    return Object.fromEntries(
      metrics
        .filter((m) => ["JSHeapUsedSize", "Nodes", "Documents"].includes(m.name))
        .map((m) => [m.name, m.value]),
    );
  };
  for (const group of ["g1", "g2"]) {
    await page.evaluate((id) => groupWorkProbe.chooseGroup(id), group);
    await page.getByRole("button", { name: "Terminals", exact: true }).click();
    await expect
      .poll(() => page.evaluate("groupWorkProbe.sockets.filter(s=>s.readyState===1).length"))
      .toBe(group === "g1" ? 4 : 12);
    await page.getByRole("button", { name: "Next page", exact: true }).click();
    await expect
      .poll(() => page.evaluate("groupWorkProbe.sockets.filter(s=>s.readyState===1).length"))
      .toBe(group === "g1" ? 8 : 16);
  }
  const before = await measure();
  for (let i = 0; i < 20; i++) {
    const group = i % 2 ? "g2" : "g1";
    await page.evaluate((id) => groupWorkProbe.chooseGroup(id), group);
    await expect(
      page.locator(`[data-runtime-group-id="${group}"]:not([inert])`).first(),
    ).toBeVisible();
    await page.getByRole("button", { name: "Previous page", exact: true }).click();
    await page.getByRole("button", { name: "Next page", exact: true }).click();
  }
  expect(await page.evaluate("groupWorkProbe.sockets.length")).toBe(16);
  expect(await page.evaluate("groupWorkProbe.terminals.length")).toBe(16);
  const after = await measure();
  await test
    .info()
    .attach("retained-terminal-measurement", {
      body: JSON.stringify({ groups: 2, terminals: 16, switches: 20, before, after }, null, 2),
      contentType: "application/json",
    });
});
