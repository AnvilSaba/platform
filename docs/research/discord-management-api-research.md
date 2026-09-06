# Discord 構成・管理メッセージ: API とライブラリの事前調査

適用範囲・動作の正本は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2)。本書は調査時点の API・固定ライブラリの事実を記録する。型別の制約は [契約監査](discord-management-api-contract-audit.md)、独立 Thread は [専用調査](discord-standalone-thread-research.md) も参照。

調査日: 2026-09-05。Discord の公式 Developer Documentation と、リポジトリで固定されている Serenity `37b9f433ada8b9ccc5f93f04826403b175855f86`、Poise `b189f7c1cfd47eb4e7863683a49c389a9702ce23` のソースを照合した。これは実装ではなく、仕様を確定するための根拠メモである。

## Discord API の管理可能範囲

以下は 2026-09-05 時点の Modify Channel の型別表を基にしたもの。各行の永続属性は API が read/modify できるものだけであり、`last_message_id`、thread の活動時刻等の観測値は desired state に入れない。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)、[Channel Object](https://docs.discord.com/developers/resources/channel#channel-object)。

|型|管理候補|制約・設計判断|
|---|---|---|
|Category (`GUILD_CATEGORY`)|name、order、permission overwrites、nsfw|親は持てない。カテゴリ削除は子を削除せず親を外すため、削除 plan では子の処理を先に明示する。[Delete Channel](https://docs.discord.com/developers/resources/channel#deleteclose-channel)|
|Text (`GUILD_TEXT`)|name、parent、order、topic、nsfw、slowmode、default auto archive、default thread slowmode、flags、overwrites|`topic` は最大 1024、slowmode は 0–21600 秒。|
|Announcement (`GUILD_ANNOUNCEMENT`)|Text と同等（default thread slowmode を除く）|Text↔Announcement の型変換だけが可能で、Guild `NEWS` feature が必要。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)|
|Voice (`GUILD_VOICE`)|name、parent、order、nsfw、slowmode、bitrate、user limit、RTC region、video quality、flags、overwrites|bitrate は boost/VIP feature により上限が変わる（通常 96k、boost 1/2/3 または VIP で 128k/256k/384k）。user limit は最大 99。|
|Stage (`GUILD_STAGE_VOICE`)|Voice と同等（flags は対象外）|bitrate 上限は 64k、user limit は最大 10,000。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)|
|Forum (`GUILD_FORUM`)|Text系に加え、available tags、default reaction emoji、default sort order、forum layout、`REQUIRE_TAG` flag|topic 最大 4096。tag は最大 20。`default_thread_rate_limit_per_user` は新規 thread にコピーされ、既存 thread を追随更新しない。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)|
|Media (`GUILD_MEDIA`)|Forum とほぼ同じ。ただし `default_forum_layout` は対象外、media 限定 flag あり|公式は Media を「active development」で、文書化された機能以外は変更されうると明記する。[Channel Types](https://docs.discord.com/developers/resources/channel#channel-object-channel-types)。利用条件と型別の受理値は実 Guild での検証が必要。|

`GUILD_MEDIA` を含む channel type、Forum/Media tag・default reaction・sort order は公式 API の Modify Channel 対象である。[型と flags](https://docs.discord.com/developers/resources/channel#channel-object-channel-types)、[Forum Tag](https://docs.discord.com/developers/resources/channel#forum-tag-object)。ただし guild feature や bot 権限が不足する場合、同一 config をすべての Guild に無条件再現できない。

### Channel / Role の順序

同じ position の Channel / Role は ID によって順序が決まるため、整数の position だけでは希望する相対順序を一意に表せない。位置更新は全体の並びを読み取って専用 endpoint へ渡す必要がある。[Channel Object](https://docs.discord.com/developers/resources/channel#channel-object)、[Role Positions](https://docs.discord.com/developers/resources/guild#modify-guild-role-positions)。

### Role と overwrite

- Role は `name`、`permissions`（文字列の bitset）、`colors`、`hoist`、`mentionable` を create/modify でき、position は別 endpoint で更新する。[Create/Modify Role](https://docs.discord.com/developers/resources/guild#create-guild-role)、[Role positions](https://docs.discord.com/developers/resources/guild#modify-guild-role-positions)。`color` は返るが deprecated で、リクエストは `colors` が推奨される。
- `@everyone` は Guild ID と同じ Role ID で、任意 Role と同じ create/delete 対象ではない。[Guild role の例](https://docs.discord.com/developers/resources/guild#get-guild)。
- overwrite は channel の一部として全配列を desired state と比較する。`type: 0` は role、`type: 1` は member、allow/deny は bitset である。[Overwrite Object](https://docs.discord.com/developers/resources/channel#overwrite-object)。
- overwrite の書込みには `MANAGE_ROLES`、channel 更新には `MANAGE_CHANNELS` が必要である。bot は原則として自分が Guild/親 category で持つ権限しか allow/deny できない。[Edit Channel Permissions](https://docs.discord.com/developers/resources/channel#edit-channel-permissions)、[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)。ロール階層より上の role は変更できないことも plan の事前検証に含める。
- effective permission は `@everyone` deny/allow、role deny/allow、member deny/allowの順に評価される。よって plan は bitset の差分を属性として表示しつつ、effective permission の変化を推測して「安全」と表示してはならない。[Permission Overwrites](https://docs.discord.com/developers/topics/permissions#permission-overwrites)。

### Thread の取得・活動状態の制約

Thread は作成者、archive/lock、membership、活動によって変動する運用リソースで、Guild create でも bot が閲覧できる active thread だけが返る。上限到達時には作成/展開も失敗する。[Threads](https://docs.discord.com/developers/topics/threads)、[Channel thread 注記](https://docs.discord.com/developers/resources/channel#channel-object)。一般 Thread の取得範囲と、明示的に作成・管理する Thread の対応を区別する。

## Components V2 と Managed Message

|宣言要素|公式 API の表現と制約|固定 Serenity での確認|
|---|---|---|
|Container|type 17、子は Action Row/Text Display/Section/Media Gallery/Separator/File、accent color と spoiler を持つ。[Component Reference](https://docs.discord.com/developers/components/reference#container)|対応。`CreateContainer` と `CreateContainerComponent` に全子型がある。[create_components.rs#L573-L676](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs#L573-L676)|
|Text Display|type 10、V2 message の markdown テキスト。[Component Reference](https://docs.discord.com/developers/components/reference#text-display)|対応。`CreateTextDisplay`。[create_components.rs#L69-L104](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs#L69-L104)|
|Section|type 9、子 1–3 個、accessory は Button または Thumbnail。[Component Reference](https://docs.discord.com/developers/components/reference#section)|対応。Section child は現 rev では `CreateSectionComponent::TextDisplay` のみで、accessory は `Thumbnail`/`Button`。[create_components.rs#L121-L265](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs#L121-L265)|
|Separator|type 14、divider と small/large spacing。[Component Reference](https://docs.discord.com/developers/components/reference#separator)|対応。`CreateSeparator`。既存 helper もある。[components.rs](../../apps/bot/src/app/utils/components.rs)|
|Thumbnail|Section accessory 専用。外部 URL または attachment、画像のみ（動画不可）。[Component Reference](https://docs.discord.com/developers/components/reference#thumbnail)|対応。`CreateThumbnail`/`CreateUnfurledMediaItem`。既存 helper もある。[components.rs](../../apps/bot/src/app/utils/components.rs)|
|Media Gallery|top-level または Container child、1–10件、URL/attachment、alt text/spoiler。[Component Reference](https://docs.discord.com/developers/components/reference#media-gallery)|対応。`CreateMediaGallery`/`CreateMediaGalleryItem`。[create_components.rs#L360-L460](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs#L360-L460)|
|File|top-level または Container child。`attachment://filename` のみで、同じ送信に attachment upload が必要。[Component Reference](https://docs.discord.com/developers/components/reference#file)|対応。`CreateFile` は同形式のみを許す。[create_components.rs#L463-L527](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs#L463-L527)|
|Button|Action Row 内か Section accessory。リンク button は interaction を発生させず、非リンクは `custom_id` を必要とする。[Component Reference](https://docs.discord.com/developers/components/reference#button)|対応。`CreateButton`。[create_components.rs#L1220-L1358](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs#L1220-L1358)|

固定 Serenity は V2 の全列挙型を送信 builder として実装済みで、現リポジトリにも `IS_COMPONENTS_V2` を付ける `create_components_v2_message` と Container/Section/Thumbnail helper がある。[utils.rs](../../apps/bot/src/utils.rs)、[components.rs](../../apps/bot/src/app/utils/components.rs)。

### Message の表示順と ID 維持は両立しない場合がある

Message edit は既存 message の `components` を編集でき、Message Object は送信時刻と edit 時刻を別に保持する。[Edit Message](https://docs.discord.com/developers/resources/message#edit-message)、[Message Object](https://docs.discord.com/developers/resources/message#message-object)。しかし channel 内の任意位置へ既存 message を移動する API はない。そのため、既存の `a`, `c` の間に source 上 `b` を挿入する場合、`a` と `c` の Discord Message ID を保存したまま表示順を `a`, `b`, `c` にすることはできない（`b` は新規投稿なので末尾になる）。

表示順を優先する場合は再投稿によってリンク・pin・reaction を失う。採用理由は [ADR 0006](../adr/0006-managed-message-boundaries-and-order.md) に記録する。message の読取りには `VIEW_CHANNEL` と `READ_MESSAGE_HISTORY` が必要であり、state にある個別 ID を GET することで、履歴全走査なしに存在と author/channel を確認できる。[Get Channel Message](https://docs.discord.com/developers/resources/message#get-channel-message)。

## 冪等性と rate limit

Message create の `nonce` と `enforce_nonce` は同一送信者について過去数分の重複作成を抑止するが、永続的な対応表の代替にはならない。[Create Message](https://docs.discord.com/developers/resources/message#message-create-request-body)。API 操作と state 返却は一つのトランザクションにできない。

- Discord は route ごとと global の rate limit を持ち、数値を hard-code せず header/bucket と 429 の `retry_after` に従うよう要求している。[Rate Limits](https://docs.discord.com/developers/topics/rate-limits)。固定 Serenity の `Ratelimiter` は route bucket を保持して pre-emptive wait と 429 retry を実装し、HTTP client では既定で有効である。[ratelimiting.rs#L68-L85](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/http/ratelimiting.rs#L68-L85)、[client.rs#L133-L145](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/http/client.rs#L133-L145)。ただし複数 process が同一 token で同時 apply すると各 process の limiter/state lock は共有されない。プロセス内の排他だけでは複数プロセス間の実行を排他できない。

## Bot / Poise への含め方

確認済み: `apps/bot` は Poise `FrameworkOptions { commands: commands(), ... }` を Serenity Client に渡す Gateway bot であり、config は `config.toml` の `AppConfig`、依存は `BotData` に組み立てられる。[main.rs](../../apps/bot/src/main.rs)、[config.rs](../../apps/bot/src/app/config.rs)、[data.rs](../../apps/bot/src/app/data.rs)。`bot-macros` は event handler/error handler の trait 実装を生成するだけで、command/HTTP 管理機能を提供しない。[lib.rs](../../crates/bot-macros/src/lib.rs)。

確認済み: このリポジトリには command の明示登録呼出しがない。固定 Poise の `Framework` は command list を dispatch 用に保持するだけで、登録は `poise::builtins::register_globally` / `register_in_guild` を明示して初めて行う。[Poise framework](https://github.com/serenity-rs/poise/blob/b189f7c1cfd47eb4e7863683a49c389a9702ce23/src/framework/mod.rs)、[Poise register](https://github.com/serenity-rs/poise/blob/b189f7c1cfd47eb4e7863683a49c389a9702ce23/src/builtins/register.rs)。したがって構成管理を slash command に載せても command registration の問題は解決しない。


## 添付と未検証事項

File の編集では既存 attachment を維持して参照できるため、常に再アップロードが必要とは限らない。期限付き attachment URL は永続的な画像の正本にできない。[Edit Message](https://docs.discord.com/developers/resources/message#edit-message)。型別の送受信、Media の利用条件、実表示は実 API で未検証。
