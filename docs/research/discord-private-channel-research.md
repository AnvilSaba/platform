# Discord の通常 Guild Channel と private 設定（一次資料調査）

調査日: 2026-09-06。公式資料に基づく記録であり、実 Guild の権限設定は未検証。

## 結論

- 通常の Guild Channel（`GUILD_TEXT` など）に、チャンネル種別とは別の「private フラグ」がある、という仕様は公式 API 資料には記載されていない。公式の Channel Types では `GUILD_TEXT` はサーバー内テキストチャンネルとして定義され、private と明示される別種別は `PRIVATE_THREAD`（type 12）である。[Channels Resource: Channel Types](https://docs.discord.com/developers/resources/channel#channel-object-channel-types)
- 通常チャンネルを特定のロールだけに見せる公式の方法は、チャンネル単位の permission overwrite（権限上書き）を設定すること。Discord 公式ガイドも、private channel では `@everyone` の `View Channel` をオフにし、対象ロールへ明示的にオンを付ける手順を示している。[How to Create a Community For My Game](https://docs.discord.com/developers/game-development/how-to-create-a-community-for-your-game#permissions)

## `@everyone` の `VIEW_CHANNEL` deny + ロール allow の意味

`VIEW_CHANNEL` はチャンネルの表示（テキストチャンネルではメッセージの閲覧を含む）を許可する権限である。チャンネル overwrite の評価では、まず `@everyone` の deny、次に特定ロールの deny、続いて特定ロールの allow が適用される。したがって、対象ロールを持つメンバーには、そのロールの `VIEW_CHANNEL` allow が効き、対象ロールを持たないメンバーは、ほかの Role / Member の明示 allow や管理者の例外がなければ閲覧できない。[Permissions: Bitwise Permission Flags / Permission Overwrites](https://docs.discord.com/developers/topics/permissions#permission-overwrites)

`VIEW_CHANNEL` を deny された場合、テキストチャンネル上の他の権限も暗黙に利用できない（見えないため）。これは `SEND_MESSAGES` などを個別に全て deny することを意味するのではなく、閲覧不可により実際の利用が制限されるという仕様である。[Permissions: Implicit Permissions](https://docs.discord.com/developers/topics/permissions#implicit-permissions)

## Owner / Administrator の例外

公式の権限計算例では、Guild owner は base permissions が `ALL` になり、`ADMINISTRATOR` を持つユーザーも `ALL` として扱われる。さらに `ADMINISTRATOR` は「全権限を許可し、チャンネル権限 overwrite を迂回する」と定義されている。したがって、`@everyone` deny + ロール allow による private 化は、Guild owner や `ADMINISTRATOR` 保持者を隠すアクセス制御にはならない。[Permissions: Permission Hierarchy](https://docs.discord.com/developers/topics/permissions#permission-hierarchy)

## Private Channel と Private Thread の違い

| 項目 | 通常 Guild Channel を private 化 | Private Thread |
|---|---|---|
| 実体 | `GUILD_TEXT` 等の通常チャンネル。チャンネル単位の permission overwrite で可視性を制御 | Channel Type `PRIVATE_THREAD`（type 12）。テキストチャンネル内の一時的なサブチャンネルで、招待された人と `MANAGE_THREADS` 保持者だけが閲覧可能 |
| 参加者の決め方 | ロール／メンバーの `VIEW_CHANNEL` overwrite | 作成時にユーザーまたはロールをメンションして招待。後からメンションで追加可能 |
| 必要権限・性質 | `MANAGE_ROLES` がチャンネル overwrite 編集に必要（API上の要件） | 作成には `CREATE_PRIVATE_THREADS`、管理者は `MANAGE_THREADS` で private thread を閲覧可能。スレッドは非アクティブ後に自動クローズされ得る |
| 親チャンネルとの関係 | 独立した通常チャンネル（カテゴリの permission syncing の影響はあり得る） | 親チャンネルの権限を継承する。`VIEW_CHANNEL` が必要で、直接招待・追加されても親チャンネルを見られない人は閲覧できない |

根拠: [Channels Resource: Channel Types](https://docs.discord.com/developers/resources/channel#channel-object-channel-types)、[Permissions: Inherited Permissions (Threads)](https://docs.discord.com/developers/topics/permissions#inherited-permissions)、[Threads FAQ: Server Permissions for Threads / Private Threads](https://support.discord.com/hc/en-us/articles/4403205878423-threads-faq#server-permissions-for-threads)

## 注意点・未確認事項

「private channel」という UI 上の呼称は公式ガイドにも登場するが、通常 Guild Channel のオブジェクトに `private` boolean があるという API フィールドは確認できなかった。ここでの「フラグがない」は、確認した現行公式 Channel Object / Channel Types 資料に基づく結論であり、クライアント UI 内部の未公開状態については判断しない。
