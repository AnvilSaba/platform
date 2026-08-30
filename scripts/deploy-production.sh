#!/usr/bin/env bash

set -Eeuo pipefail

target="${1:-}"
release_ref="${2:-}"
chart_version="${3:-}"

if [[ ! "$target" =~ ^(bot|mcguildlink|chart)$ ]]; then
  echo "デプロイ対象は bot、mcguildlink、chart のいずれかを指定してください。" >&2
  exit 2
fi
if [[ ! "$release_ref" =~ ^${target}/v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "デプロイ対象とリリースタグが一致しません: $target / $release_ref" >&2
  exit 2
fi
if [[ ! "$chart_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Chartバージョンの形式が不正です: $chart_version" >&2
  exit 2
fi

release_version="${release_ref#*/}"

for command in flock helm; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "$command がインストールされていません。" >&2
    exit 1
  fi
done

namespace="anvilsaba"
helm_release="platform"
chart="oci://ghcr.io/anvilsaba/charts/platform"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/anvilsaba"

install -d -m 700 "$state_dir"
exec 9>"$state_dir/deploy.lock"
if ! flock -w 900 9; then
  echo "別の本番デプロイが完了するまでにタイムアウトしました。" >&2
  exit 1
fi

if ! helm status "$helm_release" --namespace "$namespace" >/dev/null 2>&1; then
  echo "Helm release $helm_release が存在しません。先に初期デプロイを実行してください。" >&2
  exit 1
fi

image_args=()
case "$target" in
  bot)
    image_args=(--set-string "bot.image.tag=$release_version")
    ;;
  mcguildlink)
    image_args=(--set-string "mcguildlink.image.tag=$release_version")
    ;;
esac

helm upgrade "$helm_release" "$chart" \
  --version "$chart_version" \
  --namespace "$namespace" \
  --reset-then-reuse-values \
  "${image_args[@]}" \
  --rollback-on-failure \
  --wait=watcher \
  --timeout 10m \
  --history-max 20

helm status "$helm_release" --namespace "$namespace"
