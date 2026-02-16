//! Newtype wrappers for domain identifiers with validation.
//!
//! These types provide compile-time type safety and runtime validation
//! for profile names, network names, and channel names.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Error returned when validating a name fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameValidationError {
    pub kind: NameValidationErrorKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameValidationErrorKind {
    Empty,
    TooLong,
    InvalidChars,
    InvalidChannelPrefix,
}

impl fmt::Display for NameValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            NameValidationErrorKind::Empty => write!(f, "name cannot be empty"),
            NameValidationErrorKind::TooLong => {
                write!(f, "name too long: '{}'", self.value)
            }
            NameValidationErrorKind::InvalidChars => {
                write!(f, "name contains invalid characters: '{}'", self.value)
            }
            NameValidationErrorKind::InvalidChannelPrefix => {
                write!(f, "channel name must start with # or &: '{}'", self.value)
            }
        }
    }
}

impl std::error::Error for NameValidationError {}

/// A validated profile name.
///
/// Profile names must be non-empty, at most 64 characters,
/// and contain only alphanumeric characters, hyphens, underscores, and spaces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileName(String);

impl ProfileName {
    /// Maximum allowed length for a profile name.
    pub const MAX_LEN: usize = 64;

    /// Create a new profile name, validating the input.
    pub fn new(name: impl Into<String>) -> Result<Self, NameValidationError> {
        let name = name.into();
        Self::validate(&name)?;
        Ok(Self(name))
    }

    /// Create a profile name without validation (for internal/trusted use).
    ///
    /// # Safety
    /// Caller must ensure the name is valid.
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    fn validate(name: &str) -> Result<(), NameValidationError> {
        if name.is_empty() {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::Empty,
                value: name.to_owned(),
            });
        }
        if name.len() > Self::MAX_LEN {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::TooLong,
                value: name.to_owned(),
            });
        }
        // Allow alphanumeric, spaces, hyphens, underscores
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
        {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::InvalidChars,
                value: name.to_owned(),
            });
        }
        Ok(())
    }

    /// Get the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ProfileName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A validated network name.
///
/// Network names follow the same rules as profile names since they often
/// represent the same concept (a connection profile/network identity).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetworkName(String);

impl NetworkName {
    /// Maximum allowed length for a network name.
    pub const MAX_LEN: usize = 64;

    /// Create a new network name, validating the input.
    pub fn new(name: impl Into<String>) -> Result<Self, NameValidationError> {
        let name = name.into();
        // Use the same validation as ProfileName
        ProfileName::validate(&name)?;
        Ok(Self(name))
    }

    /// Create a network name without validation (for internal/trusted use).
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Get the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for NetworkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for NetworkName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A validated IRC channel name.
///
/// Channel names must start with # or &, be non-empty (excluding prefix),
/// and contain no spaces or commas.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelName(String);

impl ChannelName {
    /// Maximum allowed length for a channel name (IRC limit is usually 50).
    pub const MAX_LEN: usize = 50;

    /// Create a new channel name, validating the input.
    pub fn new(name: impl Into<String>) -> Result<Self, NameValidationError> {
        let name = name.into();
        Self::validate(&name)?;
        Ok(Self(name))
    }

    /// Create a channel name without validation (for internal/trusted use).
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    fn validate(name: &str) -> Result<(), NameValidationError> {
        if name.is_empty() {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::Empty,
                value: name.to_owned(),
            });
        }
        if !name.starts_with('#') && !name.starts_with('&') {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::InvalidChannelPrefix,
                value: name.to_owned(),
            });
        }
        if name.len() > Self::MAX_LEN {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::TooLong,
                value: name.to_owned(),
            });
        }
        // Channel names cannot contain spaces, commas, or control characters
        if name[1..]
            .chars()
            .any(|c| c.is_whitespace() || c == ',' || c.is_control() || c == '\x07')
        {
            return Err(NameValidationError {
                kind: NameValidationErrorKind::InvalidChars,
                value: name.to_owned(),
            });
        }
        Ok(())
    }

    /// Get the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Check if a string looks like a channel name (starts with # or &).
    pub fn is_channel_like(s: &str) -> bool {
        s.starts_with('#') || s.starts_with('&')
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ChannelName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_name_valid() {
        assert!(ProfileName::new("default").is_ok());
        assert!(ProfileName::new("My Profile").is_ok());
        assert!(ProfileName::new("test-profile_123").is_ok());
    }

    #[test]
    fn profile_name_empty() {
        let result = ProfileName::new("");
        assert!(matches!(
            result,
            Err(NameValidationError {
                kind: NameValidationErrorKind::Empty,
                ..
            })
        ));
    }

    #[test]
    fn profile_name_too_long() {
        let long_name = "a".repeat(65);
        let result = ProfileName::new(long_name);
        assert!(matches!(
            result,
            Err(NameValidationError {
                kind: NameValidationErrorKind::TooLong,
                ..
            })
        ));
    }

    #[test]
    fn profile_name_invalid_chars() {
        let result = ProfileName::new("profile@server");
        assert!(matches!(
            result,
            Err(NameValidationError {
                kind: NameValidationErrorKind::InvalidChars,
                ..
            })
        ));
    }

    #[test]
    fn channel_name_valid() {
        assert!(ChannelName::new("#rust").is_ok());
        assert!(ChannelName::new("&local").is_ok());
        assert!(ChannelName::new("#channel-name").is_ok());
    }

    #[test]
    fn channel_name_no_prefix() {
        let result = ChannelName::new("nochannel");
        assert!(matches!(
            result,
            Err(NameValidationError {
                kind: NameValidationErrorKind::InvalidChannelPrefix,
                ..
            })
        ));
    }

    #[test]
    fn channel_name_with_space() {
        let result = ChannelName::new("#my channel");
        assert!(matches!(
            result,
            Err(NameValidationError {
                kind: NameValidationErrorKind::InvalidChars,
                ..
            })
        ));
    }

    #[test]
    fn channel_is_channel_like() {
        assert!(ChannelName::is_channel_like("#test"));
        assert!(ChannelName::is_channel_like("&local"));
        assert!(!ChannelName::is_channel_like("nickname"));
    }
}
