import { test, expect } from "./helpers.js";

test("Voice document tree supports real folder filing and mixed root reordering", async ({
  page,
}) => {
  await page.goto("/ui/tests/browser/voice-workspace-mobile.html?mode=document&lang=en");
  await page.getByRole("button", { name: "New folder", exact: true }).click();
  await page.getByRole("textbox", { name: "New folder", exact: true }).fill("Meetings");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const folder = page.locator("[data-voice-folder-group]");
  const folderButton = folder.locator("[data-voice-folder] button[aria-expanded]");
  const documentRow = page
    .locator('aside [role="button"]')
    .filter({ hasText: "Linked activity document" });
  const drag = async (source, destination) => {
    const from = await source.boundingBox();
    if (!from) throw new Error("Drag source must be visible");
    await page.mouse.move(from.x + 24, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(from.x + 34, from.y + from.height / 2, { steps: 3 });
    await page.mouse.move(destination.x, destination.y, { steps: 15 });
    await page.mouse.up();
  };
  const center = async (locator) => {
    const box = await locator.boundingBox();
    if (!box) throw new Error("Drop target must be visible");
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  };
  await drag(documentRow, await center(folderButton));
  await expect(folderButton).toHaveAttribute("aria-expanded", "true");
  await expect(
    folder.getByRole("button", { name: /Linked activity document/ }).first(),
  ).toBeVisible();
  const root = page.locator("[data-voice-folder-root]");
  const rootBox = await root.boundingBox();
  if (!rootBox) throw new Error("Root drop zone must be visible");
  await drag(documentRow, { x: rootBox.x + rootBox.width / 2, y: rootBox.y + rootBox.height - 6 });
  await expect(
    folder.locator('[role="button"]').filter({ hasText: "Linked activity document" }),
  ).toHaveCount(0);
  await expect(documentRow).toBeVisible();
  await drag(folderButton, await center(page.locator("[data-voice-root-slot]").last()));
  const rootItems = root.locator(
    ":scope > [data-voice-folder-group], :scope > [data-voice-root-slot]",
  );
  await expect(rootItems.last()).toHaveAttribute("data-voice-folder-group", "true");
  await expect
    .poll(() =>
      page.evaluate(() =>
        window.voiceWorkspaceProbe.writes
          .filter((write) => write.body.action === "reorder_root")
          .at(-1)
          ?.body.root_order.at(-1),
      ),
    )
    .toBe("folder:folder-1");
});

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
