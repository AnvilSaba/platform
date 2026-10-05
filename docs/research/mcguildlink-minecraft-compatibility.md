# MCGuildLink Rust 移行における Minecraft 26.3 互換性調査

調査日: 2026-09-27（Asia/Tokyo）

## 結論

Minecraft: Java Edition 26.3 は実在し、2026-09-15 に正式リリースされている。Mojang の公式ランチャーメタデータでは `26.3` が `release` として登録され、公式 server.jar 内の `version.json` は安定版、プロトコル番号 `777`、Java 25、world/data version `5023` を示す。したがって、「26.3 のみを受け付ける」という対象は具体的に定義できる。

一方、crates.io の正確な名称が `mc_protocol` であるクレートの最新版 `2.2.0` は、Minecraft Java プロトコルの基本型、パケットフレーミング、圧縮、AES-128-CFB8 暗号化を提供する低水準ライブラリである。パケット定義を持たず、利用側が必要なパケットだけを定義する使い方は今回の設計意図と一致する。確認すべき点は「26.3 の完成済み packet catalog があるか」ではなく、codec、transport、online-mode 認証のうち、どこまでをこのクレートが担い、どこからを MCGuildLink が所有するかである。公開 API から確認できるのは codec と I/O wrapper、圧縮、共通鍵暗号までであり、TCP server lifecycle、接続状態機械、RSA 鍵交換、Mojang session server 検証はアプリ側または別ライブラリの責務になる。

Pumpkin は GPL のため、採用候補および調査・設計・実装の参照対象から除外する。

## 合意済みの対象

- Minecraft Java は 26.3 のみ対応する。
- Discord で発行したコードを、Minecraft 接続時の Configuration フェーズに表示するダイアログへ入力する現行フローを維持する。
- 26.2 以前や 26.4 以降を同時対応することは、この調査の対象外とする。

## 現行 Kotlin 実装で確認できたフロー

1. Discord の紐付け開始ボタンを押すと、`AccountLinkService.getOrCreateLinkRequest` がコードを取得または生成し、ephemeral 応答に接続先、対応 Minecraft バージョン、コードを表示する（[AccountLinkButtons.kt](../../apps/mcguildlink/app/src/main/kotlin/io/github/anvilsaba/mcguildlink/app/discord/accountlink/interactions/AccountLinkButtons.kt)）。
2. コードは既定で 8 文字であり、紛らわしい文字を除いた固定 alphabet と `SecureRandom` から生成される（[LinkCodeGenerator.kt](../../apps/mcguildlink/app/src/main/kotlin/io/github/anvilsaba/mcguildlink/app/util/LinkCodeGenerator.kt)）。同じ Discord ユーザーに未消費リクエストがある場合は既存コードを再利用する（[AccountLinkService.kt](../../apps/mcguildlink/app/src/main/kotlin/io/github/anvilsaba/mcguildlink/app/service/AccountLinkService.kt)）。
3. Minecraft サーバーは Minestom の `Auth.Online()` で起動し、`AsyncPlayerConfigurationEvent` 内でコード入力を待つ。すなわち、Play フェーズへ入る前の Configuration フェーズで処理する（[MinecraftServer.kt](../../apps/mcguildlink/app/src/main/kotlin/io/github/anvilsaba/mcguildlink/app/minecraft/MinecraftServer.kt)）。
4. サーバーは `DialogInput.Text` を含むダイアログを表示する。送信ボタンは `mcguildlink:submit_code` の動的カスタムアクションであり、`PlayerConfigCustomClickEvent` の NBT payload から `code` を取り出す。無効コードなら同じダイアログを再表示し、成功、既存リンク、ブロック済みの場合は結果別のダイアログを表示する（同上）。
5. 有効なコードでは、online-mode で得た Minecraft UUID とユーザー名を Discord アカウントへ紐付け、成功時にコードを削除してホワイトリスト更新を要求する（[AccountLinkService.kt](../../apps/mcguildlink/app/src/main/kotlin/io/github/anvilsaba/mcguildlink/app/service/AccountLinkService.kt)）。

