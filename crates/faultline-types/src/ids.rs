use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

define_id!(FactionId);
define_id!(RegionId);
define_id!(InfraId);
define_id!(ForceId);
define_id!(TechCardId);
define_id!(EventId);
define_id!(VictoryId);
define_id!(InstitutionId);
define_id!(SegmentId);
define_id!(KillChainId);
define_id!(PhaseId);
define_id!(DomainId);
define_id!(DefenderRoleId);
