# Discord 管理機能に関連する既存コードの調査

調査日: 2026-09-05〜06。実装状況の記録であり、機能要件は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2) を参照。

調査基準: `df96d286949f00bf553b54ba8e9b54a6531d3608`。調査開始時の作業ツリーはクリーン。秘密情報を含み得る実設定は読まず、型とサンプル・運用資料を参照。

| 項目 | 確認できた事実と設計への影響 | 根拠 |
| --- | --- | --- |
| Rust workspace | Bot バイナリと bot-macros の2メンバー。既存 Bot に管理操作を追加する際の調査対象 | [Cargo.toml](../../Cargo.toml) |
| ライブラリ | Serenity は表示上 0.12.5 だが Git rev `37b9f43`。Poise は表示上 0.6.1 だが Git rev `b189f7c`。公開安定版の対応表では判定できない | [Bot manifest](../../apps/bot/Cargo.toml)、[lock](../../Cargo.lock) |
| 起動 | bpaf の check-config、TOML 読み込み、Poise Framework、Gateway Client の構築。Guild/Members/Messages/Message Content intents を利用 | [main](../../apps/bot/src/main.rs) |
| 依存組み立て | DI コンテナではなく main と features の手動組み立て。BotData は設定を RwLock と Arc で共有。handler は Box の配列を順次 dispatch | [data](../../apps/bot/src/app/data.rs)、[features](../../apps/bot/src/features/mod.rs)、[dispatch](../../apps/bot/src/core/event_handler.rs) |
| bot-macros | async 関数から crate::core のイベント handler trait 実装を生成。コマンド登録・汎用 DI の仕組みではない | [macro](../../crates/bot-macros/src/lib.rs) |
| コマンド | Poise の属性マクロで定義し features::commands に明示列挙。alias ごとに複製。Bot ソースには Discord 側への登録呼び出しがなく、固定 Poise の init / Ready でも自動登録していない。新しい Bot identity へのコマンド登録は別途必要 | [features](../../apps/bot/src/features/mod.rs)、[固定 Poise](https://github.com/serenity-rs/poise/blob/b189f7c1cfd47eb4e7863683a49c389a9702ce23/src/framework/mod.rs) |
| Interaction | 認証は固定 custom_id を Gateway handler が処理。質問作成は collector、質問の解決ボタンは handler が処理。宣言からボタンを配置するだけでは処理は生まれない | [認証](../../apps/bot/src/features/auth/keyword.rs)、[質問](../../apps/bot/src/features/question/mod.rs) |
| config | config.toml を serde/toml で読み込み。Role/Channel/ForumTag は直接 Snowflake。reload_config は共有設定を差し替える | [config](../../apps/bot/src/app/config.rs)、[reload](../../apps/bot/src/features/admin.rs) |
| V2 | V2 flag と mention 抑制の helper が存在。Container/Text/Section/Separator/Thumbnail に加えログで MediaGallery/File を利用済み | [helper](../../apps/bot/src/utils.rs)、[components](../../apps/bot/src/app/utils/components.rs)、[ログ](../../apps/bot/src/features/message_logging/component_builder.rs) |
| pin | 既存メッセージのピン／解除と権限判定。本文を宣言的に同期する機能ではない | [pin](../../apps/bot/src/features/pin.rs) |
| 保存 | ログの snapshot 対応はメモリ上の DashMap。今回必要な永続 state や復旧 journal は存在しない | [snapshot](../../apps/bot/src/features/message_logging/snapshot_store.rs) |
| 配備 | k3s + Helm。Bot は readOnlyRootFilesystem、設定は Secret の read-only mount、Bot 用 PVC はない。常駐 Bot で state を書くなら配備方式の変更が必要 | [deployment](../../deploy/helm/platform/templates/bot-deployment.yaml)、[security](../../deploy/helm/platform/templates/_helpers.tpl) |
| DB | Helm に PostgreSQL、MCGuildLink に SQLite 永続化があるが、新機能がこれらを使う必然性はない | [values](../../deploy/helm/platform/values.yaml)、[運用](../deployment.md) |
| テスト | CI は cargo check --workspace --locked --all-targets と cargo test --workspace --locked。apps/crates 内に #[test] / #[tokio::test] は検索上なし | [CI](../../.github/workflows/ci-rust.yml)、[開発手順](../development-and-integration-testing.md) |
| リリース | Bot / MCGuildLink / Chart は独立バージョン。公開単位は既存のリリース手順で確認する | [リリース](../releases.md) |
