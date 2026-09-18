import { chromium, expect } from "@playwright/test";
import fs from "node:fs";
const fixture = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const browser = await chromium.launch({
  headless: true,
  ...(process.env.CHROME_BIN ? { executablePath: process.env.CHROME_BIN } : {}),
});
const evidence = new URL("../../test-results/actor-config/", import.meta.url);
fs.mkdirSync(evidence, { recursive: true });
const context = await browser.newContext({ viewport: { width: 1400, height: 1000 } });
await context.tracing.start({ screenshots: true, snapshots: true });
const page = await context.newPage();
let passed = false;
try {
  const api = async (path) => (await page.request.get(`${fixture.base}/api/v1/${path}`)).json();
  const actor = async (id) =>
    (await api(`groups/${fixture.gid}/actors`)).result.actors.find((a) => a.id === id);
  const dialog = page.getByRole("dialog", { name: /Edit Agent:/ });
  const openEditor = async () =>
    page.getByRole("button", { name: "Edit agent configuration", exact: true }).click();
  const save = async () => {
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await expect(dialog).toBeHidden();
  };
  const closeActor = async () =>
    page.getByRole("button", { name: "Close expanded Actor view", exact: true }).click();
  await page.goto(`${fixture.base}/ui/`);
  await page.getByRole("button", { name: "Open terminal for Linked actor", exact: true }).click();
  await openEditor();
  await dialog.getByPlaceholder("linked", { exact: true }).fill("Linked renamed");
  await save();
  expect((await actor("linked")).profile_id).toBe("source");
  expect((await actor("linked")).title).toBe("Linked renamed");
  await openEditor();
  await dialog.getByRole("combobox", { name: "Runtime Profile", exact: true }).click();
  await page.getByRole("option", { name: /Target profile/ }).click();
  await save();
  expect((await actor("linked")).profile_id).toBe("target");
  await openEditor();
  await dialog.getByRole("button", { name: "Custom", exact: true }).click();
  await dialog.getByRole("button", { name: "Convert to Custom", exact: true }).click();
  await dialog.getByRole("combobox", { name: "Runtime", exact: true }).click();
  await page.getByRole("option", { name: "Custom", exact: true }).click();
  await dialog.locator("input.font-mono").fill("sleep 60");
  await dialog.getByRole("button", { name: "Remove PROFILE_KEY", exact: true }).click();
  let envFailures = 0;
  await page.route(`**/actors/linked/env_private`, async (route) => {
    if (route.request().method() === "POST" && envFailures++ === 0) {
      return route.fulfill({
        status: 503,
        json: {
          ok: false,
          error: { code: "fixture_failure", message: "Synthetic Actor secret failure" },
        },
      });
    }
    return route.continue();
  });
  await dialog.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(/Synthetic Actor secret failure/)).toBeVisible();
  expect((await actor("linked")).profile_id).toBe("");
  expect((await actor("linked")).runtime).toBe("custom");
  expect((await actor("linked")).command).toEqual(["sleep", "60"]);
  // Let a normal poll observe the intermediate conversion. It must not erase
  // the draft, restore the Profile mode, or clear the pending secret removal.
  await expect(dialog.getByRole("button", { name: "Save", exact: true })).toBeEnabled();
  await page.waitForTimeout(1200);
  await save();
  expect((await api(`groups/${fixture.gid}/actors/linked/env_private`)).result.keys).toEqual([]);
  expect(envFailures).toBe(2);
  await closeActor();

  await page.getByRole("button", { name: "Open terminal for Custom actor", exact: true }).click();
  await openEditor();
  await dialog.getByPlaceholder("custom-one", { exact: true }).fill("Renamed only");
  await save();
  expect((await actor("custom-one")).command).toEqual(fixture.originalCommand);
  await openEditor();
  await dialog.getByRole("button", { name: "Remove AUDIT_OLD", exact: true }).click();
  await dialog.getByRole("tab", { name: "Runtime profile tools", exact: true }).click();
  let prompts = 0;
  page.on("dialog", async (d) => {
    if (d.type() === "prompt") {
      prompts++;
      await d.accept("Saved with removal");
    } else await d.dismiss();
  });
  let profileSecretFailures = 0;
  await page.route("**/api/v1/profiles/*/env_private", async (route) => {
    if (route.request().method() === "POST" && profileSecretFailures++ === 0) {
      return route.fulfill({
        status: 503,
        json: {
          ok: false,
          error: { code: "fixture_failure", message: "Synthetic Profile secret failure" },
        },
      });
    }
    return route.continue();
  });
  await dialog.getByRole("button", { name: "Add to Runtime Profiles", exact: true }).click();
  await expect(page.getByText(/Synthetic Profile secret failure/)).toBeVisible();
  await dialog.getByRole("button", { name: "Add to Runtime Profiles", exact: true }).click();
  await expect.poll(() => profileSecretFailures).toBe(2);
  const profiles = (await api("profiles?view=accessible")).result.profiles.filter(
    (p) => p.name === "Saved with removal",
  );
  expect(profiles).toHaveLength(1);
  expect(prompts).toBe(1);
  expect(profiles[0].command).toEqual(fixture.originalCommand);
  await expect
    .poll(
      async () => (await api(`profiles/${profiles[0].id}/env_private?scope=global`)).result.keys,
    )
    .toEqual([]);
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await closeActor();
  // Closing an inbox does not cancel its HTTP response. A later opening owns
  // the content even when the same Actor is opened again.
  await page.getByRole("button", { name: "Open terminal for Linked renamed", exact: true }).click();
  let releaseOldInbox;
  let inboxRequests = 0;
  await page.route("**/inbox/linked?*", async (route) => {
    const index = ++inboxRequests;
    if (index === 1)
      await new Promise((resolve) => {
        releaseOldInbox = resolve;
      });
    await route.fulfill({
      json: {
        ok: true,
        result: {
          messages: [
            {
              id: `inbox-${index}`,
              kind: "chat.message",
              by: "user",
              ts: "2026-09-18T00:00:00Z",
              data: { text: index === 1 ? "Previous inbox snapshot" : "Current inbox snapshot" },
            },
          ],
        },
      },
    });
  });
  await page.getByRole("button", { name: /^Open inbox/ }).click();
  const inbox = page.getByRole("dialog", { name: /^Mail · linked/ });
  await expect(inbox).toBeVisible();
  await expect.poll(() => !!releaseOldInbox).toBe(true);
  await inbox.getByRole("button", { name: "Close", exact: true }).click();
  await page.getByRole("button", { name: /^Open inbox/ }).click();
  await expect(inbox).toContainText("Current inbox snapshot");
  const oldResponse = page.waitForResponse((response) => response.url().includes("/inbox/linked?"));
  releaseOldInbox();
  await oldResponse;
  // Observe beyond delivery of the response and React's update.
  await page.waitForTimeout(150);
  await expect(inbox).toContainText("Current inbox snapshot");
  await expect(inbox).not.toContainText("Previous inbox snapshot");
  await inbox.getByRole("button", { name: "Close", exact: true }).click();
  await page.unroute("**/inbox/linked?*");

  let releaseDelete;
  await page.route("**/actors/linked?*", async (route) => {
    if (route.request().method() === "DELETE") {
      await new Promise((resolve) => {
        releaseDelete = resolve;
      });
    }
    await route.continue();
  });
  page.removeAllListeners("dialog");
  page.on("dialog", (d) => (d.type() === "confirm" ? d.accept() : d.dismiss()));
  await page.getByRole("button", { name: "Remove agent", exact: true }).click();
  await expect.poll(() => !!releaseDelete).toBe(true);
  await closeActor();
  await page.getByRole("button", { name: "Open terminal for Renamed only", exact: true }).click();
  const deleted = page.waitForResponse(
    (response) =>
      response.request().method() === "DELETE" &&
      new URL(response.url()).pathname.endsWith("/actors/linked"),
  );
  releaseDelete();
  await deleted;
  await page.evaluate(
    () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))),
  );
  await expect.poll(() => actor("linked")).toBeUndefined();
  await expect(
    page.getByRole("button", { name: "Close expanded Actor view", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Edit agent configuration", exact: true }),
  ).toBeVisible();
  await closeActor();
  await page.getByRole("button", { name: "Settings and more", exact: true }).click();
  await page.getByRole("button", { name: "This instance", exact: false }).click();
  await page.getByRole("button", { name: "Actor Profiles", exact: true }).click();
  const card = page.getByText("Source profile", { exact: true }).locator("xpath=../../..");
  await card.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page
    .getByRole("dialog")
    .filter({ has: page.getByText("Edit Actor Profile", { exact: true }) })
    .last();
  await editor.locator("textarea").first().fill("AUDIT_NEW=synthetic");
  let failures = 0;
  await page.route("**/api/v1/profiles/source/env_private", async (route) => {
    if (route.request().method() === "POST" && failures++ === 0) {
      return route.fulfill({
        status: 503,
        json: {
          ok: false,
          error: { code: "fixture_failure", message: "Synthetic settings secret failure" },
        },
      });
    }
    return route.continue();
  });
  await editor.getByRole("button", { name: "Save", exact: true }).click();
  await expect(editor.getByRole("alert")).toContainText("Synthetic settings secret failure");
  expect((await api("profiles/source")).result.profile.revision).toBe(2);
  await editor.getByRole("button", { name: "Save", exact: true }).click();
  await expect(editor).toBeHidden();
  expect(failures).toBe(2);
  expect((await api("profiles/source/env_private?scope=global")).result.keys).toEqual([
    "AUDIT_NEW",
  ]);
  passed = true;
  process.stdout.write(
    "Actor configuration real-port workflows passed: linked rename/switch/convert, draft retention, exact argv, effective secrets, partial-save retry without duplicates or stale revisions, inbox response ownership and delayed deletion preserving navigation.\n",
  );
} finally {
  if (!passed)
    await page
      .screenshot({ path: new URL("failure.png", evidence).pathname, fullPage: true })
      .catch(() => {});
  await context.tracing.stop(passed ? {} : { path: new URL("trace.zip", evidence).pathname });
  await browser.close();
}
