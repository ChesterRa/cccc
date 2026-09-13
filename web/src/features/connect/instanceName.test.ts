import { expect, it } from "vite-plus/test";
import { instanceName } from "./instanceName";
import type { ConnectInstance } from "./protocol";

it("disambiguates equal host names without changing the account name or adding IDs to unique names", () => {
  const a = { instance_id: "instance-aaaaaa", display_name: "Office" } as ConnectInstance;
  const b = { instance_id: "instance-bbbbbb", display_name: "office" } as ConnectInstance;
  expect(instanceName(a, [a])).toBe("Office");
  expect(instanceName(a, [a, b])).toBe("Office · aaaaaa");
  expect(instanceName(b, [a, b])).toBe("office · bbbbbb");
  expect(a.display_name).toBe("Office");
  expect(instanceName(a, [{ ...b, display_name: "Laptop" }])).toBe("Office");
});
