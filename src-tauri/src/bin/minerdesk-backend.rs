// Separate PE entry point: Task Scheduler launches this actual backend directly.
// Windows must not allocate a console, even briefly. The standalone CLI remains
// minerdesk-headless.exe with its console, pipes and Ctrl+C behaviour unchanged.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    minerdesk_lib::run_desktop_backend();
}
