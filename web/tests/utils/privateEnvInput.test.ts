import { describe, expect, it } from "vitest";

import { parsePrivateEnvSetText, parsePrivateEnvUnsetText } from "../../src/utils/privateEnvInput";

describe("privateEnvInput", () => {
  it("keeps existing POSIX-style set parsing", () => {
    const result = parsePrivateEnvSetText('export OPENAI_API_KEY="sk-demo"; export OPENAI_BASE_URL="https://api.example.com"');
    expect(result).toEqual({
      ok: true,
      setVars: {
        OPENAI_API_KEY: "sk-demo",
        OPENAI_BASE_URL: "https://api.example.com",
      },
    });
  });

  it("parses Windows cmd-style set statements", () => {
    const result = parsePrivateEnvSetText('set "OPENAI_API_KEY=sk-demo"\nset OPENAI_BASE_URL=https://api.example.com');
    expect(result).toEqual({
      ok: true,
      setVars: {
        OPENAI_API_KEY: "sk-demo",
        OPENAI_BASE_URL: "https://api.example.com",
      },
    });
  });

  it("parses PowerShell env assignments", () => {
    const result = parsePrivateEnvSetText('$env:OPENAI_API_KEY = "sk-demo"\n$env:OPENAI_BASE_URL="https://api.example.com"');
    expect(result).toEqual({
      ok: true,
      setVars: {
        OPENAI_API_KEY: "sk-demo",
        OPENAI_BASE_URL: "https://api.example.com",
      },
    });
  });

  it("parses unset statements across POSIX, cmd, and PowerShell forms", () => {
    const result = parsePrivateEnvUnsetText("unset OPENAI_API_KEY;\nset OPENAI_BASE_URL=\nRemove-Item Env:ANTHROPIC_API_KEY\n$env:MISTRAL_API_KEY=$null");
    expect(result).toEqual({
      ok: true,
      unsetKeys: [
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "ANTHROPIC_API_KEY",
        "MISTRAL_API_KEY",
      ],
    });
  });
});
