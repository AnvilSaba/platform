# MC Link Server

Minecraft Java 26.3（プロトコル 777）の online-mode 本人認証後、Play に進めず Configuration ダイアログでコードを受け付ける。コードは PostgreSQL で照合し、成功時は紐付け、コード消費、監査ログ、配送予定を同一トランザクションで保存する。本番環境へはまだ配置しない。

```powershell
Copy-Item apps/mc-link-server/config.example.toml config.toml
$env:DATABASE_URL = 'postgres://platform_mcguildlink:<パスワード>@localhost:5432/platform'
cargo run -p mc-link-server
```

作業ディレクトリの `config.toml` に待受アドレスを設定する。書式は [config.example.toml](config.example.toml) を参照。`DATABASE_URL` は必須。接続時に Mojang セッションサーバーへ問い合わせ、返された UUID と名前が Login Start の値と一致した場合だけ Configuration に進む。接続から 5 分でコード入力を打ち切る。

| 入力コード | 結果 |
| --- | --- |
| 未使用の有効なコード | 成功ダイアログ。コードを消費 |
| 対象の組み合わせが既に紐付け済み | 紐付け済みダイアログ。コードは保持 |
| どちらかのアカウントがブロック済み | ブロック済みダイアログ。コードは保持 |
| 無効・空欄 | エラーを表示して再入力 |

実クライアントでの確認は、正規アカウントで Java 26.3 を起動し、Discord でコードを発行してから上記の接続先に入り、コード入力・結果・切断を画面上で確認する。別バージョン（例: 26.2）が Login に進めないことも確認する。確認結果は次の表へ記録する。

| 確認項目 | 結果 |
| --- | --- |
| 26.3 の本人認証後に UUID・名前がログへ出る | 未実施（この作業環境に認証済み 26.3 クライアントがない） |
| Configuration 中の入力・再入力・結果表示 | 未実施 |
| 結果ボタンと入力タイムアウトによる切断 | 未実施 |
| 26.2 の拒否 | ユーザー報告: 26.3 を求めるメッセージと Windows の接続中止エラーが交互に発生。未読データを排出してから切断する修正を追加したが、実クライアントでの再確認は未実施。 |

パケット ID と Login Finished の 26.3 固有フィールドは、Mojang 公式 `server.jar`（SHA-1 `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c`）のクラス定義で照合した。送信するコード入力・成功ダイアログの NBT は、同じ jar の `Dialog.DIRECT_CODEC` で両方とも `valid=true` を確認した。自動テストでは通信相手から Status、再入力、結果表示、切断、タイムアウトを確認している。
