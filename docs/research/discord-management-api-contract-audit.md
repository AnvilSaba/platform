# Discord 管理設計: API 契約監査

参照日: 2026-09-06。対象は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2) に統合された定義案、[エディタ用スキーマ](../schemas/discord-management.schema.json) と、`Cargo.lock` で固定された Serenity `0.12.5`（commit `37b9f433ada8b9ccc5f93f04826403b175855f86`）である。本書は実 API を操作せず、Discord 公式資料とこの固定ソースだけを突合した。

## 判定

草案が列挙する Role と 7 種類の Guild channel（Category、Text、Announcement、Voice、Stage、Forum、Media）の管理は、型別の意味検証を設ければ概ね実装可能である。ただし、現在の schema が属性を型ごとに絞っていないため、少なくとも以下を実装前に修正または意味検証で拒否する必要がある。

|確定事実|草案・schema への影響|
|---|---|
|`default_thread_slowmode_seconds` は Text / Forum / Media 用であり、Announcement には使えない。|Announcement の「対応する Thread 既定設定」は `default_auto_archive_minutes` に限定する。|
|`default_forum_layout` は Forum 専用で、Media には使えない。|Media の同属性を拒否する。|
|`default_auto_archive_minutes` の許可値は `60, 1440, 4320, 10080` 分である。|監査時の `minimum: 0` は不十分。エディタ用スキーマは列挙値を制約する。型別の意味検証も必要。|
|Forum / Media の `available_tags` は更新時に配列全体を送る属性であり、最大 20 個。|「管理外タグを落とさない」には取得済み全タグを含めた全配列再送が必要。宣言だけを送って部分更新することはできない。|

