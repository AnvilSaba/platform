use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    marker::PhantomData,
};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
};
use validator::{Validate, ValidationError};

use super::{AttributeChange, ManagementError, RoleSnapshot, SCHEMA_VERSION};
use crate::features::discord_management::ids::{
    ChannelId, ChannelLogicalId, ChannelSettingsSetId, GuildId, MemberId, MemberLogicalId, RoleId, RoleLogicalId,
    RoleSettingsSetId,
};


mod channel;
mod definition;
mod role;
mod state;

pub(super) use channel::*;
pub(super) use definition::*;
pub use role::Color;
pub(super) use role::*;
pub(super) use state::*;

mod input;

pub(super) use input::PlanInput;
