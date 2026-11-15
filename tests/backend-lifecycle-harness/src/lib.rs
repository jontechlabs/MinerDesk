//! Test-only entry point for the actual production backend lifecycle modules.
//! Keep native Windows listener/PID behavior in this fast preflight, without
//! building Tauri or copying a second implementation into a test fixture.
#![allow(dead_code)]

#[path = "../../../src-tauri/src/backend_lifecycle.rs"]
mod backend_lifecycle;
#[path = "../../../src-tauri/src/update_handover.rs"]
mod update_handover;
