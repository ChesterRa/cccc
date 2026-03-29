/// Inject a lightweight drag-region shim into every page loaded inside the
/// webview.  We cannot rely on `data-tauri-drag-region` here because the
/// web UI is served from a **remote origin** (`http://localhost:8848`), and
/// Tauri only honours that attribute for locally-bundled content.  Instead we
/// inject a thin transparent overlay div at the top of the page and fall back
/// to the macOS-native `setMovableByWindowBackground` API (see `setup` below)
/// as the primary drag mechanism.
const INJECT_SCRIPT: &str = r#"
  (function () {
    if (document.getElementById('_tauri_drag_region')) return;
    var s = document.createElement('style');
    s.textContent = [
      'body { padding-top: 28px !important; box-sizing: border-box; }',
      '#_tauri_drag_region {',
      '  position: fixed; top: 0; left: 0; right: 0; height: 28px;',
      '  z-index: 999999; cursor: default; -webkit-app-region: drag;',
      '}'
    ].join('');
    document.head.appendChild(s);
    var d = document.createElement('div');
    d.id = '_tauri_drag_region';
    document.body.appendChild(d);
  })();
"#;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // Re-inject the shim on every navigation so it survives SPA route changes.
    .on_page_load(|window, _payload| {
      let _ = window.eval(INJECT_SCRIPT);
    })
    .setup(|app| {
      // Enable the log plugin in debug builds for easier development.
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // macOS: make the entire window background draggable at the native
      // NSWindow level.  This is the reliable path for remote-origin webviews
      // where `data-tauri-drag-region` and JS `startDragging()` are blocked by
      // the renderer's CSP / IPC sandbox.
      #[cfg(target_os = "macos")]
      {
        use tauri::Manager;
        #[allow(unused_imports)]
        use objc::{msg_send, sel, sel_impl};
        if let Some(win) = app.get_webview_window("main") {
          let ns_win = win.ns_window().unwrap() as *mut objc::runtime::Object;
          unsafe {
            let _: () = msg_send![ns_win, setMovableByWindowBackground: true as i8];
          }
        }
      }

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