根拠は [Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel) の型別属性表および [Channel Object](https://docs.discord.com/developers/resources/channel#channel-object) である。固定 Serenity の `CreateChannel` / `EditChannel` も同じ API フィールドを持つ（[create_channel.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_channel.rs)、[edit_channel.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs)）。

## Channel と Role

### 確定事実

- Channel 作成には `MANAGE_CHANNELS`、overwrite 変更には追加で `MANAGE_ROLES` が必要である。Bot が allow/deny できるのは自身が持つ権限の範囲である。[Create Guild Channel](https://docs.discord.com/developers/resources/guild#create-guild-channel)、[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)
- `parent_id: null` はカテゴリからの切離し、`rtc_region: null` は自動リージョン、`topic: null` はトピック解除、`default_reaction_emoji: null` と `default_sort_order: null` はそれぞれの未設定化を表せる。`user_limit: 0` は無制限である。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)、[Channel Object](https://docs.discord.com/developers/resources/channel#channel-object)
- overwrite の allow / deny を `null` または省略してもビットが `0` になるだけで、overwrite 自体は消えない。対象 overwrite の解除には `DELETE /channels/{channel.id}/permissions/{overwrite.id}` が必要である。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)、[Delete Channel Permission](https://docs.discord.com/developers/resources/channel#delete-channel-permission)
- Channel の種類変更は Text と Announcement 間だけで、Guild に `NEWS` feature が必要である。草案の「全種類変更禁止」は API より狭いが、安全な設計判断として整合する。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)
- Role は名前、権限ビット集合、色、hoist、mentionable を作成・更新でき、位置変更は別 endpoint である。作成時既定値は name=`new role`、permissions=everyone の Guild 権限、hoist=false、mentionable=false である。`color` は非推奨で `colors` が推奨される。[Create Guild Role](https://docs.discord.com/developers/resources/guild#create-guild-role)、[Modify Guild Role](https://docs.discord.com/developers/resources/guild#modify-guild-role)、[Modify Guild Role Positions](https://docs.discord.com/developers/resources/guild#modify-guild-role-positions)

### 推奨

- `{ default = true }` は「Discord が作成時に採る既定値を明示送信する」ものではなく、ツール側が型別の具体値へ解決して送るものに限る。作成で必須の `name` に対し `{ default = true }` を許すと、既存では読み取り値へ戻せても新規作成では意味を持たない。Channel の API は作成時に name を必須とする。[Create Guild Channel](https://docs.discord.com/developers/resources/guild#create-guild-channel)
- `clear` は API の null、空値、専用 DELETE が異なることを型別に解決する。今回の権限ごとの `clear` は、指定した権限ビットだけを allow / deny の両方から除く。他の権限ビットは維持する。単一ビットの clear を対象 overwrite 全体の DELETE に変換してはならない。最終的に allow / deny がともに0なら、空の overwrite の整理として DELETE を検討できる。
- 権限ビットの「省略は維持」を実現するには、現在の Role permissions と overwrite allow/deny を取得してからビットを合成する。Role の `permissions` は一つの bitset、Channel の `permission_overwrites` は配列全体の属性であるため、部分 PATCH として扱えない。
- API の既定値・Guild feature・boost tier で変わる制約は plan 時に現在値と Guild を取得して解決する。例えば Voice bitrate 上限は boost tier により 96k / 128k / 256k / 384k、Stage は 64k である。[Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)

### 固定 Serenity の差分

- 高水準 builder で `parent_id` と `rtc_region` の null は表現できる。`EditChannel::category(None)` と `voice_region(None)` が該当する。[edit_channel.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs#L113-L120)
- 一方で `EditChannel` は `topic` の null と `default_sort_order` の null を builder で表せない。`topic("")` は空文字であり null ではない。固定 Serenity の公開 `Http::edit_channel` は任意の `Serialize` payload を受けるため、独自の小さな request struct を使えば API の null は送れる。[edit_channel.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_channel.rs)、[client.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/http/client.rs#L1387-L1405)
- `CreateForumTag` は ID を保持しない。管理外 tag を保ったまま個別 tag を更新するには、API から取得した tag ID を含む独自 payload が必要である。これは「ライブラリ非対応」ではなく、高水準 builder だけでは不足するという差分である。[create_forum_tag.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_forum_tag.rs)

## Thread の API 経路と更新制約

独立した管理スレッドの作成・通知処理は [専用調査](discord-standalone-thread-research.md) を参照。以下の Message 起点の API は比較対象であり、Feature Spec の採用経路ではない。

### 確定事実

- 親 Message から作る経路は `GUILD_TEXT → PUBLIC_THREAD` と `GUILD_ANNOUNCEMENT → ANNOUNCEMENT_THREAD` だけである。Forum / Media ではこの endpoint は使えない。Forum / Media は「thread と最初の message を同時に作る」専用 endpoint を使い、親 Message から補足 thread を作る設計には使えない。[Start Thread from Message](https://docs.discord.com/developers/resources/channel#start-thread-from-message)、[Start Thread in Forum or Media Channel](https://docs.discord.com/developers/resources/channel#start-thread-in-forum-or-media-channel)
- Message 起点 thread の ID は親 Message ID と同一であり、一つの Message から作れる thread は一つだけである。親 Message を削除しても public thread は orphaned として残り得る。[Start Thread from Message](https://docs.discord.com/developers/resources/channel#start-thread-from-message)、[Threads](https://docs.discord.com/developers/topics/threads#public--private-threads)
- archived thread は原則として編集、reaction、application command、join ができず、削除 message だけが例外である。message の送信は自動 unarchive を試みるが、locked thread では行わない。active thread 上限に達すれば新規作成・unarchive・member 追加も失敗する。[Threads](https://docs.discord.com/developers/topics/threads#active--archived-threads)、[Channel Object](https://docs.discord.com/developers/resources/channel#channel-object)
- thread の削除には `MANAGE_THREADS` が必要である。unarchive は unlocked なら参加済み利用者、locked なら作成者または `MANAGE_THREADS` が必要である。名前・archived・auto archive の変更は作成者または `MANAGE_THREADS`、slowmode と locked の変更は `MANAGE_THREADS` が必要である。[Threads](https://docs.discord.com/developers/topics/threads#editing--deleting-threads)

### 更新時の適用上の注意

本文更新前に Thread metadata を取得し、必要なら archived を false にしてから作成・編集する。固定 Serenity は状態用の [EditThread](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/edit_thread.rs) を提供する。locked、権限、active thread 上限を含めた動作はテスト Guild で確認する。

## Components V2、preview、Interaction

### 確定事実

- Components V2 は message flag `IS_COMPONENTS_V2` (`1 << 15`) で有効化する。一度付けると edit で外せない。従来の `content` / `embeds` は使えず、attachment は component で公開する。message 全体の component 数は最大 40 である。[Component Reference](https://docs.discord.com/developers/components/reference#what-is-a-component)、[Using Message Components](https://docs.discord.com/developers/components/using-message-components)
- Interaction は受信後 3 秒以内に初期 response が必要で、超過すると token は無効になる。token は 15 分間有効で、その間だけ original response と followup の作成・編集・削除ができる。[Interaction Callback](https://docs.discord.com/developers/interactions/receiving-and-responding#interaction-callback)、[Followup Messages](https://docs.discord.com/developers/interactions/receiving-and-responding#followup-messages)
- followup も `EPHEMERAL` を個別設定できる。初回を ephemeral の deferred response にした直後の followup POST は、互換動作として new message ではなく original loading response の edit になり、ephemeral 指定は defer 時の値が維持される。この動作は非推奨である。[Create Followup Message](https://docs.discord.com/developers/interactions/receiving-and-responding#create-followup-message)
- followup の 5 件上限は「user-installed かつ server 未導入」の Interaction に限る。Guild に導入された Bot の preview を一律 5 件に制限する根拠ではない。[Create Followup Message](https://docs.discord.com/developers/interactions/receiving-and-responding#create-followup-message)

### 推奨

- 草案の preview が複数 message を必要とする場合、3 秒以内に ephemeral defer し、まず original response を `PATCH` で完成させ、その後は `POST followup` で追加する。defer 直後の `POST followup` を「2 件目」と数えない。
- 実処理開始は、15 分 token のうち preview を返す時間と結果を返す時間を残す締切より前に止める。15 分を越える apply の成功結果を Interaction token だけで返す設計にはしない。
- 40 個は入れ子を含む message 全体の上限として事前に数える。本文を Text Display に移す場合は、従来の message content 2,000 文字とは別の Component Reference の属性制約も検証する。現草案の「文字数・コンポーネント数を超えたら入力エラー」は維持できる。

固定 Serenity は V2 flag と `TextDisplay`、`Container`、`MediaGallery`、`File`、`Separator` を含む `CreateComponent`、Interaction response と followup の `ephemeral` / `components` を提供する。従って V2 自体と複数 ephemeral message は実装可能である。固定ソースが API の「40」の上限を数えて拒否することまでは確認できないため、Bot 側で数える必要がある。[create_components.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_components.rs)、[create_interaction_response.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_interaction_response.rs)、[create_interaction_response_followup.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_interaction_response_followup.rs)。

## 資料間の見かけ上の相違

Serenity の一部 builder コメントは Topic や Video quality の対象型を公式 API 表より狭く書く箇所がある。固定ライブラリが JSON field を送れることと、Discord がその型で受理することは別問題である。本監査では、受理型・値域・権限は更新日のある Discord 公式 API 表を採用し、Serenity は payload を構成できるかだけを判定した。Media channel は公式が「active development」と明記しているため、実装前の test Guild 検証対象として残す。[Channel Object](https://docs.discord.com/developers/resources/channel#channel-object)
