import { describe, expect, it } from "vite-plus/test";

import { parsePrivateEnvSetText } from "./privateEnvInput";

describe("parsePrivateEnvSetText", () => {
  it("keeps the legacy batch set formats supported by the secret manager", () => {
    expect(
      parsePrivateEnvSetText(
        'export OPENAI_API_KEY="sk-test"; set OPENAI_BASE_URL=https://example.test\n$env:HTTP_PROXY="http://proxy.test"',
      ),
    ).toEqual({
      ok: true,
      setVars: {
        OPENAI_API_KEY: "sk-test",
        OPENAI_BASE_URL: "https://example.test",
        HTTP_PROXY: "http://proxy.test",
      },
    });
  });
});
