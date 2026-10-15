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

{{/* Rust アプリの Pod 共通設定。Chart のルートを受け取る。 */}}
{{- define "platform.rustPodSettings" -}}
automountServiceAccountToken: false
imagePullSecrets: {{ toYaml .Values.imagePullSecrets | nindent 2 }}
securityContext:
  runAsNonRoot: true
  seccompProfile: { type: RuntimeDefault }
{{- end }}

{{/* dict "service" <アプリ設定> "postgres" <DB設定> を受け取る。 */}}
{{- define "platform.databaseEnv" -}}
- name: DATABASE_URL
  value: {{ printf "postgres://%s@%s:%v/%s" .service.databaseUsername .postgres.serviceName .postgres.port .postgres.database | quote }}
- name: DATABASE_PASSWORD
  valueFrom:
    secretKeyRef: { name: {{ printf "%s-database" .service.secretName }}, key: password }
{{- end }}

{{/* A digest is authoritative; tags remain available for older callers. */}}
{{- define "platform.image" -}}
{{- $repository := required "image.repository is required" .repository -}}
{{- if .digest -}}
  {{- if not (regexMatch "^sha256:[a-f0-9]{64}$" .digest) -}}
    {{- fail "image.digest must be a sha256 digest" -}}
  {{- end -}}
  {{- printf "%s@%s" $repository .digest -}}
{{- else -}}
  {{- printf "%s:%s" $repository (required "image.tag or image.digest is required" .tag) -}}
{{- end -}}
{{- end -}}
