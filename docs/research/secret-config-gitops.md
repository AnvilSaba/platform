# Secret and configuration GitOps research

Research date: 2026-10-08 (Asia/Tokyo). Scope: SOPS, age, Kustomize, and Flux configuration for the existing Helm workloads.

## Verified contracts

- Encrypt only `data` / `stringData` in Kubernetes Secret manifests with `encrypted_regex: ^(data|stringData)$`. Leave `apiVersion`, `kind`, and `metadata` readable for Flux. [Flux SOPS decryption](https://fluxcd.io/flux/components/kustomize/kustomizations/#decryption)
- SOPS accepts stdin and writes encrypted output to stdout. `--filename-override` selects the creation rule for stdin; explicit input and output types support JSON input and YAML output without a plaintext source file. [SOPS advanced usage](https://getsops.io/docs/usage/advanced/)
- SOPS creation rules match the file path relative to the configuration directory using native OS path separators. The repository rule accepts both Windows and Unix separators. [SOPS configuration source](https://github.com/getsops/sops/blob/v3.13.3/config/config.go#L536)
- By default SOPS MAC includes unencrypted values as well. Decrypt before applying transformations, and use SOPS to edit metadata as well as secret values. Do not hand-edit the encrypted document. [SOPS reference](https://getsops.io/docs/reference/)
- Flux Kustomization uses `spec.decryption.provider: sops` and `secretRef.name: sops-age`. Its decryption Secret is in the same namespace as that Kustomization. age identity data keys end in `.agekey`. [Flux SOPS decryption](https://fluxcd.io/flux/components/kustomize/kustomizations/#decryption)
- Kustomize normally appends a content hash to generated ConfigMap names and rewrites references within its own build. Existing Helm Deployments are outside that build; a fixed external `bot` contract therefore uses `disableNameSuffixHash: true`. This does not trigger Pod rollouts on configuration updates. [Kustomize generation](https://github.com/kubernetes-sigs/kustomize/blob/master/examples/configGeneration.md), [generator options](https://github.com/kubernetes-sigs/kustomize/blob/master/examples/generatorOptions.md)
- Flux supports `kustomize.toolkit.fluxcd.io/prune: disabled` to retain an object during garbage collection. The shared application namespace uses that annotation. [Flux prune control](https://fluxcd.io/flux/components/kustomize/kustomizations/#prune)

## Tool pins and local verification

| Tool      | Pin    | Primary source                                                                                  |
| --------- | ------ | ----------------------------------------------------------------------------------------------- |
| SOPS      | 3.13.3 | [SOPS release](https://github.com/getsops/sops/releases/tag/v3.13.3)                            |
| age       | 1.3.2  | [age release](https://github.com/FiloSottile/age/releases/tag/v1.3.2)                           |
| kubectl   | 1.37.1 | [Kubernetes release](https://github.com/kubernetes/kubernetes/releases/tag/v1.37.1)             |
| Kustomize | 5.8.2  | [Kustomize release](https://github.com/kubernetes-sigs/kustomize/releases/tag/kustomize/v5.8.2) |

mise's Aqua backend supports Windows. SOPS, age, Kustomize, and kubectl were installed and invoked through mise on Windows. [mise Aqua backend](https://mise.jdx.dev/dev-tools/backends/aqua.html)

All six consolidated YAML Secret samples were encrypted and decrypted successfully using the repository creation rule, YAML stdin, YAML ciphertext, and an ephemeral PQ age identity. The identity existed only in process memory; no production key or encrypted production value was generated.

## Follow-up: task-scoped dotenv for Bot

mise supports an `env` map directly on a task, and the official task-environment example uses `_.file` there. For `apps/bot/mise.toml`, the minimal task-scoped form is:

```toml
[tasks.run]
env = { _.file = { path = "{{ config_root }}/.env" } }
```

Here `config_root` is the directory containing that task's `mise.toml`, so this resolves to `apps/bot/.env`. Because the directive is under `[tasks.run]`, it applies to `:run`; it is not the project-wide `[env]` directive and will not load the Bot token into unrelated services' tasks. Merge this map with the existing `DATABASE_URL` task environment when implementing it. [mise task environments](https://mise.jdx.dev/environments/), [mise task configuration](https://mise.jdx.dev/tasks/task-configuration)

The official `env._.file` options are `path`, `tools`, `redact`, and `expand`; `optional` is not documented as an option for a dotenv file directive. The documented `optional = true` field applies to optional task dependencies, not to missing dotenv files. Therefore the proposed `{ path = "{{ config_root }}/.env", optional = true }` has no confirmed official behavior for an absent `apps/bot/.env` and should not be relied on. If absence must be allowed, use a separate local configuration decision or ensure the file exists; do not make the root `[env]` load it globally. [mise environment directives](https://mise.jdx.dev/environments/), [mise optional task dependencies](https://mise.jdx.dev/tasks/task-configuration)

The local mise registry resolves `kubectl` to `aqua:kubernetes/kubernetes/kubectl`. The official Kubernetes release page identifies `v1.37.1` as the current release checked on 2026-10-08, so the corresponding exact mise request is:

```toml
"aqua:kubernetes/kubernetes/kubectl" = "1.37.1"
```

[Kubernetes v1.37.1 release](https://github.com/kubernetes/kubernetes/releases/tag/v1.37.1), [mise Aqua backend](https://mise.jdx.dev/dev-tools/backends/aqua.html)

## Limits

The operator has supplied production Config, a PQ age recipient, and six encrypted production Secrets. The private identity remains outside Git. Kubernetes credentials, application connectivity, and live Flux reconciliation have not been verified. Operational instructions are in [Secret / Config setup](../secret-config.md).

## Follow-up: native post-quantum age interoperability

Required guidance: use age v1.3.0 or newer and SOPS v3.12.0 or newer; the repository pins age v1.3.2 and SOPS v3.13.3. For Flux, kustomize-controller v1.9.0 is the directly verified compatible released baseline; its source pins SOPS v3.13.1 and age v1.3.1 and imports `.agekey` entries through the SOPS identity parser. The exact first compatible Flux release was not established and is not required for this baseline. [age v1.3.2 README](https://github.com/FiloSottile/age/blob/v1.3.2/README.md#post-quantum-keys), [SOPS v3.12.0 dependency](https://github.com/getsops/sops/blob/v3.12.0/go.mod), [SOPS v3.12.0 parsers](https://github.com/getsops/sops/blob/v3.12.0/age/keysource.go#L435-L503), [controller v1.9.0 release](https://github.com/fluxcd/kustomize-controller/releases/tag/v1.9.0), [controller dependencies](https://github.com/fluxcd/kustomize-controller/blob/v1.9.0/go.mod), [Secret import path](https://github.com/fluxcd/kustomize-controller/blob/v1.9.0/internal/decryptor/decryptor.go#L249-L270)

Generate the hybrid X25519 + ML-KEM-768 identity with `mise exec -- age-keygen -pq -o key.txt`, then derive its recipient with `mise exec -- age-keygen -y key.txt`. The exact prefixes are `AGE-SECRET-KEY-PQ-1` for identities and `age1pq1` for recipients. SOPS v3.13.3 embeds age v1.3.1 and dispatches both prefixes to the native hybrid parsers; a standalone age upgrade does not upgrade SOPS. [age hybrid implementation](https://github.com/FiloSottile/age/blob/v1.3.2/pq.go), [SOPS v3.13.3 dependency](https://github.com/getsops/sops/blob/v3.13.3/go.mod), [SOPS v3.13.3 parsers](https://github.com/getsops/sops/blob/v3.13.3/age/keysource.go#L452-L531)

**Mixing constraint:** age rejects PQ and classic recipients in one age ciphertext. SOPS wraps its data key separately for each configured recipient, so it can still retain a classic recovery entry that defeats PQ-only protection. The creation rule and every existing SOPS recipient entry must therefore contain only complete `age1pq1...` recipients, with no classic age, SSH, PGP, KMS, or other non-PQ recovery key. [age label enforcement](https://github.com/FiloSottile/age/blob/v1.3.2/age.go#L117-L202), [SOPS recipient wrapping](https://github.com/getsops/sops/blob/v3.13.3/age/keysource.go#L79-L105), [SOPS encryption call](https://github.com/getsops/sops/blob/v3.13.3/age/keysource.go#L149-L181)

PQ-only `.sops.yaml` creation-rule recipe (replace the placeholder with the complete derived public recipient; the abbreviated value is deliberately invalid):

```yaml
creation_rules:
  - path_regex: 'deploy[/\\]config[/\\]production[/\\]secrets[/\\][^/\\]+\.sops\.yaml$'
    encrypted_regex: "^(data|stringData)$"
    age: REPLACE_WITH_AGE_PQ_PUBLIC_KEY
```

This matches the repository's production Secret path on Windows and Unix. Multiple recipients may be comma-separated only when every value is a complete `age1pq1...` recipient. Changing the rule does not remove classic recipients already stored in ciphertext. [SOPS rule parsing](https://github.com/getsops/sops/blob/v3.13.3/config/config.go), [recipient-list parsing](https://github.com/getsops/sops/blob/v3.13.3/age/keysource.go#L79-L105)

Local implementation evidence confirms that installed age v1.3.2 generated an ephemeral `-pq` identity and SOPS v3.13.3 encrypted and decrypted all six consolidated YAML Secret samples through the repository rule with a PQ-only recipient. No persistent test or production key was created. This verifies the local CLI path; no Flux version is installed or specified, and PQ identity import and live reconciliation remain unverified.
