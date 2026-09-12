"""Integrity and publication checks; no Snow installation or network required."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "snow_locked", Path(__file__).resolve().parents[1] / "install_snow_locked.py"
)
snow = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(snow)


class LockedInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.cache = self.root / "cache"
        self.cache.mkdir()
        self.destination = self.root / "project" / "lib"
        self.original = b"locked archive bytes"
        self.sha = hashlib.sha256(self.original).hexdigest()
        self.package = dict(
            name="example", version="1.0", url="https://example.invalid/example.tgz", sha256=self.sha
        )
        self.lock = dict(format=1, libraries=["(example)"], packages=[self.package])
        self.archive = self.cache / f"{self.sha}.tgz"

    def test_offline_missing_archive_never_downloads_or_publishes(self):
        with patch.object(snow.shutil, "which", return_value="snow-chibi"), \
             patch.object(snow.urllib.request, "urlopen") as network, \
             patch.object(snow, "run_snow") as installer:
            with self.assertRaisesRegex(snow.InstallError, "missing from cache"):
                snow.install(self.lock, self.cache, self.destination, False, "snow-chibi")
            network.assert_not_called()
            installer.assert_not_called()
        self.assertFalse(self.destination.exists())

    def test_corrupt_cache_is_not_silently_refetched(self):
        self.archive.write_bytes(b"changed upstream or local bytes")
        with patch.object(snow.urllib.request, "urlopen") as network:
            with self.assertRaisesRegex(snow.InstallError, "checksum mismatch"):
                snow.obtain_archive(self.package, self.cache, True)
            network.assert_not_called()
        self.assertEqual(self.archive.read_bytes(), b"changed upstream or local bytes")

    def test_wrong_download_is_not_cached(self):
        response = io.BytesIO(b"not the locked archive")
        response.geturl = lambda: self.package["url"]
        with patch.object(snow.urllib.request, "urlopen", return_value=response):
            with self.assertRaisesRegex(snow.InstallError, "checksum mismatch"):
                snow.obtain_archive(self.package, self.cache, True)
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_existing_install_is_untouched(self):
        self.destination.mkdir(parents=True)
        marker = self.destination / "original.sld"
        marker.write_text("keep this version")
        with patch.object(snow, "obtain_archive") as download:
            with self.assertRaisesRegex(snow.InstallError, "already exists"):
                snow.install(self.lock, self.cache, self.destination, True, "snow-chibi")
            download.assert_not_called()
        self.assertEqual(marker.read_text(), "keep this version")

    def test_failed_snow_install_is_not_published(self):
        self.archive.write_bytes(self.original)

        def failing(command, cwd, env):
            if "index" in command:
                (cwd / "repo.scm").write_text("(repository)")
            else:
                (cwd / "lib").mkdir()
                (cwd / "lib/partial.sld").write_text("partial library")
                raise snow.InstallError("Snow failed")

        with patch.object(snow.shutil, "which", return_value="snow-chibi"), \
             patch.object(snow, "run_snow", side_effect=failing):
            with self.assertRaisesRegex(snow.InstallError, "Snow failed"):
                snow.install(self.lock, self.cache, self.destination, False, "snow-chibi")
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.destination.parent.iterdir()), [])

    def test_git_metadata_cannot_escape_offline_archive_installation(self):
        self.archive.write_bytes(self.original)

        def index_only(command, cwd, env):
            self.assertIn("index", command)
            (cwd / "repo.scm").write_text('(repository (package (git (url "https://example.invalid/repo.git"))))')

        with patch.object(snow.shutil, "which", return_value="snow-chibi"), \
             patch.object(snow, "run_snow", side_effect=index_only) as installer:
            with self.assertRaisesRegex(snow.InstallError, "archive libraries only"):
                snow.install(self.lock, self.cache, self.destination, False, "snow-chibi")
            self.assertEqual(installer.call_count, 1)
        self.assertFalse(self.destination.exists())

    def test_lock_requires_hashes_and_unique_packages(self):
        file = self.root / "lock.json"
        self.package["sha256"] = "latest"
        file.write_text(json.dumps(self.lock))
        with self.assertRaisesRegex(snow.InstallError, "SHA-256"):
            snow.read_lock(file)
        self.package["sha256"] = self.sha
        self.lock["packages"].append(dict(self.package))
        file.write_text(json.dumps(self.lock))
        with self.assertRaisesRegex(snow.InstallError, "unique"):
            snow.read_lock(file)


if __name__ == "__main__":
    unittest.main()
