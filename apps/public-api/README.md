# ホワイトリスト公開 API

独立した Axum サービスとして `GET /whitelist.json` を公開する。旧版と同じく UUID 昇順の `[{"uuid":"...","name":"..."}]` を返し、複数の紐付けは UUID で重複除去する。ブロック済みの Minecraft アカウント、およびブロック済み Discord アカウントとの紐付けは除外する。

アクセスのたびに PostgreSQL の更新番号を確認する。番号が変わった場合は一覧と番号を同じ DB スナップショットから読み、各プロセスの JSON キャッシュを更新する。DB 確認または再生成に失敗したリクエストは `503 Service Unavailable` を返す。

```powershell
$env:DATABASE_URL = 'postgres://platform_public_api:<パスワード>@localhost:5432/platform'
$env:PUBLIC_API_LISTEN = '0.0.0.0:8080' # 省略可
cargo run -p public-api --locked
```

ログインユーザーには `platform_public_api_runtime` だけを付与する。DB のマイグレーションは専用 Job で適用し、このサービスは起動時に必要な履歴を検証する。

実 PostgreSQL を使うテストには、テスト専用 DB の管理ユーザーを `DATABASE_URL` に指定して `cargo test -p public-api --test whitelist --locked` を実行する。
