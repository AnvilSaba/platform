# 管理操作と state の受け渡しを Bot Interaction に集約する

Discord 内でファイルを渡して構成を更新できることを優先し、既存 Bot の Interaction を操作入口とする。独立 CLI や Git を正本とする運用、DB と Bot 側の永続 state バックアップは要求せず、利用者が Guild ごとの最新 state を保持する。

この選択により、捕捉できる途中失敗では成功分を巻き戻さず取得済み state を返すが、Bot 異常終了・API 応答消失・state 返却失敗を含む完全復旧は保証しない。対応を失った場合は export から再初期化するため、過去の論理 ID や管理投稿の対応を失う可能性を受け入れる。

決定日: 2026-09-05〜06。具体的な操作・検証条件は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2)。
