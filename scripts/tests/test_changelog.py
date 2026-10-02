import base64
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "changelog", Path(__file__).resolve().parents[1] / "update-changelog.py")
changelog = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changelog)


def release(identifier=1, **overrides):
    result = dict(id=identifier, tag_name=f"v1.0.{identifier}", draft=False,
                  prerelease=False, published_at=f"2026-10-0{identifier}T12:00:00Z",
                  html_url=f"https://github.com/owner/repo/releases/tag/v1.0.{identifier}",
                  body="### Changes\n\nFixed audio.\n")
    return result | overrides


class ChangelogTests(unittest.TestCase):
    def test_published_only_newest_first_with_prereleases(self):
        text = changelog.render([release(), release(2, prerelease=True),
                                 release(3, draft=True), release(4, published_at=None)])
        self.assertLess(text.index("v1.0.2"), text.index("v1.0.1"))
        self.assertIn("2026-10-02 (prerelease)", text)
        self.assertIn("### Changes\n\nFixed audio.", text)
        self.assertNotIn("v1.0.3", text)
        self.assertNotIn("v1.0.4", text)

    def test_empty_notes_and_no_releases(self):
        self.assertIn("No release notes provided.", changelog.render([release(body=None)]))
        self.assertEqual(changelog.render([]), changelog.HEADER)

    @patch.object(changelog, "api")
    def test_unchanged_content_does_not_commit(self, api):
        content = changelog.render([release()])
        api.side_effect = [{"default_branch": "main"}, [{"name": "CHANGELOG.md"}],
                           {"sha": "old", "content": base64.b64encode(content.encode()).decode()},
                           [[release()]]]
        changelog.synchronize("owner/repo")
        self.assertEqual(api.call_count, 4)

    @patch.object(changelog, "api")
    def test_paginated_releases_commit_with_existing_sha(self, api):
        api.side_effect = [{"default_branch": "main"}, [{"name": "CHANGELOG.md"}],
                           {"sha": "old", "content": base64.b64encode(b"old").decode()},
                           [[release()], [release(2)]], {}]
        changelog.synchronize("owner/repo")
        payload = api.call_args.args[1]
        self.assertEqual(payload["sha"], "old")
        self.assertEqual(payload["branch"], "main")
        self.assertEqual(base64.b64decode(payload["content"]).decode(),
                         changelog.render([release(), release(2)]))

    @patch.object(changelog, "api")
    def test_missing_file_is_created(self, api):
        api.side_effect = [{"default_branch": "main"}, [], [[release()]], {}]
        changelog.synchronize("owner/repo")
        self.assertNotIn("sha", api.call_args.args[1])


if __name__ == "__main__":
    unittest.main()
