# Discord Channel PATCH の省略・設定・明示解除: Serenity / Twilight 照合

調査日: 2026-09-16。対象は Discord の現行公式 REST 仕様、リポジトリが固定する Serenity `0.12.5`（commit `37b9f433ada8b9ccc5f93f04826403b175855f86`）、および Twilight `twilight-http` の現行 `0.17.1` である。調査結果は Channel adapter の型付き request と plan/apply の設計へ反映した。

## 結論

PATCH の「省略・値設定・`null`」は常に三状態を要求するわけではない。Discord 仕様上 nullable な属性だけが `null` を受理する。従って、プロジェクトの `ChannelUpdateValue::{Keep, Set, Clear}` は domain / plan 層では妥当だが、adapter はフィールドごとの API 契約に従い、`Clear` を `null`、`0`、または専用 DELETE のいずれに変換するかを区別しなければならない。[Discord: Nullable and Optional Resource Fields](https://docs.discord.com/developers/reference#nullable-and-optional-resource-fields)、[Discord: Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)

特に、`topic` は Modify Channel の nullable field なので、管理上の「topic なし」は `null` で表現する。空文字は topic が空の値であり、解除と同一視しない。一方、slowmode の解除は `rate_limit_per_user: 0` が公式に定義された通常の表現である。[Discord: Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)

公式 OpenAPI では `default_thread_rate_limit_per_user` も nullable と生成されるが、仕様リポジトリ自身が「ほぼ全 nullable field が optional、全 optional field が nullable になる」既知問題を明記している。endpoint 文書では同フィールドは `integer` であるため、OpenAPI の `null` だけを受理根拠にはしない。[Discord OpenAPI specification: Known issues](https://github.com/discord/discord-api-spec#known-issues)

## Discord API の正本

Discord は型の前に付く `?` を nullable、フィールド名末尾の `?` を optional と定義する。Modify Channel は「全パラメータ optional」であるため、フィールド非送信は変更なしを表す。したがって `?T` の PATCH は「非送信 / 値 / `null`」を、非 nullable の `T` は「非送信 / 値」を表す。[Discord: Nullable and Optional Resource Fields](https://docs.discord.com/developers/reference#nullable-and-optional-resource-fields)、[Discord: Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)

|属性|公式の型|API 上の更新表現|採用する解除表現|
|---|---|---|---|
|`topic`|`?string`、0–1024文字|省略 / string / `null`|管理上の解除は `null`|
|`parent_id`|`?snowflake`|省略 / category ID / `null`|`null`|
|`default_auto_archive_duration`|`?integer`|省略 / 許可分値 / `null`|`null`|
|`default_thread_rate_limit_per_user`|endpoint表は `integer`、公式OpenAPIは `integer | null`（0–21600）|省略 / 秒値。OpenAPI上は`null`も構成可能|リセットは両仕様に整合する `0` を採用|
|`rate_limit_per_user`|`?integer`、範囲 0–21600|省略 / 秒値 / `null`|無効化は `0` を設定|
|`permission_overwrites`|`?array`|省略 / 配列全置換 / `null`|完成形配列を一度の PATCH で設定し、1件削除も配列から除外して表現|

`permission_overwrites` の overwrite 要素内の `allow` / `deny` は省略または `null` なら `"0"` になるだけで、overwrite レコードは削除されない。全置換で送る場合は、削除対象を除いた完成形を配列として送信する。SDK が語彙化していない allow/deny bit は読み取り時に opaque mask として保持し、既知 bit の変更時も再送する。[Discord: Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel-json-params-guild-channel)

## Serenity 0.12.5（固定版）

このリポジトリは `apps/bot/Cargo.toml` で上記 commit を直接固定し、`Cargo.lock` でも `serenity 0.12.5` の同 commit を解決している。[apps/bot/Cargo.toml](../../apps/bot/Cargo.toml:34)、[Cargo.lock](../../Cargo.lock:2182)

`EditChannel` は `Serialize` derive と `skip_serializing_if = "Option::is_none"` を使う。よって外側の `None` はフィールドの省略である。[固定版 `EditChannel` の内部フィールド](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs#L33-L72)

|属性|公開 builder と内部型|表現できる状態|評価|
|---|---|---|---|
|`topic`|`topic(str)` / `Option<Cow<str>>`|省略、値|`null` 不可。空文字と `null` は別物|
|`parent_id`|`category(Option<ChannelId>)` / `Option<Option<ChannelId>>`|省略、値、`null`|`Option<Option<_>>` を公開せず、setter 引数に nullable 性を閉じ込めている|
|`default_auto_archive_duration`|`default_auto_archive_duration(AutoArchiveDuration)` / `Option<AutoArchiveDuration>`|省略、値|API は nullable だが builder は `null` 不可|
|`default_thread_rate_limit_per_user`|`default_thread_rate_limit_per_user(NonMaxU16)` / `Option<NonMaxU16>`|省略、値|仕様どおり `null` を送らない|
|`rate_limit_per_user`|`rate_limit_per_user(NonMaxU16)` / `Option<NonMaxU16>`|省略、値（`0` を含む）|slowmode 無効化は `0` を設定可能|
|`permission_overwrites`|`permissions([...])` / `Option<Cow<[PermissionOverwrite]>>`|省略、配列全置換（`[]` 可）|個別 DELETE には別 API を使う|

根拠となる setter と内部型は [固定版 `EditChannel`](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs#L33-L72) および [category / topic / slowmode / permissions](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs#L132-L240)、[default 属性](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs#L312-L343) で確認できる。

高水準 builder で足りない nullable 属性についても、固定 Serenity の `Http::edit_channel` は任意の `Serialize` payload を受ける公開 API である。従って raw `serde_json::Value` を domain へ流す必要はない。`topic` と `default_auto_archive_duration` の `null` だけを表す、小さな型付き request struct を adapter 内に置けばよい。[固定版 `Http::edit_channel`](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/http/client.rs#L1387-L1405)

## Twilight `twilight-http` 0.17.1（比較対象）

Twilight は `request::Nullable<T>` で「setter を呼ばない / `Some(value)` / `None`」を表す。公開 API に二重 `Option` を出さず、呼出し有無を外側の状態に、setter 引数の `Option<T>` を値か `null` に割り当てる設計である。[Twilight `UpdateChannel` 0.17.1](https://api.twilight.rs/twilight_http/request/channel/struct.UpdateChannel.html)、[現行 source: `UpdateChannelFields`](https://github.com/twilight-rs/twilight/blob/main/twilight-http/src/request/channel/update_channel.rs#L24-L69)

|属性|公開 API|表現できる状態|Serenity との差|
|---|---|---|---|
|`parent_id`|`parent_id(Option<Id<ChannelMarker>>)`|未呼出=省略、`Some`=設定、`None`=`null`|安全な3状態を setter で表す|
|`topic`|`topic(&str)`|省略、値|通常 Text / Announcement の `null` 不可。`forum_topic(Option<&str>)` は Forum / Media topic に限り `null` 可|
|`default_auto_archive_duration`|現行 `UpdateChannel` に setter なし|更新不可|API の対象属性を builder が追随していない|
|`default_thread_rate_limit_per_user`|`default_thread_rate_limit_per_user(Option<u16>)`|未呼出、値、`null` をシリアライズ可能|ただし Discord 仕様は非 nullable。`None` を有効な解除として採用しない|
|`rate_limit_per_user`|`rate_limit_per_user(u16)`|省略、値（`0` を含む）|slowmode を `0` で無効化|
|`permission_overwrites`|`permission_overwrites(&[PermissionOverwrite])`|省略、全置換（`[]` 可）|個別削除は `delete_channel_permission`|

Twilight の内部フィールドは `parent_id: Option<Nullable<...>>`、`default_thread_rate_limit_per_user: Option<Nullable<u16>>`、`topic: Option<&str>` であり、いずれも `Option::is_none` のときフィールドを省略する。各公開 setter は [現行 source](https://github.com/twilight-rs/twilight/blob/main/twilight-http/src/request/channel/update_channel.rs#L146-L320) と [topic / parent / overwrite / slowmode](https://github.com/twilight-rs/twilight/blob/main/twilight-http/src/request/channel/update_channel.rs#L244-L374) で確認できる。個別 overwrite 削除 request も公開されている。[Twilight `Client::delete_channel_permission`](https://docs.rs/twilight-http/latest/twilight_http/client/struct.Client.html#method.delete_channel_permission)

Twilight の `default_thread_rate_limit_per_user(Option<u16>)` と Discord 公式の非 nullable 契約は見かけ上矛盾する。前者は library が `null` payload を構築できることを示すだけで、Discord がそれを受理する根拠ではない。本件では API 所有者である Discord 仕様を優先する。

## 現プロジェクトへの推奨

1. `ChannelUpdateValue::{Keep, Set, Clear}` は port 境界の意図表現として維持する。ただし `Clear` を全属性に許すのではなく、仕様ごとに許可・変換を決める。現状の型は [port](../../apps/bot/src/features/discord_management/port/mod.rs:212) にあり、意図を JSON から分離できている。
2. Channel 更新は変更属性を一つの private な型付き PATCH body に集約し、`Http::edit_channel` を一度だけ呼ぶ。builder で表現できる属性とできない属性を別リクエストへ分割しない。`serde_json::Value` はこの型付き body に置換する。[固定版 `Http::edit_channel`](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/http/client.rs#L1387-L1405)
3. `topic` の管理上の解除は `null` とし、`parent_id` と `default_auto_archive_duration` の解除も同じ PATCH body に `null` として含める。domain / plan の三状態を adapter の nullable field へ変換する。
4. `default_thread_slowmode_seconds` の `Clear` は `0` へ変換する。公式OpenAPIは `null` も許す形だが nullable過剰生成の既知問題があり、endpoint表の `integer` とも整合する `0` が安全なリセット表現である。[Discord OpenAPI specification](https://github.com/discord/discord-api-spec)、[現行 adapter](../../apps/bot/src/features/discord_management/adapter/serenity/channel.rs:388)
5. overwrite 全体の同期は private な typed `EditChannel::permissions` payload（空配列を含む）で行う。plan は常に削除後の完成形配列を作り、adapter は一回の `Http::edit_channel` に全属性とともに渡す。snapshot から得た未知 allow/deny bit は opaque な型で完成形まで保持する。[現行 adapter](../../apps/bot/src/features/discord_management/adapter/serenity/channel.rs:521)

## 未解決点と採用判断

- Discord の nullable 表記は `default_auto_archive_duration` の `null` を許すが、実 Guild の権限・channel type・Guild feature による実行受理は別である。既存の型別検証を維持し、統合テスト Guild で Clear 経路を確認する。
- `default_thread_rate_limit_per_user` の Twilight builder と公式OpenAPIは `null` を構成できる。一方、endpoint表は `integer` で、OpenAPIにはnullable過剰生成の既知問題がある。このためリセットには `0` を採用する。
- `rate_limit_per_user` は nullable 表記である一方、無効化の値 `0` が公式に定義される。管理ドメインの「解除」は `null` でなく `Set(0)` とする。
