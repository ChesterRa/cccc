# macOS session-owner platform probe

This is an isolated platform experiment, not a macOS support declaration or
the Rust session-owner implementation. It starts only fixture executables
compiled from the adjacent C source. It never launches CCCC, loads provider
configuration, or uses model credits.

The workflow runs only for a push to `probe/macos-session-owner`, on the
standard `macos-15` runner. It records the actual OS/build, architecture,
SDK/compiler, UID and commit, and rejects a non-arm64 or root environment.
It needs no repository secrets and requests read-only repository access.

The four required phases are:

| Phase | Observation | Decision informed |
| --- | --- | --- |
| Short socket | Two long real paths; private, temporary directory symlinks for bind and fresh synchronous/asynchronous connect; replies after alias removal | Whether a scoped alias can leave discovery and cleanup in CCCC_HOME |
| Darwin identity | Non-root `kern.bootsessionuuid`, exact-sized BSDInfo, UID/PID/PGID/SID/birth, explicit failed lookup | Whether public metadata can supply the candidate identity evidence |
| Socket credentials | Accepted stream's `getpeereid` and `LOCAL_PEERPID`, compared with the actual connecting Python process | Whether the APIs underlying the pinned Tokio implementation work here |
| Signed replacement | Rename old inode aside, install another signed inode, remove backup; old mapping continues, old process spawns new path, same-PID exec changes code and preserves birth | Whether old/new executable lifetimes can be distinguished during a planned replacement |

The identity probe does not turn a birth snapshot into an atomic signal
handle. It sends no recovery signals to processes discovered from metadata.
Its cleanup owns only subprocesses that it created.

Not covered: the Rust owner integration, watchdog/recovery, a real reboot,
different-user rejection, provider CLIs, Gatekeeper download quarantine,
desktop Web/TUI lifecycle, or systems older than the actual runner image.
The deployment target passed to clang is a compile check, not an execution
test on macOS 11. The two fixture build numbers are not released CCCC versions.

For a manual run on an Apple Silicon Mac with the command-line tools and
system Python 3 already installed:

```sh
probe_dir=$(mktemp -d /tmp/cccc-macos-probe.XXXXXX)
xcrun clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra \
  -mmacosx-version-min=11.0 -O1 -DPROBE_BUILD=1 \
  scripts/tests/macos_session_owner_probe.c -o "$probe_dir/old-probe"
xcrun clang -std=c11 -D_DARWIN_C_SOURCE -Wall -Wextra \
  -mmacosx-version-min=11.0 -O1 -DPROBE_BUILD=2 \
  scripts/tests/macos_session_owner_probe.c -o "$probe_dir/new-probe"
python3 -I scripts/tests/macos_session_owner_probe.py \
  --old "$probe_dir/old-probe" --new "$probe_dir/new-probe" \
  --output "$probe_dir/result.json"
```

Preserve the small result JSON and command exit code, then remove only that
`probe_dir`. The full probe refuses Linux or Intel. `--socket-only` is a
standard-library mechanism check that can run on Linux; its report explicitly
does not count as Darwin evidence.

The workflow requires all four phases to complete and every recorded check to
pass. A missing response, invalid native frame, failure or timeout returns a
nonzero exit code. Native responses allow 20 seconds for cold-start evaluation;
this experiment measures mechanisms, not startup latency. Compiler warnings
remain visible instead of blocking all runtime evidence on an untested SDK.

Artifacts contain only small evidence files, retained for three days; no
binaries, Cargo cache or provider data are uploaded. The workflow also emits
the result JSON, platform description and compiler diagnostics as public check
annotations. These can be read through the check-run annotations API when the
job-log or artifact download API requires authentication. Oversized diagnostic
text is explicitly marked truncated; the small result JSON should fit whole.
