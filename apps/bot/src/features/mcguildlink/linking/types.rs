use nutype::nutype;

/// Discord の利用者 ID。DB 内の主キーとは区別する。
#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, Hash))]
pub struct DiscordUserId(u64);

/// mcguildlink.discord_accounts の主キー。
#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, Hash))]
pub struct DiscordAccountId(i64);

/// 発行済みの紐付けコード。
#[nutype(derive(Debug, Clone, PartialEq, Eq, Display, From, AsRef))]
pub struct LinkCode(String);
