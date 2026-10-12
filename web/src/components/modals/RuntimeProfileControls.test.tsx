import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import {
  OpenCodeManagedModelHint,
  RuntimeCommandControl,
  RuntimeProfilePicker,
} from "./RuntimeProfileControls";
import type { ActorProfile } from "../../types";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("OpenCode model selection hint", () => {
  it("marks the hint icon decorative and keeps the hint out of the alert role", () => {
    const html = renderToStaticMarkup(<OpenCodeManagedModelHint runtime="opencode" />);
    expect(html).toContain("opencodeManagedModelHint");
    expect(html).toContain("<svg");
    expect(html).toContain('aria-hidden="true"');
    expect(html).not.toContain('role="alert"');
  });

  it("normalizes the selected runtime", () => {
    expect(renderToStaticMarkup(<OpenCodeManagedModelHint runtime=" OpenCode " />)).toContain(
      "opencodeManagedModelHint",
    );
  });

  it("also explains model synchronization for Kilo", () => {
    expect(renderToStaticMarkup(<OpenCodeManagedModelHint runtime="kilo" />)).toContain(
      "opencodeManagedModelHint",
    );
  });

  it("does not warn for other runtimes or an empty selection", () => {
    for (const runtime of ["codex", "claude", "grok", "", null, undefined]) {
      expect(renderToStaticMarkup(<OpenCodeManagedModelHint runtime={runtime} />)).toBe("");
    }
  });
});

describe("Runtime Profile surface summaries", () => {
  it("keeps Dots Profiles free of launch commands and independent Actor connection credentials", () => {
    const profile = {
      id: "dots",
      runtime: "chatgpt_dots",
      name: "Dot",
      command: "",
    } as ActorProfile;
    const html = renderToStaticMarkup(
      <RuntimeProfilePicker
        value={actorProfileIdentityKey(profile)}
        profiles={[profile]}
        busy={false}
        onChange={() => {}}
      />,
    );
    expect(html).toContain("ChatGPT Dots");
    expect(html).toContain("webModelProfileHint");
    expect(html).not.toContain(" · TUI");
    expect(
      renderToStaticMarkup(
        <RuntimeCommandControl
          runtime="chatgpt_dots"
          command=""
          defaultCommand=""
          useDefaultCommand
          onCommandChange={() => {}}
          onUseDefaultCommandChange={() => {}}
        />,
      ),
    ).toBe("");
  });
  it.each([
    ["kimi", "default", "Kimi Code · TUI"],
    ["kimi", "acp", "Kimi Code · ACP"],
    ["auggie", "default", "Auggie (Augment) · TUI"],
    ["auggie", "acp", "Auggie (Augment) · ACP"],
    ["pi", "default", "Pi · RPC"],
    ["deepseek", "default", "DeepSeek Harness · ACP"],
    ["kiro", "default", "Kiro CLI · TUI"],
    ["kiro", "acp", "Kiro CLI · ACP"],
    ["hermes", "default", "Hermes Agent · TUI"],
    ["hermes", "acp", "Hermes Agent · ACP"],
    ["droid", "default", "Droid · TUI"],
    ["droid", "acp", "Droid · ACP"],
  ] as const)(
    "identifies the saved %s/%s surface in both selection and summary",
    (runtime, mode, label) => {
      const profile = {
        id: "audit-profile",
        name: "Shared name",
        scope: "global",
        runtime,
        runtime_mode: mode,
        command: "",
      } as ActorProfile;
      const html = renderToStaticMarkup(
        <RuntimeProfilePicker
          value={actorProfileIdentityKey(profile)}
          profiles={[profile]}
          busy={false}
          onChange={() => {}}
          detailsLabel="Details"
        />,
      );
      expect(html.split(label).length - 1).toBe(2);
    },
  );

  it("does not invent a surface for Custom or an unrecognized runtime", () => {
    for (const runtime of ["custom", "third_party_agent"]) {
      const profile = { id: "audit", runtime, command: "" } as ActorProfile;
      const html = renderToStaticMarkup(
        <RuntimeProfilePicker
          value={actorProfileIdentityKey(profile)}
          profiles={[profile]}
          busy={false}
          onChange={() => {}}
        />,
      );
      expect(html).not.toContain(" · TUI");
      expect(html).not.toContain(" · ACP");
      expect(html).not.toContain(" · RPC");
    }
  });
});
