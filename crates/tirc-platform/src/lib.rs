//! Platform integrations for TIRC.

use std::io::Write;

use thiserror::Error;

/// Cross-platform notification wrapper.
#[derive(Debug, Clone)]
pub struct PlatformNotifier {
    pub enable_bell: bool,
    pub enable_macos_notifications: bool,
}

impl Default for PlatformNotifier {
    fn default() -> Self {
        Self {
            enable_bell: true,
            enable_macos_notifications: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("notification error: {0}")]
    Notification(String),
}

impl PlatformNotifier {
    /// Emits a terminal bell to the active terminal emulator.
    pub fn ring_bell(&self) {
        if !self.enable_bell {
            return;
        }

        print!("\x07");
        let _ = std::io::stdout().flush();
    }

    /// Sends a desktop notification when supported.
    #[allow(unused_variables)]
    pub fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        if !self.enable_macos_notifications {
            return Ok(());
        }

        #[cfg(all(target_os = "macos", feature = "macos-notify"))]
        {
            notify_rust::Notification::new()
                .summary(title)
                .body(body)
                .show()
                .map_err(|err| PlatformError::Notification(err.to_string()))?;
        }

        Ok(())
    }
}
