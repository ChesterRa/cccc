import { test, expect, groupPage, wheelTo } from "./helpers.js";

test("connection feedback preserves the draft and clears after reconnection", async ({ page }) => {
  await groupPage(page);
  const input = page.getByRole("textbox", { name: "Message input" });
  await input.fill("Keep this draft");
  await page.evaluate("groupWorkProbe.setConnectionStatus('disconnected')");
  await expect(page.locator("header [role=status]:visible")).toHaveText("Disconnected");
  await expect(input).toBeFocused();
  await page.evaluate("groupWorkProbe.setConnectionStatus('connecting')");
  await expect(page.locator("header [role=status]:visible")).toHaveText("Reconnecting…");
  await page.evaluate("groupWorkProbe.setConnectionStatus('connected')");
  await expect(page.locator("header [role=status]:visible")).toHaveCount(0);
  await expect(input).toHaveValue("Keep this draft");
  await expect(input).toBeFocused();
});

test("an interrupted send keeps its draft and reports an unconfirmed result without retry", async ({
  page,
}) => {
  await groupPage(page);
  await page.evaluate(() => {
    const original = window.fetch;
    window.interruptedSends = 0;
    window.fetch = async (...args) => {
      if (String(args[0]).endsWith("/send")) {
        interruptedSends++;
        throw new TypeError("Network response was interrupted");
      }
      return original(...args);
    };
  });
  const input = page.getByRole("textbox", { name: "Message input" });
  await input.fill("Keep uncertain delivery");
  await input.press("Control+Enter");
  await expect(input).toHaveValue("Keep uncertain delivery");
  await expect.poll(() => page.evaluate("interruptedSends")).toBe(1);
  await expect
    .poll(() => page.evaluate("groupWorkProbe.ui.getState().errorMsg"))
    .toContain("Delivery could not be confirmed");
  expect(await page.evaluate("interruptedSends")).toBe(1);
});

test("same-version build differences are visible and can be copied without credentials", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await groupPage(page);
  await page.evaluate(() => {
    const original = window.fetch;
    window.fetch = async (...args) =>
      String(args[0]).includes("/api/v1/ping")
        ? Response.json({
            ok: true,
            result: {
              version: "0.4.40",
              build: { source_id: "web-source" },
              daemon: { version: "0.4.40", build: { source_id: "daemon-source" } },
              web: { assets_id: "bundle", entry_script: "/ui/assets/entry.js" },
            },
          })
        : original(...args);
    groupWorkProbe.openSettings("global", "developer");
  });
  await expect(page.getByText("web-source", { exact: true })).toBeVisible();
  await expect(page.getByText(/These components use different builds/)).toBeVisible();
  await page.getByRole("button", { name: "Copy build information" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByRole("status")).toHaveText(
    "Copied",
  );
  const copied = JSON.parse(await page.evaluate("navigator.clipboard.readText()"));
  expect(copied.webSource).toBe("web-source");
  expect(copied.daemonSource).toBe("daemon-source");
  expect(Object.keys(copied).sort()).toEqual(
    [
      "daemonSource",
      "daemon_version",
      "loadedEntry",
      "servedEntry",
      "webAssets",
      "webSource",
      "web_version",
    ].sort(),
  );
  await test
    .info()
    .attach("build-diagnostics", { body: await page.screenshot(), contentType: "image/png" });
});

test("remote references survive Group switching and a late Voice draft update", async ({
  page,
}) => {
  await groupPage(page);
  const composer = page.locator("textarea");
  await composer.fill("#");
  await page.getByRole("option").filter({ hasText: "Mac Studio" }).click();
  await composer.press("End");
  await composer.pressSequentially("@");
  await page.getByRole("option").filter({ hasText: "Remote worker" }).click();
  await page.evaluate("groupWorkProbe.chooseGroup('g2')");
  await expect(composer).toHaveValue("");
  await page.evaluate(async () => {
    const { routeVoiceTextToComposerGroup } =
      await import("/ui/src/pages/chat/voice-secretary/voiceComposerDraftRouting.ts");
    routeVoiceTextToComposerGroup({ groupId: "g1", text: "voice update", mode: "append" });
  });
  await expect(composer).toHaveValue("");
  await page.evaluate("groupWorkProbe.chooseGroup('g1')");
  await expect(composer).toHaveValue(/voice update$/);
  await composer.press("Control+Enter");
  await expect
    .poll(() => page.evaluate("groupWorkProbe.requests.filter(r=>r.path.endsWith('/send')).length"))
    .toBe(1);
  const sent = await page.evaluate("groupWorkProbe.requests.find(r=>r.path.endsWith('/send'))");
  expect(sent.path).toBe("/api/v1/groups/g1/send");
  expect(sent.body.refs[0].instance_id).toBe("i_mac");
  expect(sent.body.text).toContain("voice update");
  expect(sent.body.dst_instance_id).toBeUndefined();
  await expect(composer).toHaveValue("");
});

