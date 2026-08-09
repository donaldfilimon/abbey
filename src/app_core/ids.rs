//! Canonical opaque identifiers shared by application contracts.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! string_uuid_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Generate a new opaque identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            /// Return the canonical lower-case UUID representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let parsed = Uuid::parse_str(value).map_err(|_| IdError)?;
                if parsed.to_string() != value {
                    return Err(IdError);
                }
                Ok(Self(value.to_owned()))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value
                    .parse()
                    .map_err(|_| de::Error::custom("expected a canonical UUID"))
            }
        }
    };
}

string_uuid_id!(RunId, "Identifier for one application run.");
string_uuid_id!(
    ConversationId,
    "Identifier for a conversation spanning one or more runs."
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdError;

impl fmt::Display for IdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier must be a canonical lower-case UUID")
    }
}

impl std::error::Error for IdError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip_as_canonical_strings() {
        let run = RunId::new();
        let conversation = ConversationId::new();
        assert_eq!(run.to_string().parse::<RunId>().unwrap(), run);
        assert_eq!(
            conversation.to_string().parse::<ConversationId>().unwrap(),
            conversation
        );
        assert!("NOT-A-UUID".parse::<RunId>().is_err());
        assert!(
            "550E8400-E29B-41D4-A716-446655440000"
                .parse::<RunId>()
                .is_err()
        );
    }
}
