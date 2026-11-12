#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args_os().any(|arg| arg == "--headless") {
        minerdesk_lib::run_headless();
    } else {
        minerdesk_lib::run();
    }
}