test("retained terminals preserve their view and resynchronize a returning writer", async ({
  page,
}) => {
  await groupPage(page);
  await page.getByRole("button", { name: "Terminals", exact: true }).click();
  const first = page.locator(
    '[data-runtime-group-id="g1"][data-runtime-actor-id="actor-1"] .xterm-screen',
  );
  await expect(first).toBeVisible();
  await expect
    .poll(() => page.evaluate("groupWorkProbe.sockets.filter(s=>s.readyState===1).length"))
    .toBe(4);
  await page.evaluate(async () => {
    const p = window.groupWorkProbe;
    window.keptTerm = p.terminals.find((t) =>
      t.element?.closest('[data-runtime-actor-id="actor-1"]'),
    );
    window.keptSocket = p.sockets.find((s) => s.readyState === 1 && s.actor === "actor-1");
    window.keptScreen = keptTerm.element.querySelector(".xterm-screen");
    await new Promise((done) =>
      keptTerm.write(Array.from({ length: 200 }, (_, i) => `cache line ${i}\r\n`).join(""), done),
    );
    keptTerm.scrollToLine(30);
    keptTerm.select(0, 32, 8);
    window.keptScroll = keptTerm.buffer.active.viewportY;
    window.keptSelection = keptTerm.getSelection();
    window.keptSize = { cols: keptTerm.cols, rows: keptTerm.rows };
  });
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(first).toBeHidden();
  await expect
    .poll(() => page.evaluate("groupWorkProbe.sockets.filter(s=>s.readyState===1).length"))
    .toBe(8);
  const resizes = await page.evaluate("keptSocket.frames.filter(f=>f.type===50).length");
  await page.evaluate(() => {
    for (const terminal_writable of [false, true]) {
      keptSocket.onmessage({
        data: new TextEncoder().encode("6" + JSON.stringify({ terminal_writable })).buffer,
      });
    }
    groupWorkProbe.chooseGroup("g2");
  });
  await expect(page.getByRole("textbox", { name: "Message input" })).toBeVisible();
  expect(await page.evaluate("keptSocket.frames.filter(f=>f.type===50).length")).toBe(resizes);
  await page.evaluate("groupWorkProbe.chooseGroup('g1')");
  await page.getByRole("button", { name: "Previous page", exact: true }).click();
  await expect(first).toBeVisible();
  await expect
    .poll(() => page.evaluate("keptSocket.frames.filter(f=>f.type===50).length"))
    .toBe(resizes + 1);
  expect(
    await page.evaluate("JSON.parse(keptSocket.frames.filter(f=>f.type===50).at(-1).text)"),
  ).toEqual(await page.evaluate("keptSize"));
  expect(
    await page.evaluate(
      "keptTerm.element.querySelector('.xterm-screen')===keptScreen && keptTerm.buffer.active.viewportY===keptScroll && keptTerm.getSelection()===keptSelection",
    ),
  ).toBe(true);
  expect(await page.evaluate("groupWorkProbe.sockets.length")).toBe(8);
});

test("PDF automatic refresh keeps the same iframe and URL", async ({ page }) => {
  await groupPage(page);
  // Native PDF rendering is browser-owned. Observe its actual iframe lifecycle;
  // the asset itself stays local and does not require a daemon or external URL.
  await page.route("**/presentation/slots/**", (route) =>
    route.fulfill({ contentType: "application/pdf", body: "%PDF-1.4\n%%EOF" }),
  );
  await page.evaluate(() => {
    groupWorkProbe.group.setState({
      groupPresentation: {
        v: 1,
        slots: [
          {
            slot_id: "slot-1",
            index: 1,
            card: {
              slot_id: "slot-1",
              title: "Report",
              card_type: "pdf",
              published_at: "2026-09-18T00:00:00Z",
              published_by: "worker",
              content: { mode: "workspace_link", workspace_rel_path: "report.pdf" },
            },
          },
        ],
      },
    });
    groupWorkProbe.modals
      .getState()
      .setPresentationViewer({ groupId: "g1", slotId: "slot-1", surface: "modal" });
  });
  const iframe = page.locator('[role="dialog"] iframe');
  await expect(iframe).toBeVisible();
  await page.evaluate(() => {
    window.keptPdf = document.querySelector('[role="dialog"] iframe');
    window.pdfSource = keptPdf.src;
    window.pdfMutations = 0;
    new MutationObserver(() => window.pdfMutations++).observe(keptPdf, {
      attributes: true,
      attributeFilter: ["src"],
    });
  });
  await page.clock.install();
  await page.clock.runFor(16_000);
  expect(
    await page.evaluate(
      "document.querySelector('[role=dialog] iframe')===keptPdf && keptPdf.src===pdfSource && pdfMutations===0",
    ),
  ).toBe(true);
});

