from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"


class ReleaseRoutingContractTests(unittest.TestCase):
    def test_main_push_only_orchestrates_release_please(self) -> None:
        release_please = (WORKFLOWS / "release-please.yml").read_text()
        os_release = (WORKFLOWS / "release.yml").read_text()

        self.assertIn("branches: [main]", release_please)
        self.assertNotIn("branches: [main]", os_release)
        self.assertIn('tags: ["v*"]', os_release)
        self.assertIn("workflow_dispatch:", os_release)
        self.assertNotIn("group: os-release-", os_release)
        self.assertIn('source_sha="$GITHUB_SHA"', os_release)
        self.assertIn('if [ "$GITHUB_REF" != "refs/heads/main" ]', os_release)
        self.assertNotIn('gh api "repos/${repo}/commits/main"', os_release)
        # Every build job checks out one immutable commit, and it takes it from
        # github.sha rather than from a job output. release-meta sets source_sha
        # to GITHUB_SHA on both paths, so the value is the same; the difference
        # is that no job output decides which code a privileged job builds, which
        # is what the cache-poisoning analysis objects to.
        self.assertEqual(os_release.count("ref: ${{ github.sha }}"), 3)
        self.assertNotIn("ref: ${{ github.ref }}", os_release)
        self.assertNotIn("ref: ${{ github.ref_name }}", os_release)
        self.assertNotIn("ref: main", os_release)
        # Caches are read by later default-branch runs, so only main fills them.
        self.assertIn("save-if: ${{ github.ref == 'refs/heads/main' }}", os_release)
        self.assertEqual(os_release.count("uses: actions/cache@"), 0)
        self.assertIn(
            '--commit "${{ needs.release-meta.outputs.source_sha }}"', os_release
        )
        self.assertGreaterEqual(
            os_release.count("name: ${{ needs.release-meta.outputs.channel"), 2
        )

    def test_updater_has_one_release_owner_and_six_targets(self) -> None:
        os_release = (WORKFLOWS / "release.yml").read_text()
        updater_release = (WORKFLOWS / "release-snapdog-update.yml").read_text()

        self.assertNotIn("build-update-tool:", os_release)
        self.assertNotIn("publish-update-tool:", os_release)
        self.assertNotIn("Update Homebrew tap", os_release)
        self.assertIn('tags: ["snapdog-update-v*"]', updater_release)
        self.assertNotIn("workflow_dispatch:", updater_release)
        self.assertIn('if [ "$SOURCE_SHA" != "$GITHUB_SHA" ]', updater_release)
        self.assertIn('ref: ${{ needs.meta.outputs.source_sha }}', updater_release)
        self.assertIn('RUST_TOOLCHAIN: "1.88.0"', updater_release)
        self.assertIn("scripts/package_updater_release.py", updater_release)
        self.assertIn("--latest=false", updater_release)
        self.assertNotIn("--clobber", updater_release)
        self.assertIn("releases?per_page=100", updater_release)
        self.assertNotIn(
            'gh api "repos/${GITHUB_REPOSITORY}/releases/tags/',
            updater_release,
        )
        self.assertIn("overwrite: true", updater_release)
        self.assertIn("refusing split ownership", updater_release)

        targets = set(re.findall(r"^\s+- target: (\S+)$", updater_release, re.MULTILINE))
        self.assertEqual(
            targets,
            {
                "x86_64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
                "x86_64-apple-darwin",
                "aarch64-apple-darwin",
                "x86_64-pc-windows-msvc",
                "aarch64-pc-windows-msvc",
            },
        )

    def test_root_package_excludes_standalone_updater(self) -> None:
        config = json.loads((ROOT / "release-please-config.json").read_text())
        excluded = set(config["packages"]["."]["exclude-paths"])

        self.assertGreaterEqual(
            excluded,
            {
                "snapdog-update/**",
                "docs/snapdog-update-release-flow.md",
                ".github/workflows/release-snapdog-update.yml",
            },
        )
        for package in (".", "snapdog-ctrl", "snapdog-update"):
            self.assertIs(config["packages"][package]["draft"], True)
            self.assertIs(
                config["packages"][package]["force-tag-creation"], True
            )

    def test_r2_publication_is_leased_immutable_and_legacy_safe(self) -> None:
        os_release = (WORKFLOWS / "release.yml").read_text()
        retention = (WORKFLOWS / "retention.yml").read_text()
        catalog = (WORKFLOWS / "publish-installer-catalog.yml").read_text()

        for workflow in (os_release, retention, catalog):
            self.assertIn("tools/r2_lock.py acquire", workflow)
            self.assertIn("tools/r2_lock.py renew", workflow)
            self.assertIn("tools/r2_lock.py release", workflow)
            self.assertNotIn("group: r2-maintenance", workflow)
        self.assertIn("tools/r2_repair_aliases.py $APPLY", retention)
        self.assertIn("environment: r2-maintenance", retention)
        self.assertIn("s3api head-object", catalog)
        self.assertIn("expected_image_size", catalog)
        self.assertIn("--bundle-base-url", os_release)
        self.assertIn("sleep 310", os_release)
        self.assertIn("sleep 70", os_release)
        self.assertNotIn("--clobber", os_release)
        cutover = os_release[os_release.index("- name: Commit public R2 metadata") :]
        sleep_alias = cutover.index("sleep 310")
        beta_pointer = cutover.index(
            '"s3://${R2_BUCKET}/os/images/latest-beta.json"', sleep_alias
        )
        beta_visibility_wait = cutover.index("sleep 70", beta_pointer)
        active_pointer = cutover.index(
            '"s3://${R2_BUCKET}/os/images/latest-${CHANNEL}.json"',
            beta_visibility_wait,
        )
        active_visibility_wait = cutover.index("sleep 70", active_pointer)
        self.assertLess(cutover.index("snapdog-alias-transition"), sleep_alias)
        self.assertLess(sleep_alias, beta_pointer)
        self.assertLess(beta_pointer, beta_visibility_wait)
        self.assertLess(beta_visibility_wait, active_pointer)
        self.assertLess(active_pointer, active_visibility_wait)
        self.assertIn('steps.cutover.outcome == \'failure\'', os_release)
        recovery = os_release[
            os_release.index("- name: Recover compatibility aliases") :
            os_release.index("- name: Publish verified GitHub releases")
        ]
        self.assertIn("tools/r2_repair_aliases.py", recovery)
        self.assertLess(
            os_release.index("- name: Attest verified artifacts"),
            os_release.index("- name: Commit public R2 metadata"),
        )
        self.assertLess(
            os_release.index("- name: Prepare and verify GitHub release draft"),
            os_release.index("- name: Commit public R2 metadata"),
        )
        self.assertLess(
            os_release.index("- name: Publish verified GitHub releases"),
            os_release.index("- name: Prune superseded betas from R2"),
        )
        self.assertLess(
            os_release.index("- name: Publish verified GitHub releases"),
            os_release.index("- name: Release R2 mutation lock"),
        )
        self.assertIn("candidate_key >= semver_key(beta)", os_release)


if __name__ == "__main__":
    unittest.main()
