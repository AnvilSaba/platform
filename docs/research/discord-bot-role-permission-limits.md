# Discord Botのロール権限管理に関する調査

調査日: 2026-09-13  
対象: Discord公式Developer Documentation（取得表示上の更新時期は約3〜4か月前）

## 結論

Botが自身の実効権限に含まない権限ビットを、他のロールへ付与することはできません。Discord公式の権限仕様に明記されています。したがって、このプロジェクトはplan段階で、通常ロールについて「付与しようとする有効化権限がBotの実効権限に含まれるか」を検査し、含まれない場合は事前拒否する方針が妥当です。

ただし、Botが `ADMINISTRATOR` を持つ場合は全権限を持つ扱いになるため、付与可能ビットの検査では全ビットを許可する扱いにできます。`ADMINISTRATOR` はチャンネル権限上書きも迂回しますが、公式資料はロール編集の階層制限を解除するとまでは説明していません。したがって、`ADMINISTRATOR` を持っていても対象ロールがBotの最高位ロールより下か、という階層検査は別途維持すべきです。

## 根拠

### Botに必要な操作権限

公式のGuild API仕様では、以下のロール操作に `MANAGE_ROLES` が必要です。

- ロールの作成
- ロール位置の変更
- ロールの変更
- ロールの削除
- メンバーへのロール追加・削除

出典: [Guild Resource - Discord Developer Documentation](https://docs.discord.com/developers/resources/guild)

公式の権限仕様では、`MANAGE_ROLES` は「ロールの管理・編集」を許可する権限です。

出典: [Permissions - Discord Developer Documentation](https://docs.discord.com/developers/topics/permissions)

### 権限ビットの付与制限

公式の「Permission Hierarchy」には、Botは自身の最高位ロールより下のロールを編集できるが、そのロールへ付与できるのは「Bot自身が持つ権限だけ」と明記されています。ここでいう「持つ」は、単なるOAuth2招待時の要求値ではなく、Guild内でBotユーザーに対して計算された実効権限と解釈するのが自然です。

同じ制限はDiscord公式サポート資料でも、Manage Rolesを持つメンバーは自身が持っている権限だけを他のロールへ割り当てられ、例えば自身にBan Membersがなければ他者へ付与できない、と説明されています。

出典:

- [Permissions - Permission Hierarchy](https://docs.discord.com/developers/topics/permissions#permission-hierarchy)
- [Discord Roles and Permissions - Discord Support](https://support.discord.com/hc/en-us/articles/214836687-Discord-Roles-and-Permissions)

### ロール階層

公式仕様では、Botが編集できるのはBotの最高位ロールより低い位置のロールだけです。また、ロールの同順位はID順でソートされます。従って、plan時の対象ロール判定は、独自に `position` とIDを組み合わせるのではなく、使用ライブラリ（本プロジェクトではSerenity）がDiscordの比較規則を実装した比較を利用するべきです。

出典: [Permissions - Permission Hierarchy / Role Object](https://docs.discord.com/developers/topics/permissions)

### `ADMINISTRATOR`

公式の権限一覧では、`ADMINISTRATOR` は「すべての権限を許可し、チャンネル権限上書きを迂回する」と定義されています。よって、Botの実効権限にこのビットが含まれる場合、権限ビットの不足検査では全権限を保有すると扱えます。

一方、同じ公式ページはロール編集について「Bot自身が持つ権限だけを付与できる」と説明し、`ADMINISTRATOR` 保有時にロール位置の階層制限まで無効になるとは記載していません。階層制限を独自に免除する根拠は確認できませんでした。

出典: [Permissions - Bitwise Permission Flags / Permission Hierarchy](https://docs.discord.com/developers/topics/permissions)

### APIエラー

Discord APIはHTTP 403を「Authorization tokenにリソースへの権限がない」と定義し、JSONエラーコード `50013` を「その操作を実行する権限がない」と定義しています。Botが `MANAGE_ROLES` を持たない場合、または対象ロールの階層・付与する権限の制約に違反する場合は、API呼び出しが成功する前提にせず、planで拒否できる条件を先に検査するのが適切です。

ただし、公式のエラーコード一覧は `50013` を一般的な権限不足として定義しているだけで、「不足した権限ビットの付与」と「階層違反」を常に別コードで返すとは記載していません。

出典: [Opcodes and Status Codes - HTTP Response Codes / JSON Error Codes](https://docs.discord.com/developers/topics/opcodes-and-status-codes)

## 本プロジェクトへの判断

plan時に次を検査するのが望ましいです。

1. Botの実効権限に `MANAGE_ROLES` があること。
2. 通常ロールの対象がBotの最高位ロールより下であること。
3. 付与する権限ビットが、Botの実効権限に含まれること。
4. Botが `ADMINISTRATOR` を持つ場合は、3のビット不足検査を全許可として扱うこと。
5. `@everyone` は通常ロールの階層・管理対象として扱わず、既存方針どおり権限だけを管理すること。`@everyone` のIDはGuild IDと同じです。

3をplan時に拒否する利点は、APIに依存した遅い失敗や部分適用を防ぎ、定義ファイルの誤りを実行前に明示できることです。実行時にも権限が変更される可能性があるため、APIの403/`50013`は引き続き通常の適用エラーとして処理する必要があります。

## 調査範囲・検索語・未確認事項

確認した一次資料はDiscord Developer DocumentationのPermissions、Guild Resource、Opcodes and Status Codes、およびDiscord SupportのRoles and Permissionsです。検索語は `Discord bot role permissions hierarchy Manage Roles`, `bot can only grant permissions it has`, `Discord API 50013 Missing Permissions`, `ADMINISTRATOR role edit hierarchy` です。

未確認事項は、特定APIバージョン・特定SDKが、権限不足ビットを含むロール更新リクエストに対して必ずどのHTTPエラー本文を返すかです。公式の一般エラー仕様から403/`50013`までは確認できますが、細かな失敗理由の分類までは確認できませんでした。そこを厳密に確定する必要がある場合は `research_terra_medium` 以上で再調査が必要です。
