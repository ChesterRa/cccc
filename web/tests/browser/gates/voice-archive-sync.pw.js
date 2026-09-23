import { test, expect } from "./helpers.js";

test("Voice document menu buttons support Enter and Space without selecting the row", async ({
  page,
}) => {
  await page.goto("/ui/tests/browser/voice-workspace-mobile.html?mode=document&lang=en");
  const heading = page.locator("[data-voice-document-title]");
  await expect(heading).toHaveText("语音会议工作稿");
  const trigger = page.getByRole("button", {
    name: "Actions for Linked activity document",
    exact: true,
  });
  const menu = page.getByRole("menu", { name: "Linked activity document", exact: true });
  for (const key of ["Enter", "Space"]) {
    await trigger.focus();
    await trigger.press(key);
    await expect(menu).toBeVisible();
    await expect(heading).toHaveText("语音会议工作稿");
    await expect(menu.getByRole("menuitem").first()).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
  }
  // The row's own keyboard selection still works.
  const row = page.locator('aside [role="button"]').filter({ hasText: "Linked activity document" });
  await row.focus();
  await row.press("Enter");
  await expect(heading).toHaveText("Linked activity document");
});

test("Voice workspace polling shows a document restored by another client", async ({ page }) => {
  // Real workspace, archive action and polling. Only the server transport is a fixture.
  await page.goto("/ui/tests/browser/voice-workspace-mobile.html?mode=document&lang=en");
  const row = page.locator('aside [role="button"]').filter({ hasText: "语音会议工作稿" });
  await expect(row).toBeVisible();
  await row.click({ button: "right" });
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("menuitem", { name: "Archive", exact: true }).click();
  await expect(row).toHaveCount(0);

  // Simulate another client restoring on the server, without calling this UI's onRestored.
  const restored = await page.evaluate(async () => {
    const archive = window.voiceWorkspaceProbe.writes.find((write) =>
      write.url.endsWith("/documents/archive"),
    );
    const response = await fetch(archive.url.replace(/archive$/, "library"), {
      method: "POST",
      body: JSON.stringify({ action: "restore", document_path: archive.body.document_path }),
    });
    const result = await response.json();
    return result.result.documents.find(
      (document) => document.document_path === archive.body.document_path,
    )?.status;
  });
  expect(restored).toBe("active");
  // Exercise the normal 30-second workspace poll, without a reload or group switch.
  await expect(row).toBeVisible({ timeout: 35_000 });
});
