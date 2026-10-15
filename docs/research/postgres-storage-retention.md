# PostgreSQL storage retention

Research date: 2026-10-14 (Asia/Tokyo).
Scope: Kubernetes PV/PVC reclamation, Helm retention, bundled k3s local-path storage, and CloudNativePG 1.30 deletion behavior.
Evidence: repository files, local kubeconfig context names, and official documentation/source; no production credentials or cluster access.
Integration behavior remains unverified; no Helm uninstall or PVC deletion was executed.

## Existing chart

- `deploy/helm/platform/templates/postgres-pvc.yaml` declares a standalone `postgres-data` PVC; its metadata contains only `name`, without `helm.sh/resource-policy: keep` or `ownerReferences`.
- `postgres-statefulset.yaml` references that PVC through `volumes`; it has no `volumeClaimTemplates`. StatefulSet deletion alone does not establish ownership of this PVC ([Kubernetes ownership](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)).
- Empty `postgres.storageClassName` in `values.yaml` causes the template to omit the field, rather than emit explicit `storageClassName: ""`; an omitted field allows default-class assignment ([StorageClasses](https://kubernetes.io/docs/concepts/storage/storage-classes/#default-storageclass)).
- The running production PVC's class and bound PV's reclaim policy remain unknown; neither repository defaults nor a configured local context establish production behavior.
- [ADR 0005](../adr/0005-mcguildlink-cutover.md) explicitly assigns production backup/recovery design to separate work; the parent investigation found no deployed backup setup. Backup availability remains unverified.

## Kubernetes after PVC deletion

- `Retain`: the PV and backing data remain, with the PV `Released`; another claim cannot automatically reuse it without administrator reclamation/rebinding ([PV reclaiming](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#retain)).
- `Delete`: a supporting provisioner removes the PV and its backing storage after the claim is released ([PV deletion](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#delete)).
- Dynamically provisioned PVs inherit their StorageClass policy, defaulting to `Delete` when unspecified; an existing PV's own `persistentVolumeReclaimPolicy` determines reclamation ([PV deletion](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#delete)).
- PVC protection postpones deletion while a Pod uses the claim; it does not provide permanent retention after that Pod disappears ([in-use protection](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#storage-object-in-use-protection)).

## Helm keep: implementation and limits

- Helm documents `helm.sh/resource-policy: keep` as skipping deletion during uninstall, upgrade, or rollback; a retained resource becomes orphaned and can complicate reinstall ([official guidance](https://helm.sh/docs/howto/charts_tips_and_tricks/#tell-helm-not-to-uninstall-a-resource)).
- Repository-pinned Helm **4.2.4** filters `keep` resources from the **stored release manifest** before uninstall deletion; its upstream unit test covers that path ([uninstall](https://github.com/helm/helm/blob/v4.2.4/pkg/action/uninstall.go#L238-L267), [filter](https://github.com/helm/helm/blob/v4.2.4/pkg/action/resource_policy.go#L23-L42), [test](https://github.com/helm/helm/blob/v4.2.4/pkg/action/uninstall_test.go#L57-L86)).
- Current Helm **4.3.0** also filters kept manifests before deletion; this is source verification, not an executed cluster test ([uninstall implementation](https://github.com/helm/helm/blob/v4.3.0/pkg/action/uninstall.go#L296-L320)).
- In 4.2.4, adding `keep` only to the live PVC is insufficient for uninstall: filtering reads release metadata, and the downstream delete function does not recheck live annotations ([filter](https://github.com/helm/helm/blob/v4.2.4/pkg/action/resource_policy.go#L23-L42), [delete](https://github.com/helm/helm/blob/v4.2.4/pkg/kube/client.go#L838-L875)).
- Historical [Helm issue #10890](https://github.com/helm/helm/issues/10890) reports Helm 3.7.2 behavior; issue closure/status alone does not establish installed Helm 4 behavior.
- `keep` is a Helm deletion rule, not Kubernetes deletion protection: direct PVC deletion, namespace deletion, or owner-driven garbage collection can bypass it ([ownership](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/), [namespace case](https://github.com/helm/helm/issues/12074)). `--keep-history` retains release history, not resources ([uninstall reference](https://helm.sh/docs/helm/helm_uninstall/)).

## Bundled k3s local-path and chart implication

- The upstream k3s bundled manifest marks `local-path` as default, uses `rancher.io/local-path`, and explicitly sets `reclaimPolicy: Delete` ([k3s manifest](https://github.com/k3s-io/k3s/blob/master/manifests/local-storage.yaml#L90-L99)); installed versions/customizations may differ.
- Its teardown script runs `rm -rf "${VOL_DIR}"`; local-path's delete implementation invokes a helper Pod for cleanup unless the PV policy is `Retain` ([bundled teardown](https://github.com/k3s-io/k3s/blob/master/manifests/local-storage.yaml#L121-L124), [provisioner v0.0.37](https://github.com/rancher/local-path-provisioner/blob/v0.0.37/provisioner.go#L549-L588)). Cleanup is asynchronous and can fail or be skipped when the node no longer exists.
- Inference: uninstall/removal of the current standalone chart PVC can delete its data **if its actual bound PV uses `Delete`**; StatefulSet usage supplies no chart-level retention. Helm `keep` preserves the claim during Helm deletion, while PV `Retain` preserves backing data after claim deletion; neither establishes the unknown production policy.

## CloudNativePG deletion and recommendation

- A CNPG `Cluster` owns generated PVCs; normal cascading deletion removes dependent objects. PVC creation uses cluster metadata inheritance, and the plugin's explicit keep operation removes their owner references ([PVC construction](https://github.com/cloudnative-pg/cloudnative-pg/blob/release-1.30/pkg/reconciler/persistentvolumeclaim/build.go), [plugin documentation](https://github.com/cloudnative-pg/cloudnative-pg/blob/release-1.30/docs/src/kubectl-plugin.md#destroy), [Kubernetes ownership](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)). The resulting PV/data reclamation still follows the PV's own policy.
- The 1.30 `StorageConfiguration` exposes class, size, resize behavior, and a PVC **spec** template, with no Cluster-deletion PVC-retention switch. A `Database` reclaim policy concerns a SQL database, not the lifecycle of Cluster PVCs ([Cluster types](https://github.com/cloudnative-pg/cloudnative-pg/blob/release-1.30/api/v1/cluster_types.go#L2027-L2049), [Database management](https://cloudnative-pg.io/docs/1.30/declarative_database_management/)).
- For planned suspension, declarative `cnpg.io/hibernation: "on"` keeps the Cluster and all PVCs while removing instance Pods; resume by changing it to `off` ([hibernation](https://cloudnative-pg.io/docs/1.30/declarative_hibernation/)). This does not guard against deletion of the Cluster itself.
- The plugin's `destroy --keep-pvc` is an explicit **instance** operation, not a global safety policy. Do not promise arbitrary Cluster recreation automatically adopts retained volumes ([plugin source documentation](https://github.com/cloudnative-pg/cloudnative-pg/blob/release-1.30/docs/src/kubectl-plugin.md#destroy)).
- Helm `keep` must protect the chart-managed Cluster itself if that Cluster lives in a Helm release; annotations on operator-created PVCs do not prevent Kubernetes garbage collection. Flux prune controls similarly concern Flux-managed resources. Direct Cluster or namespace deletion remains a separate path.
- Operator Helm uninstall and Cluster deletion are distinct: the operator chart retains CRDs, but deleting those CRDs cascades through all Cluster resources and their PVCs ([official operator chart warning](https://github.com/cloudnative-pg/charts/blob/main/charts/cloudnative-pg/README.md#uninstalling)).
- Recommendation (inference): separate database lifecycle from application releases, use Helm/Flux retention at the database resource plus RBAC/admission safeguards for accidental deletion, choose a dedicated `Retain` storage class and verify existing PV policies, and maintain independent PostgreSQL backups with a tested restore. Retention is not a backup or automatic recovery mechanism.
- Local kubeconfig has only `k3d-anvilsaba`; the production PVC/PV settings could not be queried. No runtime infrastructure changes or destructive verification were performed.
