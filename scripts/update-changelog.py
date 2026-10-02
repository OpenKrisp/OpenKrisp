#!/usr/bin/env python3
"""Synchronize CHANGELOG.md with published GitHub releases using the Contents API."""

import base64
import json
import os
import subprocess

HEADER = """# Changelog

Automatically synchronized from published GitHub releases, newest first.
Draft releases are excluded; prereleases are included.
"""


def api(endpoint, payload=None, paginate=False):
    command = ["gh", "api", endpoint]
    if paginate:
        command += ["--paginate", "--slurp"]
    if payload is not None:
        command += ["--method", "PUT", "--input", "-"]
    result = subprocess.run(command, input=json.dumps(payload) if payload else None,
                            capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


def render(releases):
    published = [r for r in releases if not r["draft"] and r.get("published_at")]
    published.sort(key=lambda r: (r["published_at"], r["id"]), reverse=True)
    sections = [HEADER.rstrip()]
    for release in published:
        tag = release["tag_name"].replace("\n", " ").replace("\r", " ")
        date = release["published_at"][:10]
        kind = " (prerelease)" if release["prerelease"] else ""
        body = (release.get("body") or "").strip() or "No release notes provided."
        sections.append(f"## [{tag}]({release['html_url']}) — {date}{kind}\n\n{body}")
    return "\n\n".join(sections) + "\n"


def synchronize(repo):
    branch = api(f"repos/{repo}")["default_branch"]
    # Read the current file and SHA before listing releases; concurrent writes
    # fail rather than overwriting another update with stale release data.
    listing = api(f"repos/{repo}/contents?ref={branch}")
    entry = next((item for item in listing if item["name"] == "CHANGELOG.md"), None)
    current = api(f"repos/{repo}/contents/CHANGELOG.md?ref={branch}") if entry else None
    pages = api(f"repos/{repo}/releases?per_page=100", paginate=True)
    content = render([release for page in pages for release in page])
    if current and base64.b64decode(current["content"]).decode("utf-8") == content:
        print("Changelog already matches published releases.")
        return
    payload = {"message": "docs: synchronize changelog with published releases",
               "content": base64.b64encode(content.encode("utf-8")).decode("ascii"),
               "branch": branch}
    if current:
        payload["sha"] = current["sha"]
    api(f"repos/{repo}/contents/CHANGELOG.md", payload)
    print(f"Updated CHANGELOG.md on {branch}.")


if __name__ == "__main__":
    synchronize(os.environ["GITHUB_REPOSITORY"])
