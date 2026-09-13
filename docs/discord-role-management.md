# Discord Role の export と plan

Bot 所有者は Discord の Interaction から、管理可能な Role の現在値を export し、編集した定義と Guild 固有の state を再投入して属性単位の plan を確認・適用できます。

## テスト Guild へのコマンド登録

開発専用 token と `config.toml` を用意し、リポジトリルートで次を実行します。

```powershell
cargo run --package bot --locked -- --register-guild <TEST_GUILD_ID>
```

この操作は、Role 管理コマンドだけでなく `features::commands()` にある既存コマンドを含む全コマンドを、指定した Guild のこの Bot application 用コマンドとして登録して終了します。本番 Guild への登録はこの手順では行わないでください。

Bot をテスト Guild に導入する際は `applications.commands` と `bot` scope を使い、少なくとも `View Channels` と `Manage Roles` を付与します。Bot が扱えるのは Bot 自身の最上位 Role より下にある、Discord integration に管理されていない Role だけです。

`@everyone` は予約論理 ID `everyone` で表し、state の Role 対応表には含めません。Guild ID から自動的に解決されます。基底権限だけを管理し、名前・色・表示・メンション可否は管理しません。export では `@everyone` の権限を true/false とも列挙しますが、通常 Role は有効な権限だけを列挙します。通常 Role で省略された権限ビットは変更されず、明示的に `false` を指定した権限だけが無効化されます。

Bot自身が持たない権限をRoleへ新たに付与することはできません。planはそのような `false` から `true` への変更を入力エラーとして報告します。既存権限の維持と削除は許可され、Botが `ADMINISTRATOR` を持つ場合はすべての権限を付与可能として扱います。Role階層の制限は `ADMINISTRATOR` でも別途適用されます。

## 操作手順

1. `/role_export` を state 添付なしで実行し、`discord-roles.toml` と `discord-state.json` を保存します。
2. TOML を編集します。現時点で対応する構造は [サンプル](examples/discord-role-management.toml) を参照してください。
3. `/role_plan` に編集した TOML と、同じ export で得た state JSON を添付します。
4. `discord-role-plan.txt` で現在値と希望値を確認します。
5. `/role_apply` に同じ TOML と state JSON を添付し、表示された plan を確認して5分以内に適用ボタンを押します。
6. 結果とともに返る `discord-state.json` を保存し、次の操作に使用します。

## 既存 Resource の bind

作成結果が不明な Role・Channel・Member は、Discord 上で対象を確認してから `/bind` で対応 state に追加します。definition に対象の論理 ID を先に宣言し、`mode = "reference"` の Role・Channel・Member は属性を指定せず参照専用にします。Member は対象 Guild に所属している必要があります。

`/bind` には定義 TOML、現在の state JSON、`role`／`channel`／`member` の種別、論理 ID、Discord ID を渡します。たとえば参照専用 Channel と Member は次のように宣言します。

```toml
schema_version = 1

[channels.information]
mode = "reference"

[members.moderator]
mode = "reference"
```

返却された `discord-state.json` はそのまま次の `/role_plan` や後続の構成 plan に添付できます。`everyone` は予約参照で Guild ID へ解決されるため bind せず、同じ Discord ID の重複採用や既存論理 ID の付け替えはエラーになります。作成応答が不明な場合は、Discord クライアントで実物の ID を確認し、同じ definition と最新 state を使って一度 bind してください。

適用ボタンは plan の作成者だけが一度だけ操作できます。同じ Guild の apply は同時実行されません。確認後に管理対象の Role が外部で変更された場合は適用せず、新しい plan を求めます。処理は最初の失敗または処理期限で停止し、成功した属性数、未完了の属性数、取得済みの最新 state を返します。

再 export では `/role_export` に既存 state を添付します。state に対応済みの Role は論理 ID を維持し、結果は別の TOML/JSON 添付として返ります。state は Guild ごとに分けて保管してください。別 Guild の state、未対応の形式版、未知キー、未知の権限、参照先不足、論理 ID や Snowflake の衝突は Interaction の操作結果として表示されます。
