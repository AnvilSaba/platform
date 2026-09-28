# Discord の親 Message なしの公開 Thread と作成通知

調査日: 2026-09-06。対象は Discord の現行公式 API 資料と、ローカルの Serenity 固定 checkout `37b9f43` である。

## 結論

管理補足スレッドは、親を初期実装では `GUILD_TEXT` に限定し、`POST /channels/{parent_id}/threads`（Start Thread without Message）へ `type: PUBLIC_THREAD`（`11`）を**明示**して作成する。成功応答の Channel object の `id` を thread ID として保存する。[Start Thread without Message](https://docs.discord.com/developers/resources/channel#start-thread-without-message)・[Channel Types](https://docs.discord.com/developers/resources/channel#channel-object-channel-types)

同 endpoint は既存 Message に接続しない thread を作成し、`type` を指定できる。省略時だけ現在は `PRIVATE_THREAD` だが、将来の API 版では `type` は既定値なしの必須フィールドになるため、公開用途では省略しない。[Start Thread without Message](https://docs.discord.com/developers/resources/channel#start-thread-without-message)

## type 18 通知の削除

Message Resource は、type `18` (`THREAD_CREATED`) を「古い Message から、**または Message なしで**作成された public thread」の自動 Message と明記し、`deletable = true` としている。[Thread Created messages](https://docs.discord.com/developers/resources/message#message-reference-content-attribution)・[Message Types](https://docs.discord.com/developers/resources/message#message-object-message-types)

削除対象は、親チャンネルで受信した次の Message に限定する。

1. `message.type == 18`
2. `message.channel_id == parent_id`
3. `message.message_reference.channel_id == created_thread_id`
4. `message_reference.guild_id == guild_id`

`Message.channel_id` は「その Message が送信された channel」の ID である。一方 type `18` の attribution における `message_reference.channel_id` と `guild_id` は作成された thread channel を指す。保存先と参照先を区別する。[Message Object](https://docs.discord.com/developers/resources/message#message-object)・[Message Reference Structure](https://docs.discord.com/developers/resources/message#message-reference-structure)・[Thread Created messages](https://docs.discord.com/developers/resources/message#message-reference-content-attribution)

削除は `DELETE /channels/{parent_id}/messages/{system_message_id}` を用いる。自動 Message を bot 自身の投稿と仮定しないため、bot には `MANAGE_MESSAGES` を付与して結合テストする。[Delete Message](https://docs.discord.com/developers/resources/message#delete-message) 作成後は `MESSAGE_CREATE` を上記条件で照合し、再接続・配信順の揺れに備えて親 Message history でも同じ照合を行う。後者は API 仕様を組み合わせた実装上の補完である。

## 資料の不一致と採用解釈

Threads 概説には public thread は既存 Message から作るとの一般説明がある。[Public & Private Threads](https://docs.discord.com/developers/topics/threads#public-private-threads) しかし操作別の Channels Resource は standalone endpoint に `type` を定義し、Message Resource は public standalone の type `18` を明記する。今回の可否・通知処理には、具体的な endpoint と Message の一次仕様を優先する。これは同一の入出力条件を直接否定し合う実質的矛盾ではなく、概説が standalone public の場合を表し切れていない見かけ上の矛盾として扱う。

## 適用範囲・権限・未確認事項

初期対応は `GUILD_TEXT` のみとする。Forum/Media は専用 endpoint で thread と最初の Message を同時作成するため、本要件に合わない。[Start Thread in Forum or Media Channel](https://docs.discord.com/developers/resources/channel#start-thread-in-forum-or-media-channel) `GUILD_ANNOUNCEMENT` の standalone endpoint が必ず失敗することを直接示す資料は確認できなかったため、「API 上不可能」とは断定せず、設計上の対象外とする。

公開作成、thread 内送信、thread 操作にはそれぞれ `CREATE_PUBLIC_THREADS`、`SEND_MESSAGES_IN_THREADS`、`MANAGE_THREADS` が対応する。[Permissions](https://docs.discord.com/developers/topics/permissions) 親の閲覧権限・上書きまで含む最終的な作成／削除可否は、開発 guild で確認する。

固定 Serenity の実装: [create_thread.rs](https://github.com/serenity-rs/serenity/blob/37b9f433ada8b9ccc5f93f04826403b175855f86/src/builder/create_thread.rs) `CreateThread::kind` は public/private を受け、standalone の未指定を private とするが明示指定を推奨する。`message_id: None` は standalone 作成経路へ分岐する。ソースコメントの「public は Message を要する」は上記一般概説と同じであり、Discord の操作別一次仕様より優先しない。

`https://discord.com/channels/{guild_id}/{thread_id}` はクライアントで一般に用いられる形式だが、今回の公式 API 資料に URL 仕様の明記はない。実表示は導入前に検証する。実 Guild の権限上書き、作成通知の取得・削除、リンク実表示は実装後の結合テストで検証する。
