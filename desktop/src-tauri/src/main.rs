// Prevents an additional console window on Windows in release mode.
// DO NOT REMOVE this attribute.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  app_lib::run();
}
