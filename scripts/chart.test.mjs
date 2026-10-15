import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const chart = fileURLToPath(new URL("../deploy/helm/platform", import.meta.url));
const components = ["bot", "mcLinkServer", "publicApi", "dbMigrator"];
const digest = "sha256:" + "a".repeat(64);
const settings = components.flatMap((component) => ["--set-string", component + ".image.tag=legacy"]);
const render = (args = []) =>
  execFileSync("helm", ["template", "platform", chart, ...settings, ...args], { encoding: "utf8" });

test("digest takes precedence and migration hook is preserved", () => {
  const output = render([
    "--set",
    "dbMigrator.enabled=true",
    ...components.flatMap((component) => ["--set-string", component + ".image.digest=" + digest]),
  ]);
  for (const name of ["bot", "mc-link-server", "public-api", "db-migrator"]) {
    assert.ok(output.includes('image: "ghcr.io/anvilsaba/' + name + "@" + digest + '"'));
  }
  assert.match(output, /helm.sh\/hook.*pre-install,pre-upgrade/);
  assert.match(output, /helm.sh\/hook-weight.*"0"/);
  assert.match(output, /allowPrivilegeEscalation: false/);
  assert.match(output, /name: DATABASE_PASSWORD/);
});

test("legacy tags work and disabled migrator creates no Job", () => {
  execFileSync("helm", ["lint", chart, ...settings]);
  const output = render();
  assert.match(output, /ghcr.io\/anvilsaba\/bot:legacy/);
  assert.doesNotMatch(output, /kind: Job/);
});

test("missing reference and malformed digest fail", () => {
  for (const args of [
    ["--set-string", "bot.image.tag="],
    ["--set-string", "bot.image.digest=sha256:bad"],
    ["--set", "dbMigrator.enabled=true", "--set-string", "dbMigrator.image.tag="],
  ]) {
    const result = spawnSync("helm", ["template", "platform", chart, ...settings, ...args], { encoding: "utf8" });
    assert.notEqual(result.status, 0);
  }
});
