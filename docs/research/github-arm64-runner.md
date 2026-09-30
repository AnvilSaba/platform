# GitHub Actions 公開リポジトリ向け ARM64 runner

調査日: 2026-09-30（Asia/Tokyo）

## 結論

公開リポジトリ向け標準 GitHub-hosted runner として、`runs-on: ubuntu-24.04-arm` は現在利用可能である。GitHub公式表では Linux / arm64、4 CPU、16 GB RAM、14 GB SSD の runner label として掲載されている（[GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners#standard-github-hosted-runners-for-public-repositories)）。

公開リポジトリでは標準 GitHub-hosted runner の利用は無料かつ無制限と明記されている（[GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners#supported-runners-and-hardware-resources)）。したがって、`image-mcguildlink-old.yml` の `runs-on` を `ubuntu-24.04-arm` に変更すれば、ARM64 上でターゲット native JDK を実行する構成にできる。

制限: 参照資料の ARM64 固有の制限記載は macOS runner 向けであり、`ubuntu-24.04-arm` について追加の制限は同表には記載されていない。
