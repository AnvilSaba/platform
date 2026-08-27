# MCGuildLink


## 使い方
### 必要な権限
#### 自動ブロック
メンバーが BAN された際に自動でブロックし以降の紐付けを禁止する機能を使用するには、Botのロール自体に以下の権限が必要です。
- `Permission.BanMembers`: `メンバーをBAN`

#### コマンド
`/create_panel` コマンドを使用するには、以下の権限が必要です。

現状それ以外で使用する予定はないので、パネルを設置するチャンネルでのみ Bot のロールへと権限を与えることを推奨します。

- `Permission.ViewChannel`: `チャンネルを見る`
- `Permission.SendMessages`: `メッセージを送信`

#### ログ
ログを送るチャンネルでは、Bot に以下の権限が必要です。

- `Permission.ViewChannel`: `チャンネルを見る`
- `Permission.SendMessages`: `メッセージを送信`
