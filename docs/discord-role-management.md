# Discord Role の export と plan

Bot 所有者は Discord の Interaction から、管理可能な Role の現在値を export し、編集した定義と Guild 固有の state を再投入して属性単位の plan を確認できます。現在の実装は Role の読み取りと plan だけを行い、Discord の Role は変更しません。

## テスト Guild へのコマンド登録

開発専用 token と `config.toml` を用意し、リポジトリルートで次を実行します。

```powershell
cargo run --package bot --locked -- --register-guild <TEST_GUILD_ID>
```

この操作は、Role 管理コマンドだけでなく `features::commands()` にある既存コマンドを含む全コマンドを、指定した Guild のこの Bot application 用コマンドとして登録して終了します。本番 Guild への登録はこの手順では行わないでください。

Bot をテスト Guild に導入する際は `applications.commands` と `bot` scope を使い、少なくとも `View Channels` と `Manage Roles` を付与します。Bot が扱えるのは Bot 自身の最上位 Role より下にある、Discord integration に管理されていない Role だけです。

## 操作手順

1. `/role_export` を state 添付なしで実行し、`discord-roles.toml` と `discord-state.json` を保存します。
2. TOML を編集します。現時点で対応する構造は [サンプル](examples/discord-role-management.toml) を参照してください。
3. `/role_plan` に編集した TOML と、同じ export で得た state JSON を添付します。
4. `discord-role-plan.txt` で現在値と希望値を確認します。

再 export では `/role_export` に既存 state を添付します。state に対応済みの Role は論理 ID を維持し、結果は別の TOML/JSON 添付として返ります。state は Guild ごとに分けて保管してください。別 Guild の state、未対応の形式版、未知キー、未知の権限、参照先不足、論理 ID や Snowflake の衝突は Interaction の操作結果として表示されます。