test("Web Access refresh preserves the complete draft through Save", async ({ page }) => {
  await groupPage(page);
  await page.evaluate(() => {
    const original = window.fetch;
    window.accessSaves = [];
    let access = {
      provider: "off",
      mode: "tailnet_only",
      require_access_token: true,
      enabled: false,
      status: "stopped",
      config: { web_host: "127.0.0.1", web_port: 8848, web_public_url: "" },
    };
    window.fetch = async (...args) => {
      const path = new URL(String(args[0]), location.href).pathname;
      if (path.endsWith("/access-tokens"))
        return Response.json({
          ok: true,
          result: {
            access_tokens: [
              { token_id: "fixture-admin", user_id: "owner", is_admin: true, allowed_groups: [] },
            ],
          },
        });
      if (path.endsWith("/remote_access")) {
        if (args[1]?.method === "PUT") {
          const draft = JSON.parse(args[1].body);
          accessSaves.push(draft);
          access = {
            ...access,
            ...draft,
            config: {
              web_host: draft.web_host,
              web_port: draft.web_port,
              web_public_url: draft.web_public_url,
            },
          };
        }
        return Response.json({ ok: true, result: { remote_access: access } });
      }
      return original(...args);
    };
    groupWorkProbe.openSettings("global", "webAccess");
  });
  await page.getByRole("button", { name: /Private network/ }).click();
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  await expect(page.getByRole("button", { name: "Refresh", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect.poll(() => page.evaluate("accessSaves.length")).toBe(1);
  expect(await page.evaluate("accessSaves[0]")).toMatchObject({
    provider: "manual",
    web_host: "0.0.0.0",
    require_access_token: true,
  });
  await expect(page.getByRole("button", { name: "Saved", exact: true })).toBeVisible();
});

for (const mode of ["instruction", "prompt"]) {
  test(`Voice ${mode} opens linked documents without changing the recording target`, async ({
    page,
  }) => {
    await page.goto(`/ui/tests/browser/voice-workspace-mobile.html?mode=${mode}&lang=en`);
    const link = page
      .locator('[data-voice-activity-item="first"] [data-voice-document-link]')
      .last();
    await expect(link).toBeVisible();
    await page.locator("[data-voice-record]").click();
    await expect.poll(() => page.evaluate("voiceWorkspaceProbe.starts")).toBe(1);
    const writes = await page.evaluate("voiceWorkspaceProbe.writes.length");
    await link.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-voice-document-title]")).toHaveText(
      "Linked activity document",
    );
    await expect(page.locator("[data-voice-back-to-activity]")).toBeFocused();
    await expect(page.locator("[data-voice-mobile-sheet]")).toHaveAttribute(
      "data-voice-sheet-mode",
      mode,
    );
    expect(await page.evaluate("voiceWorkspaceProbe.stops")).toBe(0);
    expect(await page.evaluate("voiceWorkspaceProbe.writes.length")).toBe(writes);
    await page.keyboard.press("Enter");
    await expect(link).toBeFocused();
    await page.locator("[data-voice-record]").click();
    await expect.poll(() => page.evaluate("voiceWorkspaceProbe.stops")).toBe(1);
  });
}

for (const [width, height, lang, scale] of [
  [320, 568, "ja", 125],
  [390, 667, "en", 100],
]) {
  test(`short Prompt controls remain reachable at ${width}x${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.goto(
      `/ui/tests/browser/voice-workspace-mobile.html?mode=prompt&lang=${lang}&scale=${scale}&extra=15`,
    );
    await page.locator(".voice-mobile-prompt-toggle").click();
    const point = await wheelTo(
      page,
      page.locator("[data-voice-workspace-optimize]"),
      page.locator("[data-voice-request-panel]"),
    );
    await page.mouse.click(point.x, point.y);
    await expect
      .poll(() =>
        page.evaluate("voiceWorkspaceProbe.writes.some(r=>r.body.kind==='prompt_refine')"),
      )
      .toBe(true);
    const link = page
      .locator('[data-voice-activity-item="first"] [data-voice-document-link]')
      .last();
    const linkPoint = await wheelTo(page, link);
    await page.mouse.click(linkPoint.x, linkPoint.y);
    await expect(page.locator("[data-voice-document-title]")).toHaveText(
      "Linked activity document",
    );
    await page.locator("[data-voice-back-to-activity]").click();
    const area = page.locator("[data-voice-activity-scroll]");
    await area.hover();
    await page.mouse.wheel(0, 300);
    await expect.poll(() => area.evaluate((e) => e.scrollTop)).toBeGreaterThan(0);
  });
}
