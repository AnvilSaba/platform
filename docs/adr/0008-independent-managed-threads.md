# 管理スレッドを参照元 Message から独立させる

案内 Message の並べ替えや再投稿で補足スレッドまで失わないよう、管理スレッドは親 Message なしで作成し、独立した論理 ID で参照する。複数の Message から参照できる一方、作成通知の特定・削除と、再作成時の参照元リンク更新を管理側で引き受ける。公開型の作成可否と通知型の根拠は [API 調査](../research/discord-standalone-thread-research.md) に記録する。

決定日: 2026-09-05〜06。具体的な操作・検証条件は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2)。
