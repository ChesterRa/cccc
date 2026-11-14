export function invitation(expires = Date.now() + 1800000, version = 1) {
  return `cccc-direct:${btoa(
    String.fromCharCode(
      ...new TextEncoder().encode(
        JSON.stringify({
          v: version,
          id: "direct-fixture",
          address: "office.test:8847",
          secret: "a".repeat(64),
          expires_at: new Date(expires).toISOString(),
          host: {
            instance_id: "peer-a",
            name: "办公室",
            group_id: "remote-group",
            title: "Review",
          },
        }),
      ),
    ),
  )}`;
}
export function relation(overrides: Record<string, unknown> = {}) {
  return {
    id: "direct-fixture",
    local: { title: "Local A" },
    remote: null,
    state: "invited",
    initiated: false,
    current: true,
    expired: false,
    online: false,
    error: null,
    expires_at: new Date(Date.now() + 1800000).toISOString(),
    ...overrides,
  };
}
