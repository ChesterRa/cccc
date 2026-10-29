// This worker handles notifications only. It never intercepts or caches fetches.
const OPEN_MESSAGE = "cccc:push:open";
// Some browsers only permit opening a window briefly after a notification click.
const CLICK_TIMEOUT_MS = 650;

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
const OTHER_PAGES = ["/ui/push", "/ui/capabilities", "/ui/connect"];

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

function askClient(client, target, deadline) {
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
        },
        [channel.port2],
      );
    } catch {
      finish(false);
    }
  });
}

async function openTarget(target) {
  const deadline = Date.now() + CLICK_TIMEOUT_MS;
  const windows = await untilDeadline(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }),
    deadline - 100,
  );
  // A failed or unfinished lookup does not prove that there is no existing
  // app window; openWindow might reuse one and discard unsaved work.
  if (!windows) return;
  // matchAll orders windows by most recent focus. Focusing the first helps
  // resume a backgrounded mobile page before we send it the target.
  const client = windows.find(canReceiveNavigation);
  if (client) {
    try {
      await untilDeadline(client.focus(), Math.min(deadline, Date.now() + 200));
    } catch {
      // We can still message an already-open page if focusing is denied.
    }
    if (target && Date.now() < deadline && (await askClient(client, target, deadline))) return;
    // Even if focusing fails, openWindow can reuse this page and discard its
    // unsaved work. The user can open the app and find the request there.
    return;
  }
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
