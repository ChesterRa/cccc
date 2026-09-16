// Cross-member UI journey through the production account handler and native Web.
// Used only by connect-workbench.mjs with CCCC_CONNECT_GROUPS_PROBE=1.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { join } from "node:path";

export async function connectGroupJourney({
  call,
  evaluate,
  eventually,
  ipc,
  homes,
  groups,
  origins,
  tokens,
  account,
  dir,
  failures,
}) {
  const groupDialog =
    "[...document.querySelectorAll('[role=dialog]')].find(d=>d.getClientRects().length && d.innerText.includes('Group connections'))";
  const body = () => evaluate("document.body?.innerText || ''");
  const navigate = async (url) => {
    const navigation = await call("Page.navigate", { url });
    assert(!navigation.errorText, "fixture navigation succeeds");
    await eventually(async () => {
      const { frameTree } = await call("Page.getFrameTree");
      if (navigation.loaderId && frameTree.frame.loaderId !== navigation.loaderId) return false;
      return evaluate("document.readyState !== 'loading'");
    }, "new document loaded");
  };
  const accountLogin = async (index) => {
    await call("Network.setCookie", {
      name: "cccc_account",
      value: account.sessions[index],
      url: account.origin,
      path: "/",
      httpOnly: true,
      sameSite: "Lax",
    });
  };
  const input = async (selector, value, react = false) =>
    evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(selector)});if(!input)throw Error('missing input');
    ${react ? `Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(value)})` : `input.value=${JSON.stringify(value)}`};
    input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));})()`);
  const click = async (text) => {
    const position = await evaluate(`(() => {
      const button = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)} || b.getAttribute('aria-label') === ${JSON.stringify(text)});
      if (!button || button.disabled) throw Error('button unavailable: ' + ${JSON.stringify(text)});
      const r = button.getBoundingClientRect();
      return {x: r.left + r.width / 2, y: r.top + r.height / 2};
    })()`);
    await call("Input.dispatchMouseEvent", {
      type: "mousePressed",
      button: "left",
      clickCount: 1,
      ...position,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      button: "left",
      clickCount: 1,
      ...position,
    });
  };
  const openNative = async (index, url = `${origins[index]}/ui/`) => {
    await navigate(url);
    await eventually(
      () =>
        evaluate(
          "!!document.querySelector('input[name=cccc-access-token]') || !!document.querySelector('header')",
        ),
      "native login or shell",
    );
    if (await evaluate("!!document.querySelector('input[name=cccc-access-token]')")) {
      await input("input[name=cccc-access-token]", tokens[index], true);
      await evaluate("document.querySelector('form').requestSubmit()");
    }
    await eventually(() => evaluate("!!document.querySelector('header')"), "native shell");
    await evaluate("localStorage.setItem('i18nextLng','en')");
    await navigate(url);
    await eventually(
      () => evaluate("!!document.querySelector('aside button[aria-haspopup=menu]')"),
      "Group connection entry",
    );
  };
  const openConnections = async () => {
    // Open the selected Group's menu; connections are a Group action, not a header toggle.
    await evaluate("document.querySelector('aside button[aria-haspopup=menu]').click()");
    await eventually(
      () => evaluate("!!document.querySelector('[role=menuitem]')"),
      "Group actions menu",
    );
    await click("Group connections");
  };
  const continueToAccount = async (button) => {
    await click(button);
    let popup;
    await eventually(async () => {
      popup = (await call("Target.getTargets")).targetInfos.find(
        (t) => t.type === "page" && t.url.startsWith(account.origin + "/connect/select?"),
      );
      return !!popup;
    }, "native selection opens account confirmation");
    const url = popup.url;
    await call("Target.closeTarget", { targetId: popup.targetId });
    await navigate(url);
    await eventually(
      () =>
        evaluate(
          "!!document.querySelector('input[name=csrf_token]') && !!document.querySelector('input[name=ticket]')",
        ),
      "member confirmation form",
    );
  };
  const screenshot = async (name) =>
    writeFileSync(
      join(dir, name),
      Buffer.from((await call("Page.captureScreenshot", { format: "png" })).data, "base64"),
    );

  await accountLogin(0);
  await openNative(0);
  assert(
    !(await body()).includes("fixture-b"),
    "other-member instance must not enter the aggregate sidebar",
  );
  await openConnections();
  await eventually(
    () =>
      evaluate(
        "!![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Invite a member' && !b.disabled)",
      ),
    "source Group can be selected",
  );
  await continueToAccount("Invite a member");
  assert((await body()).includes("Workspace A"), "invitation identifies the native source Group");
  const sourcePath = join(homes[0], "groups", groups[0], "group.yaml");
  const sourceYaml = readFileSync(sourcePath, "utf8");
  assert(/^generation: .+$/m.test(sourceYaml), "fixture has explicit resource generation");
  writeFileSync(sourcePath, sourceYaml.replace(/^generation: .+$/m, `generation: ${randomUUID()}`));
  await input("input[name=recipient]", account.members[1]);
  await evaluate("document.querySelector('main form').requestSubmit()");
  await eventually(
    () =>
      evaluate(
        "!!document.querySelector('main [role=alert]') && !document.querySelector('input[name=ticket]')",
      ),
    "stale native selection cannot enter offline retry",
  );
  assert((await body()).includes("select the Group again in CCCC"));
  assert(!(await body()).includes("Both selected Groups must be online"));
  await screenshot("groups-selection-changed.png");
  await openNative(0);
  await openConnections();
  await eventually(
    () =>
      evaluate(
        "!![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Invite a member' && !b.disabled)",
      ),
    "fresh Group selection ready",
  );
  await continueToAccount("Invite a member");
  await input("input[name=recipient]", account.members[1]);
  assert.equal(
    (await fetch(account.origin + "/__fixture/fail-group-check", { method: "POST" })).status,
    204,
  );
  await evaluate("document.querySelector('main form').requestSubmit()");
  await eventually(
    () =>
      evaluate(
        "!!document.querySelector('main [role=alert]') && !!document.querySelector('input[name=ticket]')",
      ),
    "offline invitation retains confirmation",
  );
  assert.equal(
    await evaluate("document.querySelector('input[name=recipient]').value"),
    account.members[1],
  );
  assert((await body()).includes("Workspace A"));
  await screenshot("groups-offline-retry.png");
  await evaluate("document.querySelector('main form').requestSubmit()");
  await eventually(
    () => evaluate("location.pathname==='/connect' && location.search.includes('saved=1')"),
    "invitation committed",
  );
  assert((await body()).includes("Waiting for acceptance"));
  await screenshot("groups-invited.png");
  await call("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 1,
    mobile: true,
  });
  assert(
    await evaluate("document.documentElement.scrollWidth <= innerWidth"),
    "account connections fit a narrow viewport",
  );
  await screenshot("groups-account-mobile.png");
  await call("Emulation.clearDeviceMetricsOverride");
  await accountLogin(1);
  await navigate(account.origin + "/connect?lang=en");
  await eventually(
    () => evaluate("!!document.querySelector('a[href*=connect_invite]')"),
    "recipient sees invitation",
  );
  const destination = await evaluate("document.querySelector('a[href*=connect_invite]').href");
  assert(destination.startsWith(origins[1]), "recipient chooses only an owned instance");
  await openNative(1, destination);
  await eventually(
    () =>
      evaluate(
        "!!document.querySelector('[role=dialog]') && !![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Review invitation on website' && !b.disabled)",
      ),
    "invitation opens Group chooser after native login",
  );
  await continueToAccount("Review invitation on website");
  const confirmation = await body();
  assert(
    confirmation.includes("Workspace A") && confirmation.includes("Workspace B"),
    "both concrete Groups shown before approval",
  );
  await screenshot("groups-confirm.png");
  const selectedInvitation = await evaluate(
    "document.querySelector('input[name=invitation]').value",
  );
  const targetPath = join(homes[1], "groups", groups[1], "group.yaml");
  const targetYaml = readFileSync(targetPath);
  try {
    writeFileSync(targetPath, "v: [invalid YAML");
    await evaluate("document.querySelector('main form').requestSubmit()");
    await eventually(
      () =>
        evaluate(
          "!!document.querySelector('main [role=alert]') && !!document.querySelector('input[name=ticket]')",
        ),
      "temporary native read failure retains both Groups",
    );
  } finally {
    writeFileSync(targetPath, targetYaml);
  }
  assert.equal(
    await evaluate("document.querySelector('input[name=invitation]').value"),
    selectedInvitation,
  );
  assert((await body()).includes("Workspace A") && (await body()).includes("Workspace B"));
  await evaluate("document.querySelector('main form').requestSubmit()");
  await eventually(
    () => evaluate("location.pathname==='/connect' && location.search.includes('saved=1')"),
    "acceptance committed",
  );
  let local;
  await eventually(
    async () => {
      local = await ipc(homes[0], "connect_group_status", { by: "user", group_id: groups[0] });
      return (
        local.links.length === 1 &&
        (await ipc(homes[1], "connect_group_status", { by: "user", group_id: groups[1] })).links
          .length === 1
      );
    },
    "both daemons synchronize the connection",
    80000,
  );
  const link = local.links[0];
  const unrelated = await ipc(homes[2], "connect_catalog", { by: "user", group_id: groups[2] });
  assert.deepEqual(
    unrelated.external_groups,
    [],
    "same-account sibling instance does not inherit external Group access",
  );
  await eventually(
    async () => {
      // A is loopback HTTP. The daemon intentionally does not trust this browser fixture's private TLS CA.
      // Both transport directions are covered by the separate real-HTTP daemon regression.
      const result = await ipc(homes[1], "connect_catalog", {
        by: "user",
        group_id: groups[1],
        instance_id: link.source.instance.instance_id,
        target_group_id: groups[0],
      });
      return result.catalog?.groups[0]?.group_id === groups[0];
    },
    "external Actor catalogue",
    25000,
  );

  await openNative(1);
  await openConnections();
  await eventually(
    () => evaluate(`!!(${groupDialog})?.innerText.includes('Workspace A')`),
    "native connection summary",
  );
  await screenshot("groups-native-desktop.png");
  await call("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 1,
    mobile: true,
  });
  assert(
    await evaluate(
      `(()=>{const r=(${groupDialog}).getBoundingClientRect();return r.left>=0 && r.right<=innerWidth && r.bottom<=innerHeight;})()`,
    ),
    "mobile dialog fits the viewport",
  );
  await screenshot("groups-native-mobile.png");
  await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape" });
  await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape" });
  await eventually(() => evaluate(`!(${groupDialog})`), "Escape closes native dialog");
  await call("Emulation.clearDeviceMetricsOverride");
  await navigate(`${account.origin}/connect/${link.id}/disconnect?lang=en`);
  await eventually(
    () => evaluate("!!document.querySelector('input[name=csrf_token]')"),
    "member disconnect confirmation",
  );
  await evaluate("document.querySelector('main form').requestSubmit()");
  await eventually(
    () => evaluate("location.pathname==='/connect' && location.search.includes('saved=1')"),
    "disconnect committed",
  );
  await eventually(
    async () =>
      (await ipc(homes[0], "connect_group_status", { by: "user", group_id: groups[0] })).links
        .length === 0 &&
      (await ipc(homes[1], "connect_group_status", { by: "user", group_id: groups[1] })).links
        .length === 0,
    "both daemons retire the connection",
    80000,
  );
  assert.equal(failures.length, 0, `browser exceptions: ${failures.join(", ")}`);
  const result = {
    invitationFromNativeGroup: true,
    staleNativeSelectionRejected: true,
    temporaryNativeReadRecovery: true,
    offlineInvitationRetry: true,
    offlineAcceptanceRetry: true,
    memberConfirmation: true,
    recipientNativeGroupSelection: true,
    realResourceChecks: true,
    boundedDaemonSync: true,
    nonTransitive: true,
    externalActorCatalog: true,
    mobileDialog: true,
    mobileAccount: true,
    keyboardClose: true,
    memberDisconnect: true,
    exceptions: failures.length,
  };
  if (process.env.CCCC_CONNECT_GROUPS_EVIDENCE) {
    // Screenshots contain only isolated fixture data. The harness removes its private credentials.
    const { mkdirSync, copyFileSync } = await import("node:fs");
    mkdirSync(process.env.CCCC_CONNECT_GROUPS_EVIDENCE, { recursive: true });
    for (const name of [
      "groups-selection-changed.png",
      "groups-invited.png",
      "groups-offline-retry.png",
      "groups-confirm.png",
      "groups-native-desktop.png",
      "groups-native-mobile.png",
      "groups-account-mobile.png",
    ])
      copyFileSync(join(dir, name), join(process.env.CCCC_CONNECT_GROUPS_EVIDENCE, name));
    writeFileSync(
      join(process.env.CCCC_CONNECT_GROUPS_EVIDENCE, "result.json"),
      JSON.stringify(result),
    );
  }
  process.stdout.write(JSON.stringify(result) + "\n");
}
