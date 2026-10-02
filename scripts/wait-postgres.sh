#!/bin/sh
set -eu
attempt=0
until pg_isready -h 127.0.0.1 -U postgres -d "$1"; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 60 ]; then
        echo "PostgreSQL の起動待ちがタイムアウトしました。対象 DB のログを確認してください。" >&2
        exit 1
    fi
    sleep 1
done
