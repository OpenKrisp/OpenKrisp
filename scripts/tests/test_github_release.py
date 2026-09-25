import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("github_release", ROOT / "scripts/github-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class GitHubReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = "example/OpenKrisp"
        self.tag, self.version = "v0.1.0-beta.2", "0.1.0-beta.2"
        self.environment = {
            "GITHUB_REPOSITORY": self.repo, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1",
            "GITHUB_SHA": "b" * 40, "RELEASE_CI_RUN_ID": "100", "GITHUB_REF": "refs/heads/main",
        }
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Release test")
        self.git("config", "user.email", "release-test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")
        self.git("config", "core.hooksPath", "/dev/null")
        files = {
            "Cargo.toml": f'[workspace]\nmembers = ["crates/fixture"]\n[workspace.package]\nversion = "{self.version}"\n',
            "Cargo.lock": f'[[package]]\nname = "fixture"\nversion = "{self.version}"\n',
            "crates/fixture/Cargo.toml": '[package]\nname = "fixture"\nversion.workspace = true\n',
            "apps/desktop/package.json": json.dumps({"version": self.version}),
            "apps/desktop/package-lock.json": json.dumps({"version": self.version, "packages": {"": {"version": self.version}}}),
            "apps/desktop/src-tauri/tauri.conf.json": json.dumps({"version": self.version}),
            release.WORKFLOW: "workflow fixture\n",
            "scripts/github-release.py": "script fixture\n",
            "packaging/macos/INSTALL.md": "installation fixture\n",
            "packaging/linux/INSTALL.md": "Linux installation fixture\n",
            f"docs/releases/{self.version}.md": "notes fixture\n",
        }
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        self.commit = self.commit_files()
        self.git("update-ref", "refs/remotes/origin/main", self.commit)
        self.git("tag", self.tag)
        root_patch = patch.object(release, "ROOT", self.root)
        root_patch.start()
        self.addCleanup(root_patch.stop)

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, stderr=subprocess.PIPE, text=True).strip()

    def commit_files(self):
        self.git("add", ".")
        self.git("commit", "-m", "fixture")
        return self.git("rev-parse", "HEAD")

    def assemble(self):
        installer = self.root / "fixture.pkg"
        installer.write_bytes(b"installer fixture")
        deb, rpm = self.root / "fixture.deb", self.root / "fixture.rpm"
        deb.write_bytes(b"DEB fixture")
        rpm.write_bytes(b"RPM fixture")
        args = SimpleNamespace(tag=self.tag, commit=self.commit, directory=self.root / "assets",
                               installer=installer, deb=deb, rpm=rpm)
        with patch.dict(os.environ, self.environment):
            release.package(args)
        return args

    def metadata(self, assets):
        return {
            "draft": True, "immutable": False, "target_commitish": self.commit,
            "body": release.MARKER, "html_url": "https://example.invalid/draft",
            "assets": [{"name": p.name, "state": "uploaded", "size": p.stat().st_size,
                        "digest": "sha256:" + release.digest(p)} for p in assets.iterdir()],
        }

    def simulate_publish(self, args, *, changed=False, conclusion="success", moved=False, expired=False):
        # Only the GitHub boundary is mocked; file validation and publication ordering are real.
        remote = self.root / "remote"
        shutil.copytree(args.directory, remote)
        if changed:
            (remote / (changed if isinstance(changed, str) else "INSTALL-macos.md")).write_text("changed after preparation")
            self.checksums(remote)
        metadata = self.metadata(remote)
        published = dict(metadata, draft=False)
        calls = []

        def command(*argv, **_kwargs):
            calls.append(argv)
            if argv[:3] == ("gh", "release", "download"):
                shutil.copytree(remote, argv[argv.index("--dir") + 1])
            elif argv[:3] == ("gh", "run", "download"):
                if expired:
                    raise subprocess.CalledProcessError(1, argv)
                shutil.copytree(args.directory, argv[argv.index("--dir") + 1])
            return ""

        build = {"conclusion": conclusion, "event": "workflow_dispatch", "path": release.WORKFLOW,
                 "head_branch": "main", "head_sha": self.environment["GITHUB_SHA"]}
        with patch.object(release, "run", side_effect=command), patch.object(release, "gh", return_value=build), \
                patch.object(release, "remote_commit", return_value="f" * 40 if moved else self.commit), \
                patch.object(release, "release_for", side_effect=[metadata, metadata, published]), \
                patch("builtins.print"):
            try:
                release.publish(args, self.repo)
            except (ValueError, subprocess.CalledProcessError):
                self.assertFalse(any("--draft=false" in call for call in calls))
                raise
        return calls

    def checksums(self, directory):
        (directory / "SHA256SUMS").write_text("".join(
            f"{release.digest(p)}  {p.name}\n" for p in sorted(directory.iterdir()) if p.name != "SHA256SUMS"))

    def test_versions_and_tagged_source_must_match(self):
        self.assertEqual(release.validate_source(self.tag), (self.version, self.commit))
        path = self.root / "apps/desktop/src-tauri/tauri.conf.json"
        path.write_text('{"version":"9.0.0"}')
        bad = self.commit_files()
        self.git("update-ref", "refs/remotes/origin/main", bad)
        self.git("tag", "--force", self.tag)
        with self.assertRaisesRegex(ValueError, "versions must match"):
            release.validate_source(self.tag)

    def test_tagged_commit_outside_main_is_rejected(self):
        (self.root / "new.txt").write_text("not merged")
        self.commit_files()
        self.git("tag", "--force", self.tag)
        with self.assertRaises(subprocess.CalledProcessError):
            release.validate_source(self.tag)

    def test_invalid_tag_and_shell_metacharacters_are_rejected(self):
        for tag in ["v01.2.3", "v1.2.3-beta.01", "1.2.3", "../../main", "v1.0.0$(echo hi)", "v1.0.0+build"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.version_for(tag)
        self.assertEqual(release.version_for("v1.2.3"), "1.2.3")

    def test_package_contains_exact_git_source_and_verifiable_assets(self):
        args = self.assemble()
        provenance = release.verify_assets(args.directory, self.tag, self.commit)
        self.assertEqual(provenance["source_commit"], self.commit)
        with tarfile.open(args.directory / f"OpenKrisp-{self.version}-source.tar.gz") as archive:
            names = archive.getnames()
            self.assertNotIn(f"OpenKrisp-{self.version}/fixture.pkg", names)
            self.assertIn(f"OpenKrisp-{self.version}/Cargo.toml", names)

    def test_modified_binary_is_rejected(self):
        args = self.assemble()
        (args.directory / f"OpenKrisp-{self.version}-macos-arm64.pkg").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            release.verify_assets(args.directory, self.tag, self.commit)

    def test_missing_linux_input_prevents_assembling_partial_release(self):
        args = self.assemble()
        shutil.rmtree(args.directory)
        args.rpm.unlink()
        with patch.dict(os.environ, self.environment), self.assertRaisesRegex(ValueError, "All three"):
            release.package(args)
        self.assertFalse(args.directory.exists())

    def test_missing_linux_release_asset_is_rejected(self):
        args = self.assemble()
        (args.directory / f"OpenKrisp-{self.version}-linux-amd64.deb").unlink()
        self.checksums(args.directory)
        with self.assertRaisesRegex(ValueError, "missing"):
            release.verify_assets(args.directory, self.tag, self.commit)

    def test_linux_binary_changed_with_matching_checksums_cannot_be_published(self):
        args = self.assemble()
        with self.assertRaisesRegex(ValueError, "differ from"):
            self.simulate_publish(args, changed=f"OpenKrisp-{self.version}-linux-x86_64.rpm")

    def test_missing_extra_and_symlink_assets_are_rejected(self):
        args = self.assemble()
        guide = args.directory / "INSTALL-macos.md"
        guide.unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            release.verify_assets(args.directory, self.tag, self.commit)
        guide.symlink_to(self.root / "packaging/macos/INSTALL.md")
        with self.assertRaisesRegex(ValueError, "regular files"):
            release.verify_assets(args.directory, self.tag, self.commit)
        guide.unlink()
        guide.write_text("restored")
        (args.directory / "unexpected.txt").write_text("extra")
        with self.assertRaisesRegex(ValueError, "unexpected"):
            release.verify_assets(args.directory, self.tag, self.commit)

    def test_traversal_or_duplicate_checksum_entries_are_rejected(self):
        args = self.assemble()
        manifest = args.directory / "SHA256SUMS"
        original = manifest.read_text()
        for extra in ["a" * 64 + "  ../private\n", original.splitlines()[0] + "\n"]:
            manifest.write_text(original + extra)
            with self.assertRaisesRegex(ValueError, "Invalid or duplicate"):
                release.verify_assets(args.directory, self.tag, self.commit)

    def test_wrong_source_provenance_is_rejected_even_with_valid_checksums(self):
        args = self.assemble()
        p = args.directory / "PROVENANCE.json"
        record = json.loads(p.read_text())
        record["source_commit"] = "f" * 40
        p.write_text(json.dumps(record))
        self.checksums(args.directory)
        with self.assertRaisesRegex(ValueError, "Provenance"):
            release.verify_assets(args.directory, self.tag, self.commit)

    def test_manual_or_published_releases_cannot_be_overwritten(self):
        args = self.assemble()
        base = self.metadata(args.directory)
        for change in [{"draft": False}, {"body": "manual draft"}, {"target_commitish": "f" * 40}, {"immutable": True}]:
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "Refusing"):
                release.owned_draft(dict(base, **change), self.commit)

    def test_ci_must_succeed_for_exact_source_on_main(self):
        candidate = {"id": 12, "head_sha": self.commit, "head_branch": "main", "conclusion": "success"}
        with patch.object(release, "gh", return_value={"workflow_runs": [candidate]}):
            self.assertEqual(release.successful_ci(self.repo, self.commit), 12)
        for change in [{"head_sha": "f" * 40}, {"head_branch": "feature"}, {"conclusion": "failure"}]:
            with patch.object(release, "gh", return_value={"workflow_runs": [dict(candidate, **change)]}):
                with self.assertRaisesRegex(ValueError, "successful CI"):
                    release.successful_ci(self.repo, self.commit)

    def test_draft_lookup_searches_paginated_list_after_tag_endpoint_404(self):
        result = subprocess.CompletedProcess([], 1, "", "gh: Not Found (HTTP 404)")
        draft = {"tag_name": self.tag, "draft": True}
        with patch.object(release.subprocess, "run", return_value=result), \
                patch.object(release, "gh", return_value=[[{"tag_name": "v0.0.1"}], [draft]]) as api:
            self.assertEqual(release.release_for(self.repo, self.tag), draft)
            self.assertIn("--paginate", api.call_args.args)

    def test_api_failure_is_not_treated_as_a_missing_release(self):
        result = subprocess.CompletedProcess([], 1, "", "gh: unavailable (HTTP 500)")
        with patch.object(release.subprocess, "run", return_value=result), patch.object(release, "gh") as api:
            with self.assertRaisesRegex(RuntimeError, "500"):
                release.release_for(self.repo, self.tag)
            api.assert_not_called()

    def test_prepare_creates_a_draft_and_verifies_every_upload(self):
        args = self.assemble()
        metadata = self.metadata(args.directory)
        with patch.dict(os.environ, self.environment), patch.object(release, "run") as command, \
                patch.object(release, "remote_commit", return_value=self.commit), \
                patch.object(release, "release_for", side_effect=[None, metadata]), patch("builtins.print"):
            release.prepare(args, self.repo)
            commands = [call.args for call in command.call_args_list]
            create = next(c for c in commands if c[:3] == ("gh", "release", "create"))
            self.assertIn("--draft", create)
            self.assertIn("--verify-tag", create)
            self.assertFalse(any("--draft=false" in c for c in commands))

    def test_prepare_resumes_its_partial_draft_but_never_a_manual_one(self):
        args = self.assemble()
        complete = self.metadata(args.directory)
        partial = dict(complete, assets=complete["assets"][:1])
        with patch.dict(os.environ, self.environment), patch.object(release, "run") as command, \
                patch.object(release, "remote_commit", return_value=self.commit), \
                patch.object(release, "release_for", side_effect=[partial, complete]), patch("builtins.print"):
            release.prepare(args, self.repo)
            self.assertFalse(any(c.args[:3] == ("gh", "release", "create") for c in command.call_args_list))
        with patch.dict(os.environ, self.environment), patch.object(release, "run") as command, \
                patch.object(release, "remote_commit", return_value=self.commit), \
                patch.object(release, "release_for", return_value=dict(partial, body="manual")):
            with self.assertRaisesRegex(ValueError, "Refusing"):
                release.prepare(args, self.repo)
            command.assert_not_called()

    def test_publish_uses_verified_existing_files_without_building(self):
        calls = self.simulate_publish(self.assemble())
        edits = [call for call in calls if "--draft=false" in call]
        self.assertEqual(len(edits), 1)
        self.assertIn("--prerelease=true", edits[0])
        self.assertTrue(all(call[0] == "gh" for call in calls))

    def test_publish_rejects_coherently_modified_draft(self):
        with self.assertRaisesRegex(ValueError, "differ from"):
            self.simulate_publish(self.assemble(), changed=True)

    def test_failed_preparation_cannot_publish(self):
        with self.assertRaisesRegex(ValueError, "successful prepare"):
            self.simulate_publish(self.assemble(), conclusion="failure")

    def test_moved_tag_cannot_publish(self):
        with self.assertRaisesRegex(ValueError, "Tag moved"):
            self.simulate_publish(self.assemble(), moved=True)

    def test_expired_artifact_cannot_publish(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.simulate_publish(self.assemble(), expired=True)

    def test_incomplete_remote_upload_is_rejected(self):
        args = self.assemble()
        base = self.metadata(args.directory)
        for state in ["starter", "uploaded"]:
            metadata = copy.deepcopy(base)
            metadata["assets"][0].update(state=state, digest="sha256:" + "f" * 64)
            with self.assertRaisesRegex(ValueError, "failed verification"):
                release.verify_uploaded(metadata, args.directory)


if __name__ == "__main__":
    unittest.main()
