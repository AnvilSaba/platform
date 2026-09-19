# Discord 管理スキーマ調査メモ

調査日: 2026-09-06（Asia/Tokyo）  
範囲: TOML の array-of-tables 構文、Taplo の TOML→JSON Schema 接続と対応 draft、Discord API のメンション通知抑制と Guild channel の型変更。  
検索語: `TOML array of tables`, `Taplo schema directive JSON Schema Draft 4`, `Discord allowed_mentions suppress notifications`, `Discord Modify Channel type conversion`。

## 結論

- `[[message_sets.guidelines.body]]` は正しい TOML 構文であり、各要素に `id` と複数行文字列の `body` を置ける。TOML の配列テーブルは同じ二重角括弧ヘッダーを繰り返すたびに配列へ新しいテーブル要素を追加し、出現順を保持する。したがって、例えば次の形は `message_sets.guidelines.body` を「オブジェクトの配列」として表す。

  ```toml
  [[message_sets.guidelines.body]]
  id = "basic"
  body = '''
  # 基本ルール
  互いを尊重してください。
  '''

  [[message_sets.guidelines.body]]
  id = "questions"
  body = '''質問について'''
  ```

  TOML の構文自体はキー名を `id` / `body` に限定しないため、必須性・型・重複 `id`・未知キーを制約するのは JSON Schema またはアプリケーション検証の責務である。[TOML 1.1.0 §Array of Tables](https://toml.io/en/v1.1.0#array-of-tables)（公開版 1.1.0、2025-01-11）

- Taplo の文書先頭に `#:schema ./foo-schema.json` または URL を置いて、文書単位の JSON Schema を指定できる。相対パスは TOML 文書ファイル基準で、同一文書に複数の schema directive を置く動作は未定義。Taplo のスキーマ検証で公式に明記されている対応は JSON Schema Draft 4 であり、Draft 7/2019-09/2020-12 対応は今回の一次資料から確認できなかった。`$schema` をルートの URL として置く方法も優先順位付きの割当方法として文書化されている。[Taplo Directives](https://taplo.tamasfe.dev/configuration/directives.html#the-schema-directive)、[Taplo Validation](https://taplo.tamasfe.dev/cli/usage/validation.html#schema-validation)、[Taplo Using Schemas](https://taplo.tamasfe.dev/configuration/using-schemas.html)（いずれも 2026-09-06 閲覧）

- メンションを含む本文で `allowed_mentions = { parse = [] }` 相当を送ると、`@everyone`、ロール、ユーザーの全メンション解析を抑制できる。`parse` は `roles` / `users` などの ID リストと相互排他的で、本文に見えるメンションがなければ許可リストだけで通知されることもない。これはメンションの通知抑制であり、通常のメッセージに付随するプッシュ通知全般を止める設定ではない。[Discord Message Resource — Allowed Mentions](https://docs.discord.com/developers/resources/message#allowed-mentions-object)

- プッシュ通知そのものを止める必要がある場合、Create Message の `flags` に `SUPPRESS_NOTIFICATIONS` を設定するとプッシュを無効化し、通知バッジだけにできる。従って「メンションを発火させない」は `allowed_mentions.parse=[]`、「プッシュを送らない」まで要求するなら `SUPPRESS_NOTIFICATIONS` も別途指定する、という二つの制御として扱う。[Discord Message Resource — Allowed Mentions](https://docs.discord.com/developers/resources/message#allowed-mentions-object)、[Discord Create Message](https://docs.discord.com/developers/resources/message#create-message)

- Guild channel の Modify Channel における `type` 変更は、現行公式仕様では Text (`GUILD_TEXT`, 0) と Announcement (`GUILD_ANNOUNCEMENT`, 5) の相互変換だけで、かつ Guild に `NEWS` feature が必要。Guild channel の更新には `MANAGE_CHANNELS` が必要である。Forum、Media、Voice、Stage、Category、Thread などへの一般的な型変換はこの API の記載から支持されないため、型変更を初期版でエラーにする方針は仕様に整合する。[Discord Channels Resource — Modify Channel](https://docs.discord.com/developers/resources/channel#modify-channel)、[Discord Channel Types](https://docs.discord.com/developers/resources/channel#channel-object-channel-types)

## 不明点・適用上の注意

- TOML 仕様は「配列要素がテーブルであること」と順序を規定するが、`id` の論理的一意性や `body` の Markdown 解釈は規定しない。そこは本プロジェクトのスキーマ／検証規則として定義する必要がある。
- Taplo の現行一次資料は Draft 4 対応を明記するだけで、より新しい JSON Schema draft を使えるかは確定できない。新 draft のキーワードを schema に採用する前に、固定する Taplo バージョンの実動作またはソースコードで再確認が必要である。
- `allowed_mentions` の指定は Discord ユーザー側の通知設定や権限の影響を受ける。`SUPPRESS_NOTIFICATIONS` もプッシュ停止を保証する設定として文書化されているが、通知バッジは残る。
- 上記は公式仕様の適用可能範囲の確認であり、Serenity 等の固定ライブラリが各フィールドを同じ名前・型で公開しているかはこの調査では確認していない。
