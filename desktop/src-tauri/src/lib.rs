/// Inject a lightweight drag-region shim into every page loaded inside the
/// webview. We cannot rely on `data-tauri-drag-region` here because the web UI
/// is served from a remote origin (`http://localhost:8848`), and Tauri only
/// honors that attribute for locally bundled content. Instead we inject a thin
/// transparent overlay div at the top of the page and use the macOS-native
/// `setMovableByWindowBackground` API as the primary drag mechanism.
const INJECT_SCRIPT: &str = r#"
  (function () {
    if (document.getElementById('_tauri_drag_region')) return;
    var s = document.createElement('style');
    s.textContent = [
      'body { padding-top: 28px !important; box-sizing: border-box; }',
      '#_tauri_drag_region {',
      '  position: fixed; top: 0; left: 0; right: 0; height: 28px;',
      '  z-index: 999999; cursor: default;',
      '}'
    ].join('');
    document.head.appendChild(s);
    var d = document.createElement('div');
    d.id = '_tauri_drag_region';
    d.addEventListener('mousedown', function (e) {
      if (e.buttons !== 1 || !window.__TAURI__?.window?.getCurrentWindow) return;
      window.__TAURI__.window.getCurrentWindow().startDragging();
    });
    document.body.appendChild(d);
  })();
"#;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .on_page_load(|window, _payload| {
      let _ = window.eval(INJECT_SCRIPT);
    })
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
