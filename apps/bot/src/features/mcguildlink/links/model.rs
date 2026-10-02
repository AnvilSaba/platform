use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Link {
    pub discord_user_id: String,
    pub discord_name: String,
    pub minecraft_uuid: Uuid,
    pub minecraft_name: String,
    pub linked_at: DateTime<Utc>,
}
