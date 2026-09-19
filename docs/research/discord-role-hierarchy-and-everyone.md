# Discord Role 階層と `@everyone` の調査

調査日: 2026-09-13 (Asia/Tokyo)

## 結論

- `position` は一意な順位ではない。Discord の Role Object は「同じ position の Role は id でソートされる」と明記しているため、`position == highest_position` のときに ID を比較する必要がある。[Discord Permissions / Role Structure](https://docs.discord.com/developers/topics/permissions#role-structure)、[Discord Guild Resource / Modify Guild Role Positions](https://docs.discord.com/developers/resources/guild#modify-guild-role-positions)
- このリポジトリは独自の position/ID 式を持たず、Serenity の `Role` 比較（固定 revision の `Ord`）を利用する。同実装は「position が低いほど下、同位置では ID が大きいほど下」と扱う。ただし、同位置の ID の向きは Discord の本文が単に「id でソート」としか書いていないため、実装上の向きは固定した Serenity ソースで確認するのが根拠になる。[Serenity 37b9f433 `role.rs`](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/model/guild/role.rs)、固定 revision を指定する本リポジトリの [Cargo.lock](../../Cargo.lock:2204)
- 同じ位置になる理由は、Discord が position の重複・非連続を許容するためである。Discord API の一次資料側でも、Role positions endpoint の `position` は「同じ position は id でソート」と定義されている。[Discord API docs issue #1778（Discord 開発者コメントを含む）](https://github.com/discord/discord-api-docs/issues/1778)
- `@everyone` は通常の Role と異なり、Guild ID と同じ ID を持つ基底 Role であり、全メンバーに既定で付与される。Discord の権限計算は最初に `@everyone` の permissions を base permissions として読み、その後メンバーの他 Role の権限を OR する。[Discord Permissions / Permission Hierarchy](https://docs.discord.com/developers/topics/permissions#permission-hierarchy)、[Discord support: Setting Up Permissions FAQ](https://support.discord.com/hc/en-us/articles/206029707-Setting-Up-Permissions-FAQ)
- したがって管理モデルでは `@everyone` を通常 Role の属性更新対象から分離し、「権限だけ変更可能」と扱うのが妥当。Discord API の Modify Guild Role の一般パラメータは name/permissions/color/hoist/mentionable 等だが、公式仕様は `@everyone` の特殊な編集制約をこの表では列挙していないため、API が保証する制約として断定せず、アダプターで明示的に制限するべきである。[Modify Guild Role](https://docs.discord.com/developers/resources/guild#modify-guild-role)
- 権限は Discord API 上で bitwise value（bit set）として表現される。false は「その bit が立っていない」状態であり、各 Role の export に false を全件列挙することは API の必要条件ではない。[Discord Permissions / Bitwise Permission Flags](https://docs.discord.com/developers/topics/permissions#bitwise-permission-flags)

## リポジトリ実装との対応

- 通常 Role の階層判定は固定 Serenity の `Role::cmp` に委ねる。[adapter.rs](../../apps/bot/src/features/discord_management/adapter.rs)
- 現在の `role_catalog` は全 Role について `Permissions::all().iter_names()` を走査し、true/false を両方 `RoleSnapshot.permissions` に格納している。[adapter.rs:109](../../apps/bot/src/features/discord_management/adapter.rs:109)
- 一方、サービスの `build_role_update` は宣言された permission だけを `actual` に適用し、宣言されていない permission を保持する。[service.rs:547](../../apps/bot/src/features/discord_management/service.rs:547) これは Role の false を毎回出力しなくても差分適用できる根拠になる。
- `default_permissions` は `@everyone` の bitset から生成され、`Default` 指定の解決に使われる。[adapter.rs:113](../../apps/bot/src/features/discord_management/adapter.rs:113)、[service.rs:583](../../apps/bot/src/features/discord_management/service.rs:583)

## 推奨する仕様整理

1. 階層比較は独自に再実装せず、position と ID tie-break を実装済みの固定 Serenity `Role::cmp` に委ねる。
2. `@everyone` は `role.id == guild_id` で識別し、通常 Role の階層判定とは別に権限更新対象へ含める。更新 payload は permissions のみ許可し、name/color/hoist/mentionable は拒否する。
3. 定義/export の permissions は、`@everyone` では基底権限を明示する。それ以外の Role では true の権限だけを列挙し、false は省略する。省略時は「変更しない（現状保持）」と解釈し、`false` を明示した場合だけ bit を落とす。

## 不明点・注意

- Discord の現行公開 docs は「同位置は id でソート」と記すが、本文だけでは ID の昇順/降順を明示していない。ID の向きは Serenity 37b9f433 の `Role` `Ord` 実装と実際の API 応答で固定すべきである。
- Discord の Modify Guild Role ページには `@everyone` の「権限のみ編集可能」という明示的な表記を確認できなかった。これは Discord クライアント/サーバーの既知の特殊挙動としてアダプターで守る設計であり、API docs の明文根拠ではない。
- 以上の未確認点を仕様として厳密に確定する必要がある場合は、`research_terra_medium` 以上で再調査が必要。

## 調査範囲・検索語・資料版

- 範囲: Discord Developer Documentation の Permissions / Guild Resource、Discord Support の permissions FAQ、Serenity 固定 revision `37b9f433ada8b9ccc5f93f04826403b175855f86`、本リポジトリの Discord management 実装。
- 検索語: `Discord role position same position sorted by id`, `Modify Guild Role @everyone permissions`, `Serenity Role Ord position id`, `@everyone base permissions`。
- Discord docs: 2026-09-13 に取得（サイト表示上のクロール時期は 2026 年）。Serenity: 本リポジトリの `Cargo.lock` が固定する revision 37b9f43。
