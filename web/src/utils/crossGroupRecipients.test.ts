import { describe, expect, it } from "vite-plus/test";

import { projectCrossGroupRecipients } from "./crossGroupRecipients";

describe("projectCrossGroupRecipients", () => {
  it("prefers the destination snapshot over canonical recipients", () => {
    expect(projectCrossGroupRecipients({ dst_to: ["peer"], to: ["@foreman"] })).toEqual(["peer"]);
  });

  it("reads canonical recipients from the real Rust ledger shape", () => {
    expect(projectCrossGroupRecipients({ to: ["@foreman"] })).toEqual(["@foreman"]);
  });

  it("defaults missing or blank recipients to foreman, never all", () => {
    expect(projectCrossGroupRecipients({ dst_to: ["  "], to: [] })).toEqual(["@foreman"]);
    expect(projectCrossGroupRecipients(undefined)).toEqual(["@foreman"]);
  });
});
