# Isolated browser checks

From the repository root:

```sh
npm ci --prefix web
npm -C web exec -- playwright install --with-deps chromium
npm -C web run test:browser
npm -C web run test:browser:matrix
```

The critical suite runs in CI and the full local quality gate. Nightly adds English,
Chinese and Japanese, light/dark themes, desktop and short mobile viewports, including
125% text scaling. `CHROME_BIN` may select a local test executable; fresh browser
contexts are always used. Native mobile browsers and real providers are not covered.

Playwright owns a fixture-only Vite server on `127.0.0.1:15559` and fails if that port
is already occupied. It never reuses your dev server, browser profile or daemon.
The existing production-component fixtures mock HTTP, WebSocket, microphone and
provider boundaries. The server has no backend proxy and tests reject external HTTP
requests. No account tokens or provider credentials are needed.

Failures retain screenshots, traces and an HTML report under `web/test-results/`
and `web/playwright-report/`; CI uploads them for seven days. To inspect locally:

```sh
cd web
npx playwright show-report
```

Tests use actual pointer/keyboard input. For clipped controls use `wheelTo`, which
scrolls real scroll containers with wheel input. Programmatic `scrollIntoView` can
move an `overflow:hidden` container and conceal a real accessibility regression.
Fixture controls may simulate server-side changes, but must not replace user actions
whose reachability is under test. No automatic retry hides an intermittent failure.

The other Python/Shell scripts are focused diagnostic probes and larger historical
matrices. They remain useful for their documented scenarios; they are not all CI
entry points. Add durable critical regressions to `gates/` instead of copying browser
launch, CDP connection and cleanup code into another one-off script.

## Actor configuration through real ports

Linux CI also runs `actor-config.py` with an isolated `CCCC_HOME`, owned daemon/Web
processes and a fresh browser. It uses the actual app and HTTP/IPC implementations;
only injected secret-save failures are synthetic. No Actors start and no provider
account is used. This complements the component fixtures above, which cannot catch
Web/daemon contract mismatches.

```sh
npm -C web run build
cargo build --locked -p cccc --bin cccc
python3 web/tests/browser/actor-config.py --binary target/debug/cccc
# The same check accepts target/release/cccc after a package build.
```

The test covers linked Actor rename/Profile switch/conversion, exact command
arguments, secret drafts, partial-save retry, and duplicate Profile prevention.
Failure screenshots and traces are retained under `web/test-results/actor-config/`.
