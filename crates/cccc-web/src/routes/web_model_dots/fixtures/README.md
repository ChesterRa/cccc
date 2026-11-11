# Credential-rejection capture

The JSON is the real redacted HTTP 403 body observed on 2026-11-11 at approximately 02:08 UTC, after a connector credential was replaced. Its keys and message are unchanged. It contains no credentials, identities, paths or user text.

The regression test creates and rotates an actual fixture credential through the HTTP/daemon path, then compares the rejected old-credential response with this capture. No fabricated stale-secret suffix is used. Coalescing/race controls are controlled variations, not claims of additional observed incidents.
