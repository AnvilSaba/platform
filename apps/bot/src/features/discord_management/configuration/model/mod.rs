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

use super::{ManagementError, SCHEMA_VERSION};
use crate::features::discord_management::ids::{
    ChannelId, ChannelLogicalId, ChannelSettingsSetId, GuildId, MemberId, MemberLogicalId, RoleId, RoleLogicalId,
    RoleSettingsSetId,
};

mod channel;
mod definition;
mod permission;
mod role;
mod state;

pub(crate) use channel::*;
pub(crate) use definition::*;
pub(crate) use permission::*;
pub use role::Color;
pub(crate) use role::*;
pub(crate) use state::*;

mod input;

pub(crate) use input::PlanInput;
