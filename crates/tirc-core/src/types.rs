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
                write!(
                    f,
                    "channel name must start with #, &, +, or !: '{}'",
                    self.value
                )
            }
        }
    }
}

impl std::error::Error for NameValidationError {}

fn validate_profile_like_name(name: &str, max_len: usize) -> Result<(), NameValidationError> {
    if name.is_empty() {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::Empty,
            value: name.to_owned(),
        });
    }
    if name.len() > max_len {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::TooLong,
            value: name.to_owned(),
        });
    }
    // Allow alphanumeric, spaces, hyphens, underscores.
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

fn is_valid_channel_prefix(c: char) -> bool {
    matches!(c, '#' | '&' | '+' | '!')
}

fn is_forbidden_channel_char(c: char) -> bool {
    // RFC 2812 forbids NUL, BELL, CR, LF, SPACE, COMMA, and COLON.
    matches!(c, '\0' | '\x07' | '\r' | '\n' | ' ' | ',' | ':') || c.is_control()
}

fn validate_channel_name(name: &str, max_len: usize) -> Result<(), NameValidationError> {
    if name.is_empty() {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::Empty,
            value: name.to_owned(),
        });
    }

    let mut chars = name.chars();
    let Some(prefix) = chars.next() else {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::Empty,
            value: name.to_owned(),
        });
    };

    if !is_valid_channel_prefix(prefix) {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::InvalidChannelPrefix,
            value: name.to_owned(),
        });
    }

    let rest = chars.as_str();
    if rest.is_empty() {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::Empty,
            value: name.to_owned(),
        });
    }

    if name.len() > max_len {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::TooLong,
            value: name.to_owned(),
        });
    }

    if rest.chars().any(is_forbidden_channel_char) {
        return Err(NameValidationError {
            kind: NameValidationErrorKind::InvalidChars,
            value: name.to_owned(),
        });
    }

    Ok(())
}

/// A validated profile name.
///
/// Profile names must be non-empty, at most 64 characters,
/// and contain only alphanumeric characters, hyphens, underscores, and spaces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileName(String);

impl ProfileName {
    /// Maximum allowed length for a profile name.
    pub const MAX_LEN: usize = 64;

    /// Create a new profile name, validating the input.
    pub fn new(name: &str) -> Result<Self, NameValidationError> {
        Self::validate(name)?;
        Ok(Self(name.to_owned()))
    }

    /// Create a profile name without validation (for internal/trusted use).
    ///
    /// Caller is responsible for ensuring the value is valid.
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    fn validate(name: &str) -> Result<(), NameValidationError> {
        validate_profile_like_name(name, Self::MAX_LEN)
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

impl TryFrom<String> for ProfileName {
    type Error = NameValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl TryFrom<&str> for ProfileName {
    type Error = NameValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProfileName> for String {
    fn from(value: ProfileName) -> Self {
        value.0
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
/// Network names use the same allowed character set as profile names,
/// but maintain independent constants and constructors.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NetworkName(String);

impl NetworkName {
    /// Maximum allowed length for a network name.
    pub const MAX_LEN: usize = 64;

    /// Create a new network name, validating the input.
    pub fn new(name: &str) -> Result<Self, NameValidationError> {
        Self::validate(name)?;
        Ok(Self(name.to_owned()))
    }

    /// Create a network name without validation (for internal/trusted use).
    ///
    /// Caller is responsible for ensuring the value is valid.
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    fn validate(name: &str) -> Result<(), NameValidationError> {
        validate_profile_like_name(name, Self::MAX_LEN)
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

impl TryFrom<String> for NetworkName {
    type Error = NameValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl TryFrom<&str> for NetworkName {
    type Error = NameValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NetworkName> for String {
    fn from(value: NetworkName) -> Self {
        value.0
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
/// Channel names must start with `#`, `&`, `+`, or `!`, have at least one
/// character after the prefix, and may not contain forbidden RFC 2812 chars.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ChannelName(String);

impl ChannelName {
    /// Maximum allowed length for a channel name (IRC limit is usually 50).
    pub const MAX_LEN: usize = 50;

    /// Create a new channel name, validating the input.
    pub fn new(name: &str) -> Result<Self, NameValidationError> {
        Self::validate(name)?;
        Ok(Self(name.to_owned()))
    }

    /// Create a channel name without validation (for internal/trusted use).
    ///
    /// Caller is responsible for ensuring the value is valid.
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    fn validate(name: &str) -> Result<(), NameValidationError> {
        validate_channel_name(name, Self::MAX_LEN)
    }

    /// Get the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Check if a string looks like a channel name (starts with `#`, `&`, `+`, or `!`).
    pub fn is_channel_like(s: &str) -> bool {
        matches!(s.chars().next(), Some('#' | '&' | '+' | '!'))
    }
}

impl TryFrom<String> for ChannelName {
    type Error = NameValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl TryFrom<&str> for ChannelName {
    type Error = NameValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ChannelName> for String {
    fn from(value: ChannelName) -> Self {
        value.0
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
        let result = ProfileName::new(&long_name);
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
    fn network_name_valid() {
        assert!(NetworkName::new("freenode").is_ok());
    }

    #[test]
    fn network_name_invalid_chars() {
        let result = NetworkName::new("bad/network");
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
        assert!(ChannelName::new("+modeless").is_ok());
        assert!(ChannelName::new("!safechan").is_ok());
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
    fn channel_name_with_colon() {
        let result = ChannelName::new("#my:channel");
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
        assert!(ChannelName::is_channel_like("+modeless"));
        assert!(ChannelName::is_channel_like("!safechan"));
        assert!(!ChannelName::is_channel_like("nickname"));
    }

    #[test]
    fn serde_rejects_invalid_profile_name() {
        let result: Result<ProfileName, _> = serde_json::from_str("\"bad@name\"");
        assert!(result.is_err());
    }

    #[test]
    fn serde_rejects_invalid_channel_name() {
        let result: Result<ChannelName, _> = serde_json::from_str("\"no-prefix\"");
        assert!(result.is_err());
    }

    #[test]
    fn serde_round_trip_profile_name() {
        let profile = ProfileName::new("default").unwrap();
        let json = serde_json::to_string(&profile).unwrap();
        assert_eq!(json, "\"default\"");

        let decoded: ProfileName = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, profile);
    }

    #[test]
    fn try_from_impls_work() {
        let profile = ProfileName::try_from("default").unwrap();
        assert_eq!(profile.as_str(), "default");

        let network = NetworkName::try_from(String::from("my-net")).unwrap();
        assert_eq!(network.as_str(), "my-net");

        let channel = ChannelName::try_from("#rust").unwrap();
        assert_eq!(channel.as_str(), "#rust");
    }
}
