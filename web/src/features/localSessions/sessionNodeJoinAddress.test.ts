import { expect, it } from "vite-plus/test";
import { sessionNodeJoinCommand } from "./sessionNodeJoin";

it("copies only a shareable HTTPS origin as a terminal argument", () => {
  expect(sessionNodeJoinCommand(null, "https://hub.example:8443")).toBe(
    "cccc sessions node join https://hub.example:8443",
  );
  expect(sessionNodeJoinCommand("https://[2001:db8::1]/", "http://localhost")).toBe(
    'cccc sessions node join "https://[2001:db8::1]"',
  );
  for (const origin of [
    "http://office.example",
    "https://localhost.",
    "https://127.10.2.3",
    "https://[::1]",
    "https://[::ffff:127.0.0.1]",
    "https://0.0.0.0",
    "https://host.docker.internal",
    "https://user:fixture@hub.example",
    "https://hub.example/path",
    "https://hub.example/?token=fixture",
    "https://bad\u0024(echo).example",
    "https://hub.example:0",
  ])
    expect(sessionNodeJoinCommand(origin, "http://127.0.0.1")).toBeNull();
});
