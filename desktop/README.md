# CCCC Desktop

A native desktop wrapper for the CCCC web UI, built with [Tauri v2](https://tauri.app/).

> **Status**: Community contribution — macOS tested and working. Windows / Linux
> builds should work but have not been verified yet.

## What this adds

| Feature | Detail |
|---------|--------|
| Native `.app` / `.exe` | No browser required — launch CCCC like any other desktop app |
| Transparent title bar (macOS) | `titleBarStyle: Overlay` gives a seamless, chrome-free look |
| Native window drag | Uses `NSWindow.setMovableByWindowBackground` so the entire top bar is draggable, even though the UI is served from a remote origin (`localhost:8848`) |
| Packaged DMG | `tauri build` produces a signed-ready `.app` bundle and `.dmg` installer |

### Screenshot

> The transparent title bar blends into the CCCC dark UI — no grey chrome, no
> separate title-bar colour mismatch.

![cccc desktop](../screenshots/desktop-macos.png)

---

## Prerequisites

| Tool | Version |
|------|---------|
| [Rust](https://rustup.rs/) | 1.77+ |
| [Node.js](https://nodejs.org/) | 18+ |
| [CCCC daemon](https://github.com/ChesterRa/cccc) | running on `localhost:8848` |

macOS also needs Xcode Command Line Tools:

```bash
xcode-select --install
```

---

## Quick start

```bash
# 1. Install the Tauri CLI
cd desktop
npm install

# 2. Make sure the CCCC daemon is running first
cccc               # or: cccc daemon start

# 3. Launch in dev mode (hot-reload from localhost:8848)
npm run dev

# 4. Build a release bundle
npm run build
#    → desktop/src-tauri/target/release/bundle/macos/cccc.app
#    → desktop/src-tauri/target/release/bundle/dmg/cccc_x.y.z_x64.dmg
```

---

## How it works

The wrapper is a **pure shell** — it contains no frontend code of its own.
`tauri.conf.json` points both the dev URL and the release URL at
`http://localhost:8848/ui/`, so the Tauri webview simply renders whatever
the CCCC daemon serves.

### Transparent title bar + drag (macOS)

macOS's `titleBarStyle: Overlay` hides the default chrome and exposes the
traffic-light buttons over the content area.  Because the webview loads a
**remote** origin, Tauri's `data-tauri-drag-region` attribute and the JS
`startDragging()` API are both blocked by the renderer sandbox.  We work
around this with two layers:

1. **CSS shim** (injected via `on_page_load`): a 28 px transparent overlay div
   is appended to every page so the UI body is pushed down below the
   traffic-light buttons.

2. **Native NSWindow API** (Rust `setup`): `setMovableByWindowBackground: YES`
   tells macOS to treat any unobstructed window background pixel as a drag
   handle, which works regardless of content origin.

```
src-tauri/src/lib.rs
  ├── INJECT_SCRIPT   — CSS shim injected on every page load
  └── setup()
        └── #[cfg(target_os = "macos")]
              └── NSWindow::setMovableByWindowBackground(true)
```

---

## Configuration

All Tauri settings live in `src-tauri/tauri.conf.json`.  The most likely
things you might want to change:

```jsonc
{
  "build": {
    // Change if your CCCC daemon runs on a different port
    "devUrl": "http://localhost:8848/ui/"
  },
  "app": {
    "windows": [{
      "width": 1280,    // initial window size
      "height": 800,
      "minWidth": 900,
      "minHeight": 600
    }]
  }
}
```

---

## Project layout

```
desktop/
├── package.json               npm wrapper (only @tauri-apps/cli)
├── src-tauri/
│   ├── Cargo.toml             Rust crate (cccc-desktop)
│   ├── tauri.conf.json        Tauri app configuration
│   ├── capabilities/
│   │   └── default.json       permission set
│   ├── icons/                 app icons (all sizes + .icns / .ico)
│   └── src/
│       ├── main.rs            binary entry point
│       └── lib.rs             Tauri builder + shim injection
└── README.md                  this file
```

---

## Contributing

Issues and PRs welcome.  If you verify the wrapper on **Windows** or
**Linux**, please open an issue or PR to update this README with your findings.
