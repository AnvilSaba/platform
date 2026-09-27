use crate::{AppResult, identity::Name, invalid};
use sha1::{Digest, Sha1};
use std::time::Duration;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub(crate) struct SessionProfile {
    pub(crate) id: Uuid,
    pub(crate) name: Name,
}

pub(crate) fn authenticate(name: &str, secret: &[u8; 16], public_key: &[u8]) -> AppResult<SessionProfile> {
    let mut hash_source = Vec::with_capacity(16 + public_key.len());
    hash_source.extend_from_slice(secret);
    hash_source.extend_from_slice(public_key);
    let hash = signed_sha1(&hash_source);
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .get("https://sessionserver.mojang.com/session/minecraft/hasJoined")
        .query(&[("username", name), ("serverId", hash.as_str())])
        .send()?;
    if !response.status().is_success() {
        return Err(invalid("Mojang session verification failed"));
    }
    let profile: SessionProfile = response.json()?;
    Ok(profile)
}

pub(crate) fn signed_sha1(source: &[u8]) -> String {
    let mut digest: [u8; 20] = Sha1::digest(source).into();
    let negative = digest[0] & 0x80 != 0;
    if negative {
        for byte in &mut digest {
            *byte = !*byte;
        }
        for byte in digest.iter_mut().rev() {
            let (value, overflow) = byte.overflowing_add(1);
            *byte = value;
            if !overflow {
                break;
            }
        }
    }
    let first = digest.iter().position(|byte| *byte != 0).unwrap_or(digest.len() - 1);
    let mut result = digest[first..]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    result = result.trim_start_matches('0').to_owned();
    if negative { format!("-{result}") } else { result }
}
