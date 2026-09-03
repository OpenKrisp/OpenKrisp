use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Validation failure for a stable route, device, or endpoint identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdentifierError {
    #[error("identifier must not be empty")]
    Empty,
    #[error("identifier must not contain control characters")]
    ContainsControlCharacter,
}

fn validate(value: &str) -> Result<(), IdentifierError> {
    if value.trim().is_empty() {
        return Err(IdentifierError::Empty);
    }
    if value.chars().any(char::is_control) {
        return Err(IdentifierError::ContainsControlCharacter);
    }
    Ok(())
}

macro_rules! string_identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated identifier.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate(&value)?;
                Ok(Self(value))
            }

            /// Returns the identifier as a borrowed string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Returns the owned identifier string.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentifierError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdentifierError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

string_identifier!(
    RouteId,
    "Stable identifier for one independently managed route."
);
string_identifier!(
    DeviceId,
    "Backend-qualified identifier for a physical audio device."
);
string_identifier!(
    LoopbackEndpointId,
    "Identifier for a platform-provided system loopback source."
);
string_identifier!(
    ApplicationStreamId,
    "Identifier for an application-owned PCM source."
);
string_identifier!(
    VirtualEndpointId,
    "Identifier for a platform virtual audio endpoint."
);
string_identifier!(
    ApplicationSinkId,
    "Identifier for an application-owned PCM destination."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_empty_and_control_characters() {
        assert_eq!(RouteId::new("  "), Err(IdentifierError::Empty));
        assert_eq!(
            DeviceId::new("coreaudio\ninput"),
            Err(IdentifierError::ContainsControlCharacter)
        );
    }

    #[test]
    fn identifiers_preserve_backend_values() {
        let id = DeviceId::new("coreaudio:BuiltIn-Input").expect("valid test identifier");
        assert_eq!(id.as_str(), "coreaudio:BuiltIn-Input");
        assert_eq!(id.to_string(), "coreaudio:BuiltIn-Input");
    }
}
