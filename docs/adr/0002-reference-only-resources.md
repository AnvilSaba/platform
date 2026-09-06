# 管理外リソースへの参照を論理 ID で明示する

手動管理の Role や Category を参照するため、属性を管理しない参照専用宣言を設け、Discord ID との対応は state に分離する。名前一致や state の残存だけによる暗黙の接続では、同名資源や管理範囲の編集によって別の対象へ接続し得るため、定義による参照意図と明示的な bind を必要とする。

決定日: 2026-09-05〜06。具体的な操作・検証条件は [Feature Spec #2](https://github.com/AnvilSaba/platform/issues/2)。
