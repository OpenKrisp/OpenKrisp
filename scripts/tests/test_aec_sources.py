import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("build_aec", Path(__file__).resolve().parents[1] / "build-aec.py")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)


class AecSources(unittest.TestCase):
    def test_vendored_sources_match_the_reviewed_manifest(self):
        BUILD.verify_sources(BUILD.VENDOR)

    def test_tampered_source_fails_before_building(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "SHA256.json").write_text(json.dumps({"source": hashlib.sha256(b"reviewed").hexdigest()}))
            (root / "source").write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "integrity failure"):
                BUILD.verify_sources(root)

    def test_extraction_rejects_escape_paths_and_links(self):
        for name, kind in [("../escaped", tarfile.REGTYPE), ("/escaped", tarfile.REGTYPE), ("link", tarfile.SYMTYPE)]:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                archive = root / "bad.tar"
                with tarfile.open(archive, "w") as tar:
                    info = tarfile.TarInfo(name)
                    info.type = kind
                    info.linkname = "../escaped"
                    info.size = 1 if kind == tarfile.REGTYPE else 0
                    tar.addfile(info, io.BytesIO(b"x"))
                with self.assertRaisesRegex(ValueError, "Unsafe archive"):
                    BUILD.extract(archive, root / "out")
