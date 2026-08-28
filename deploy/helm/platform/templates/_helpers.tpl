{{/*
共通のコンテナ制限設定です。
UID/GID など Workload 固有の実行ユーザー設定は呼び出し側に残します。
*/}}
{{- define "platform.restrictedContainerSecurityContext" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities:
  drop: ["ALL"]
{{- end }}
