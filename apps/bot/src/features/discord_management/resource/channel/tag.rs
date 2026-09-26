use crate::features::discord_management::{
    configuration::{StateFile, TagAttributes, TagDefinition, TagEmojiUpdate, TagResult},
    domain::ManagementError,
    ids::{ChannelLogicalId, TagLogicalId},
    port::{ForumTagAttributes, ForumTagSnapshot, ForumTagWrite},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TagPlan {
    pub(super) payload: Vec<ForumTagWrite>,
    pub(super) released: Vec<TagLogicalId>,
    pub(super) settled: Vec<TagLogicalId>,
    pub(super) deleted: Vec<TagLogicalId>,
    /// 新規 Tag の payload 内位置。応答の同じ位置の ID を採用します。
    pub(super) created: BTreeMap<TagLogicalId, usize>,
    pub(super) changed: Vec<TagLogicalId>,
    pub(super) write: bool,
}
impl TagPlan {
    pub(super) fn between(
        channel: &ChannelLogicalId,
        declarations: &BTreeMap<TagLogicalId, TagDefinition>,
        state: &StateFile,
        actual: &[ForumTagSnapshot],
    ) -> Result<Option<Self>, ManagementError> {
        let empty = BTreeMap::new();
        let mappings = state.tags.get(channel).unwrap_or(&empty);
        let mut plan = Self {
            payload: actual.iter().map(ForumTagWrite::from).collect(),
            ..Default::default()
        };
        if let Some(results) = state.tag_results.get(channel) {
            for (id, result) in results {
                if *result != TagResult::ResponseUnknown {
                    continue;
                }
                if mappings.contains_key(id) {
                    plan.settled.push(id.clone());
                } else if !declarations.contains_key(id) {
                    plan.released.push(id.clone());
                } else {
                    return Err(ManagementError::InvalidState(format!(
                        "Tag {channel}/{id} の作成結果は不明です。Discord ID を確認して bind してください"
                    )));
                }
            }
        }
        for (logical_id, id) in mappings {
            let declaration = declarations.get(logical_id);
            if declaration.is_none() {
                plan.released.push(logical_id.clone());
                continue;
            }
            let declaration = declaration.unwrap();
            if declaration.is_absent() {
                plan.payload.retain(|tag| tag.id() != Some(*id));
                plan.deleted.push(logical_id.clone());
                continue;
            }
            if !actual.iter().any(|tag| tag.id == *id) {
                return Err(ManagementError::InvalidState(format!(
                    "Tag {channel}/{logical_id} の ID {id} が親 Channel に存在しません"
                )));
            }
        }
        for (logical_id, declaration) in declarations {
            let attributes = match declaration {
                TagDefinition::Absent => continue,
                TagDefinition::Reference => {
                    if !mappings.contains_key(logical_id) {
                        return Err(ManagementError::InvalidState(format!(
                            "参照専用 Tag {channel}/{logical_id} の対応がありません"
                        )));
                    }
                    continue;
                }
                TagDefinition::Managed { attributes } => attributes,
            };
            if let Some(id) = mappings.get(logical_id) {
                let tag = plan
                    .payload
                    .iter_mut()
                    .find(|tag| tag.id() == Some(*id))
                    .expect("Tag の存在は検証済みです");
                let before = tag.clone();
                update_tag(tag.attributes_mut(), attributes);
                if *tag != before {
                    plan.changed.push(logical_id.clone());
                }
            } else {
                let Some(name) = &attributes.name else {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "新規 Tag {channel}/{logical_id} には name が必要です"
                    )));
                };
                let mut tag = ForumTagAttributes {
                    name: name.clone().into_inner(),
                    moderated: false,
                    emoji: None,
                };
                update_tag(&mut tag, attributes);
                plan.created.insert(logical_id.clone(), plan.payload.len());
                plan.payload.push(ForumTagWrite::Create(tag));
            }
        }
        if plan.payload.len() > 20 {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {channel} の管理外 Tag を含む全 Tag は20個以内で指定してください"
            )));
        }
        plan.write = plan.payload != actual.iter().map(ForumTagWrite::from).collect::<Vec<_>>();
        Ok(
            (plan.write || !plan.released.is_empty() || !plan.deleted.is_empty() || !plan.settled.is_empty())
                .then_some(plan),
        )
    }
    pub(super) fn unknown(&self, channel: &ChannelLogicalId, state: &mut StateFile) {
        let results = state.tag_results.entry(channel.clone()).or_default();
        for id in self.created.keys().chain(&self.changed).chain(&self.deleted) {
            results.insert(id.clone(), TagResult::ResponseUnknown);
        }
        if results.is_empty() {
            state.tag_results.remove(channel);
        }
    }
    pub(super) fn acknowledge_deletions(&self, channel: &ChannelLogicalId, state: &mut StateFile) {
        for id in &self.deleted {
            if let Some(mappings) = state.tags.get_mut(channel) {
                mappings.remove(id);
            }
            state
                .tag_results
                .entry(channel.clone())
                .or_default()
                .insert(id.clone(), TagResult::Deleted);
        }
    }
    pub(super) fn finish(
        &self,
        channel: &ChannelLogicalId,
        state: &mut StateFile,
        actual: &[ForumTagSnapshot],
    ) -> Result<(), ManagementError> {
        // 名前は一意ではないため、ID と応答配列の位置の両方を検証します。
        if self.write
            && (actual.len() != self.payload.len()
                || !actual.iter().zip(&self.payload).all(|(got, want)| want.matches(got)))
        {
            return Err(ManagementError::InvalidState(
                "Tag 更新後の配列が希望値と一致しません".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        if actual.iter().any(|tag| tag.id.get() == 0 || !ids.insert(tag.id)) {
            return Err(ManagementError::InvalidState(
                "Tag 更新応答の ID が不正または重複しています".into(),
            ));
        }
        let mut mappings = state.tags.get(channel).cloned().unwrap_or_default();
        for logical_id in self.released.iter().chain(&self.deleted) {
            mappings.remove(logical_id);
        }
        for (logical_id, index) in &self.created {
            let id = actual[*index].id;
            if id.get() == 0 || mappings.values().any(|existing| *existing == id) {
                return Err(ManagementError::InvalidState(
                    "作成 Tag ID が既存対応と衝突しています".into(),
                ));
            }
            mappings.insert(logical_id.clone(), id);
        }
        if mappings.is_empty() {
            state.tags.remove(channel);
        } else {
            state.tags.insert(channel.clone(), mappings);
        }
        let results = state.tag_results.entry(channel.clone()).or_default();
        for id in self
            .created
            .keys()
            .chain(&self.changed)
            .chain(&self.released)
            .chain(&self.settled)
        {
            results.remove(id);
        }
        for id in &self.deleted {
            results.insert(id.clone(), TagResult::Deleted);
        }
        if results.is_empty() {
            state.tag_results.remove(channel);
        }
        Ok(())
    }
}
fn update_tag(tag: &mut ForumTagAttributes, attributes: &TagAttributes) {
    if let Some(name) = &attributes.name {
        tag.name = name.clone().into_inner();
    }
    if let Some(moderated) = attributes.moderated {
        tag.moderated = moderated;
    }
    if let Some(emoji) = &attributes.emoji {
        tag.emoji = match emoji {
            TagEmojiUpdate::Set(value) => Some(value.clone().into_string()),
            TagEmojiUpdate::Clear => None,
        };
    }
}
