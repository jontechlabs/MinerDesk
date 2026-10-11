use std::{fmt, io};

#[derive(Debug)]
pub struct GpuDiagnosticError {
    pub message: String,
    pub spawn_os_error: Option<i32>,
    pub timed_out: bool,
}

impl GpuDiagnosticError {
    pub fn spawn(error: io::Error) -> Self {
        Self { message: error.to_string(), spawn_os_error: error.raw_os_error(), timed_out: false }
    }

    pub fn timeout() -> Self {
        Self { message: "GPU diagnostic timed out".into(), spawn_os_error: None, timed_out: true }
    }

    pub fn code(&self, platform: &str) -> &'static str {
        if self.timed_out { "timed_out" }
        else if platform == "windows" && self.spawn_os_error == Some(5) { "windows_access_denied" }
        else { "launch_failed" }
    }
}

impl From<String> for GpuDiagnosticError {
    fn from(message: String) -> Self {
        Self { message, spawn_os_error: None, timed_out: false }
    }
}

impl fmt::Display for GpuDiagnosticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_access_denied_is_classified_by_native_code_not_translated_text() {
        let error = GpuDiagnosticError { message: "Accès refusé. (os error 5)".into(), spawn_os_error: Some(5), timed_out: false };
        assert_eq!(error.code("windows"), "windows_access_denied");
        assert_eq!(error.code("linux"), "launch_failed");
        assert_eq!(error.to_string(), "Accès refusé. (os error 5)");
        assert_eq!(GpuDiagnosticError::from("Access denied without an OS code".to_string()).code("windows"), "launch_failed");
    }

    #[test]
    fn timeout_and_other_native_errors_stay_distinct() {
        assert_eq!(GpuDiagnosticError::timeout().code("windows"), "timed_out");
        let error = GpuDiagnosticError::spawn(io::Error::from_raw_os_error(193));
        assert_eq!(error.spawn_os_error, Some(193));
        assert_eq!(error.code("windows"), "launch_failed");
    }
}
