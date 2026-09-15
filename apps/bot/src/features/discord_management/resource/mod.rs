//! 管理対象リソースごとの差分計算・変換をまとめる内部モジュールです。
//!
//! Discord との通信はここでは行いません。通信を必要とする処理は `port` の
//! Interface を通じて上位のワークフローから受け取ります。

pub(super) mod channel;
pub(super) mod role;