ここでいう「本人認証」の技術的意味は、Minecraft 側の online-mode 認証済みアカウントと、Discord 側で受け取ったコードを所持する利用者を結び付けることである。自然人として同一人物であることまで証明する仕組みではない。

現行実装では、Minecraft 接続中の入力待ちタイムアウトはあるが、発行済みコード自体の有効期限、失敗回数制限、試行レート制限は確認できなかった。またコード比較は文字列の完全一致である。Rust 移行時に「フロー」を維持することと、これらのセキュリティ特性まで無変更にすることは分けて決める必要がある。

## Minecraft Java 26.3 の版とプロトコル

### 確認済み

- Mojang の公式リリース記事は 2026-09-15 公開で、表題を「Minecraft Java Edition 26.3」、状態を「Released」としている（[Minecraft Java Edition 26.3](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-3)）。
- Mojang の公式ランチャーメタデータは `id: 26.3`、`type: release`、`releaseTime: 2026-09-15T11:23:02+00:00` を返す（[26.3 launcher metadata](https://piston-meta.mojang.com/v1/packages/bc098d111a72e9f6178801544a42099bdfbb0cf2/26.3.json)、一覧は [version_manifest_v2.json](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json)）。
- 同メタデータが指す公式 server.jar は SHA-1 `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c` である（[Mojang server.jar](https://piston-data.mojang.com/v1/objects/33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c/server.jar)）。この jar 内の `version.json` を直接抽出して確認した値は、`id: 26.3`、`protocol_version: 777`、`world_version: 5023`、`java_version: 25`、`stable: true` である。
- 独立した実装側の反証確認として、ViaVersion のプロトコル定義も `v26_3 = register(777, "26.3")` としている（[ViaVersion ProtocolVersion.java](https://github.com/ViaVersion/ViaVersion/blob/master/api/src/main/java/com/viaversion/viaversion/api/protocol/version/ProtocolVersion.java)）。ただし、プロトコル番号の決定根拠には上記 Mojang jar を優先する。
- 公式 26.3 リリースノートの Data Pack `121.0`、Resource Pack `97.1` も jar 内の値と一致する（[26.3 Technical Changes](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-3#technical-changes)）。これらはネットワークプロトコル番号 `777` とは別の版番号である。

### 26.3 のみを受け付けるための境界

Handshake の protocol version が `777` であることを検査し、それ以外を Login / Configuration へ進めない設計が必要である。Status 応答も表示名 `26.3` と protocol `777` を返す必要がある。文字列のバージョン名だけではなく数値 `777` を判定基準にする。

これだけでは十分ではない。現行フローには少なくとも Status、Handshake、Login、online-mode の暗号化とセッション検証、Login Acknowledged、Configuration、Show Dialog、Custom Click Action、切断が関係する。Configuration へ到達するまでのパケット順序と registry/configuration 応答を含む状態機械を、採用ライブラリがどこまで担うかを確認する必要がある。

## `mc_protocol` の正確な候補と対応範囲

### crates.io `mc_protocol` 2.2.0

- 正確な package/crate 名は `mc_protocol`、調査時点の最新版は `2.2.0`。crates.io API が示すリポジトリは `kauri-off/mc_protocol` で、最終更新は 2026-06-12 である（[crates.io](https://crates.io/crates/mc_protocol)、[公式リポジトリ](https://github.com/kauri-off/mc_protocol)、[docs.rs 2.2.0](https://docs.rs/mc_protocol/2.2.0/mc_protocol/)）。
- 提供物は VarInt/VarLong、基本型の serialize/deserialize、長さ付きパケットフレーム、zlib 圧縮、AES-128-CFB8、同期/非同期 I/O、`#[derive(Packet)]` である。README と公開 API の module 一覧が直接これを示す（[README](https://github.com/kauri-off/mc_protocol#features)、[docs.rs module list](https://docs.rs/mc_protocol/2.2.0/mc_protocol/#modules)）。
- 公開 API には、26.3/777 と結び付いた packet catalog、Handshake/Login/Configuration/Play の状態機械、Mojang session server を用いる online-mode 認証、Dialog の NBT モデル、Show Dialog、Custom Click Action が見当たらない。README の例も、利用者が packet ID と struct を自分で定義する形である。
- したがって、このクレートは wire codec の候補にはなるが、Minestom の置換となるサーバーフレームワークではない。「26.3 非対応」と断定するより、「版固有機能を提供していないため、26.3 対応は利用側の実装と試験に依存する」と表現するのが正確である。
- `2.2.0` の公開日は 26.3 正式リリースより前である。ただし低水準プリミティブは版をまたいで利用できるため、日付だけを理由に使用不能とはいえない。反対に、基本型を処理できることは 26.3 の接続成立を証明しない。

### 名前が似る候補と反証

- `airi-protocol` `1.0.0` は「全パケット」を掲げるが、公式 README の対応表は Minecraft 26.2 / protocol 776 であり、26.3 / 777 の根拠にはできない（[airi-protocol README](https://docs.rs/crate/airi-protocol/1.0.0/source/README.md)）。
- Azalea は Rust の広範なクライアント/プロトコル実装だが、公式 README の調査時点の対応版は 26.2 である。また主目的はクライアント/ボットであり、今回のサーバー置換とは方向が異なる（[azalea-rs/azalea](https://github.com/azalea-rs/azalea)）。
- `mc-rpc` は Minecraft Server Management Protocol の Rust binding であり、プレイヤー接続用 Java Edition network protocol とは別物である（[mc-rpc docs](https://docs.rs/mc-rpc/latest/mc_rpc/)）。

## 実装可能性の判断

現時点の判断は「26.3 を対象とする Rust 実装の経路は存在するが、採用ライブラリとダイアログ経路のスパイク前には実装可能と確定しない」である。

`mc_protocol` を基盤に必要なパケットだけを実装する案は、不要な完成済み packet catalog を避けるという合意済みの設計意図に合う。実装量の多寡だけで不採用とはしない。採否を左右するのは、MCGuildLink が所有する最小状態機械と online-mode 認証を正しく境界化できるか、26.3 固有 schema を一次資料または実通信で固定できるか、異常系を含む相互運用試験を用意できるかである。これらはまだ未検証であり、基本 codec が揃うことだけから end-to-end の実装可能性は断定しない。

## 採用方針

mc_protocol を通信の基盤に使い、26.3 に必要なパケットと接続状態処理をアプリ内で実装する。通信処理は自動テストし、実クライアントとの接続とダイアログは手動で確認する。実クライアントの CI 自動化は今回の対象外。

## 最小スパイクの合格条件

- 正規の Minecraft 26.3 クライアントが online-mode で接続でき、UUID/username がセッション検証後の値になる。
- 26.2（protocol 776）および 26.4 snapshot を明示的に拒否する。
- Play フェーズへ進めず、Configuration 中に text input 付きダイアログを表示できる。
- 8 文字コードを送信すると、serverbound Custom Click Action から action ID と NBT の `code` を復元できる。
- 無効コードで再表示、成功/既存リンク/ブロック済みで結果ダイアログ、ボタン操作またはタイムアウトで切断できる。
- malformed/oversized payload、空コード、切断競合、タイムアウト競合で panic や接続リークを起こさない。
- 成功時に既存の `AccountLinkService` 相当の原子的処理、コード削除、ホワイトリスト更新要求を保てる。

## MC-Runtime-Test / HeadlessMC を使うクライアント E2E

以下は採用判断のために調査した選択肢の記録であり、今回の実装要件ではない。設計協議では実クライアントのCI自動化を対象外とし、26.3の実クライアントとの接続確認は手動で行うことに合意した。

### 結論

**条件付きで使える可能性が高いが、調査時点で Minecraft 26.3 をそのまま指定して完成する構成ではない。** HeadlessMC は実際の Minecraft Java クライアントを起動でき、HMC-Specifics には外部サーバーへの `connect`、画面列挙の `gui`、テキスト入力の `text`、ボタン操作の `click` がある。しかし MC-Runtime-Test と HMC-Specifics の公式対応表はいずれも 26.2 までで、26.3 の公式サポート表明、配布 artifact、成功した CI run は確認できなかった（[MC-Runtime-Test](https://github.com/headlesshq/mc-runtime-test)、[HMC-Specifics](https://github.com/headlesshq/hmc-specifics)）。

したがって採用判断は次のようになる。

| 確認対象                            | 判断                                 | 根拠と条件                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ----------------------------------- | ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Vanilla/Fabric 26.3 の起動          | **未確認**                           | HeadlessMC 本体は任意 version を `launch <version>` で起動し、LWJGL patch は Minecraft version 非依存だと説明している。一方、MC-Runtime-Test の対応表とソース tree は 26.2 までである（[HeadlessMC launching](https://headlesshq.github.io/headlessmc/launch/)、[MC-Runtime-Test README](https://github.com/headlesshq/mc-runtime-test#supported-minecraft-versions-and-modloaders)）。26.3 + Java 25 + Fabric Loader の実起動確認が必要。                                                                                                                                                                                                                       |
| 外部 Rust サーバーへの接続          | **条件付きで可能**                   | HMC-Specifics は `connect <ip> <port>` を公式に提供する（[Specifics: Servers](https://headlesshq.github.io/headlessmc/specifics/#servers)）。ただし 26.3 対応 artifact は未確認なので、26.3 では HMC-Specifics の port または接続処理を持つ custom Fabric test mod が必要。MC-Runtime-Test 同梱 mod は single-player world へ入り GameTest を実行する用途で、任意の外部サーバー接続は標準動作ではない（[MC-Runtime-Test features](https://github.com/headlesshq/mc-runtime-test#features)、[mc-runtime-test-mod](https://github.com/headlesshq/mc-runtime-test-mod)）。                                                                                          |
| Configuration の Show Dialog を認識 | **条件付き、26.3 では未確認**        | HMC-Specifics の `gui` は現在の Screen と GUI 要素を列挙し、`render` は描画文字列を列挙する。ただし公式対応は 26.2 までで、新しい 26.3 dialog screen/input が列挙対象になる根拠はまだない（[Specifics: GUIs](https://headlesshq.github.io/headlessmc/specifics/#guis)）。                                                                                                                                                                                                                                                                                                                                                                                        |
| コード入力と Custom Click 送信      | **条件付き、26.3 では未確認**        | `text` と `click` により通常の text field と button は操作できる（[Specifics: Text](https://headlesshq.github.io/headlessmc/specifics/#text)）。26.3 の configuration dialog でも同じ抽象化が効くかは未確認。custom Fabric test mod で vanilla の dialog screen にコードを設定し、実際の button action を発火させれば、サーバー側の Custom Click 受信まで E2E にできる。packet を mod から直接送るだけの試験では UI 配線を検証できない。                                                                                                                                                                                                                         |
| Microsoft online-mode 認証          | **対話実行は可能、無人 CI は未確立** | HeadlessMC の公式手順は `login` 後に表示される Microsoft device-code URL を人が開く方式で、起動時の account refresh 設定もある（[HeadlessMC login](https://headlesshq.github.io/headlessmc/launch/#logging-in)、[configuration](https://headlesshq.github.io/headlessmc/configuration/#hmcaccountrefreshongamelaunch)）。既存認証状態を CI secret として安全に復元する公式手順、service account、非対話の初回 login は確認できなかった。公式 repository の issue には保存済み account が約3日後に refresh 失敗して消えるという未解決報告もあるため、長期無人運用を前提にしない（[headlessmc issue #300](https://github.com/headlesshq/headlessmc/issues/300)）。 |

### LWJGL stub と Xvfb の違い

HeadlessMC の `-lwjgl` は LWJGL の各関数を no-op または stub 値へ書き換える方式である。これは version 非依存で画面のない CI に向く一方、実際の pixel 描画、font/layout、GPU/native library 経路を通ったことは保証しない（[HeadlessMC optimizations](https://github.com/headlesshq/headlessmc#optimizations)、[headlessmc-lwjgl](https://github.com/headlesshq/headlessmc/blob/main/headlessmc-lwjgl/README.md)）。このモードで確認できるのは、主にクライアントの screen/widget 状態、入力イベント、ネットワーク packet と状態遷移である。

MC-Runtime-Test は Xvfb も提供し、Xvfb 使用時は `-lwjgl` が不要だと説明する（[MC-Runtime-Test inputs](https://github.com/headlesshq/mc-runtime-test#inputs)、[mc-runtime-test-mod README](https://github.com/headlesshq/mc-runtime-test-mod#mc-runtime-test-mods)）。Xvfb は通常の描画コードを virtual framebuffer 上で走らせるので、スクリーンショットや文字・配置の回帰検査にはこちらが適する。ただし仮想 X server 上の描画であり、利用者の OS/GPU 上での表示保証そのものではない。リリース判定には、vanilla 26.3 を通常表示で起動する手動 smoke test を残す。

### 調査で検討した試験構成（今回は採用しない）

1. **packet/状態試験**: Rust 側に deterministic な test peer を置き、Show Dialog の NBT、Custom Click payload、Configuration の状態遷移を account と描画なしで試験する。
2. **HeadlessMC + custom Fabric test mod**: 26.3 client を起動し、外部 Rust server へ接続、dialog screen の class/widget を検出、Discord 発行コードを入力、実ボタンを押し、結果 dialog または切断理由を assertion して終了する。26.3 対応 HMC-Specifics が提供されるまでは、汎用 `gui/text/click` に依存するよりこの小さな version 固定 mod の方が検証範囲を明示しやすい。
3. **Xvfb job**: 上と同じ test mod を `-lwjgl` なしで走らせ、画面が構築・描画される経路とスクリーンショットを確認する。
4. **online-mode job**: 購入済み専用 Microsoft account の認証状態を GitHub Actions secret として扱える手順を別途確立できた場合だけ、保護された定期/手動 job で実行する。初回 device-code login、refresh、失効時の再 bootstrap、secret rotation を運用設計に含める。公式に確立された無人 credential 注入方式を確認できていないため、通常 PR ごとの必須 job にはしない。
5. **実画面 smoke test**: vanilla 26.3 の通常クライアントで text input、button label、無効コード時の再表示、成功後の結果表示を人が確認する。

この構成では、MC-Runtime-Test/HeadlessMC は「実クライアントを CI で起動する器」として使う。26.3 の接続と dialog 操作を実際に担うのは custom Fabric test mod であり、MC-Runtime-Test 標準 modや現行 HMC-Specificsだけで要件を満たすとは扱わない。

## 未解決点と追加検証

- `mc_protocol` を使った online-mode の end-to-end 実装例は確認できず、暗号 primitive の存在だけではセッション認証の正しさを保証できない。
- コードの entropy、総当たり耐性、期限と試行制限は移行互換性とは別のセキュリティ判断が必要。
- HeadlessMC 単体で vanilla/Fabric 26.3 + Java 25 を起動できるか、MC-Runtime-Test action の version module を追加すれば動くかは未検証。
- HMC-Specifics の 26.2 実装を 26.3 へ単純移植できるか、26.3 dialog screen が既存 `gui/text/click` の抽象化で扱えるかは未検証。
- Microsoft 認証状態を CI secret として安全かつ継続的に refresh する公式運用は確認できていない。

以上は medium で一次資料と現行ソースから確認できた範囲である。未解決点は推測で埋めず、上記の最小スパイクで検証する。
