# preview は実行者だけに表示する

内容の確認に専用 Channel や公開投稿は不要なため、preview は Interaction の実行場所で ephemeral に返す。実際の管理スレッドは作らず、本番の対応状態を変更しない。実物の閲覧権限やリンク表示までの検証はテスト Guild で行う。

決定日: 2026-09-05〜06。具体的な操作・検証条件は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2)。
