// This worker handles notifications only. It never intercepts or caches fetches.
const OPEN_MESSAGE = "cccc:push:open";
// Some browsers only permit opening a window briefly after a notification click.
const CLICK_TIMEOUT_MS = 650;
// How long after that an open page still acts on the click (LATE_CLICK_MS in the page's
// pushNotifications/navigation.ts). A slow window lookup is awaited until then.
const LATE_CLICK_MS = 60_000;

function identifier(value) {
  if (typeof value !== "string") return "";
  const result = value.trim();
  if (!result || result.length > 256) return "";
  for (const character of result) {
    const code = character.charCodeAt(0);
    if (code < 32 || code === 127) return "";
  }
  return result;
}

function notificationDetails(notification) {
  const data = notification?.data;
  const groupId = data?.v === 1 ? identifier(data.group_id) : "";
  const eventId = data?.v === 1 ? identifier(data.event_id) : "";
  const target = groupId && eventId ? { group_id: groupId, event_id: eventId } : null;
  return {
    title:
      typeof notification?.title === "string" && notification.title.trim()
        ? notification.title
        : "CCCC",
    body: typeof notification?.body === "string" ? notification.body : "",
    tag: typeof notification?.tag === "string" ? notification.tag : "",
    target,
  };
}

function targetUrl(target) {
  const url = new URL("/ui/", self.location.origin);
  if (target) {
    url.searchParams.set("group", target.group_id);
    url.searchParams.set("event", target.event_id);
  }
  return url.href;
}

// Pages under /ui/ that are not the workbench and cannot show a message.
const OTHER_PAGES = ["/ui/push", "/ui/connect"];

function canReceiveNavigation(client) {
  try {
    if (client.frameType === "nested") return false;
    const url = new URL(client.url);
    const path = url.pathname.replace(/\/+$/, "");
    return (
      url.origin === self.location.origin &&
      (url.pathname === "/" || path === "/ui" || path.startsWith("/ui/")) &&
      !OTHER_PAGES.some((page) => path === page || path.startsWith(`${page}/`))
    );
  } catch {
    return false;
  }
}

function untilDeadline(promise, deadline) {
  return new Promise((resolve) => {
    let done = false;
    const finish = (value) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve(value);
    };
    const timer = setTimeout(() => finish(null), Math.max(0, deadline - Date.now()));
    Promise.resolve(promise).then(finish, () => finish(null));
  });
}

function askClient(client, target, deadline, startedAt) {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    let done = false;
    const finish = (handled) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      channel.port1.close();
      resolve(handled);
    };
    const timer = setTimeout(() => finish(false), Math.max(0, deadline - Date.now()));
    channel.port1.onmessage = (event) => finish(event.data?.handled === true);
    try {
      client.postMessage(
        {
          type: OPEN_MESSAGE,
          v: 1,
          group_id: target.group_id,
          event_id: target.event_id,
          deadline_ms: deadline,
          started_at_ms: startedAt,
        },
        [channel.port2],
      );
    } catch {
      finish(false);
    }
  });
}

// Focusing first helps resume a backgrounded mobile page before we send it the target. The
// target goes to the page even once the click's time has passed, as long as the page would
// still act on it: a page or browser slow to respond after the click reads it when it can.
// The clock is checked each time because a timer can run late in a paused worker.
async function showInPage(client, target, deadline, startedAt) {
  const current = () => Date.now() <= deadline + LATE_CLICK_MS;
  if (!current()) return;
  try {
    await untilDeadline(client.focus(), Math.min(deadline, Date.now() + 200));
  } catch {
    // We can still message an already-open page if focusing is denied.
  }
  if (target && current()) await askClient(client, target, deadline, startedAt);
}

async function openTarget(target) {
  const startedAt = Date.now();
  const deadline = startedAt + CLICK_TIMEOUT_MS;
  const lookup = self.clients.matchAll({ type: "window", includeUncontrolled: true });
  const windows = await untilDeadline(lookup, deadline - 100);
  if (!windows) {
    // A failed or unfinished lookup does not prove that there is no existing
    // app window; openWindow might reuse one and discard unsaved work. A page
    // the lookup finds later still gets the target.
    const late = await untilDeadline(lookup, deadline + LATE_CLICK_MS);
    const client = late?.find(canReceiveNavigation);
    if (client) await showInPage(client, target, deadline, startedAt);
    return;
  }
  // matchAll orders windows by most recent focus. An open page never falls back to
  // openWindow, even if focusing it fails: openWindow can reuse that page and discard
  // its unsaved work.
  const client = windows.find(canReceiveNavigation);
  if (client) return showInPage(client, target, deadline, startedAt);
  await self.clients.openWindow(targetUrl(target));
}

self.addEventListener("push", (event) => {
  let payload;
  try {
    payload = event.data?.json();
  } catch {
    payload = null;
  }
  const details = notificationDetails(payload?.notification);
  // Even malformed data produces a visible generic notification: a silent
  // push can cause browsers to revoke the subscription.
  event.waitUntil(
    self.registration.showNotification(details.title, {
      body: details.body,
      ...(details.tag ? { tag: details.tag } : {}),
      icon: "/ui/logo.png",
      data: details.target,
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const groupId = identifier(event.notification.data?.group_id);
  const eventId = identifier(event.notification.data?.event_id);
  const target = groupId && eventId ? { group_id: groupId, event_id: eventId } : null;
  event.waitUntil(openTarget(target));
});
