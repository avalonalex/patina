#!/usr/bin/env python3
"""Install an explicitly locked set of Snow archives into a fresh project root.

This is an offline installer by default, not a dependency resolver or updater.
Use --fetch to obtain missing archives at the URLs and SHA-256 values in the
lock file. Snow builds a local index and installs only from those archives.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request


class InstallError(Exception):
    pass


def read_lock(path: Path) -> dict:
    lock = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(lock, dict) or lock.get("format") != 1:
        raise InstallError("expected lock format 1")
    libraries = lock.get("libraries")
    if not isinstance(libraries, list) or not libraries:
        raise InstallError("lock must name at least one requested library")
    # These names are passed as individual CLI arguments, never shell code.
    name_pattern = r"\([A-Za-z0-9_+*/!?<>=.:-]+(?: [A-Za-z0-9_+*/!?<>=.:-]+)*\)"
    if any(not isinstance(x, str) or not re.fullmatch(name_pattern, x)
           for x in libraries):
        raise InstallError("library names must be parenthesized, space-separated identifiers")
    packages = lock.get("packages")
    if not isinstance(packages, list) or not packages:
        raise InstallError("lock must contain the complete archive dependency set")
    names, hashes = set(), set()
    for package in packages:
        if not isinstance(package, dict):
            raise InstallError("invalid package entry")
        name, version = package.get("name"), package.get("version")
        if not isinstance(name, str) or not name or name in names:
            raise InstallError("package names must be nonempty and unique")
        if not isinstance(version, str) or not version:
            raise InstallError(f"{name}: missing version")
        url, digest = package.get("url"), package.get("sha256")
        if not isinstance(url, str) or not url.startswith("https://"):
            raise InstallError(f"{name}: archive URL must use HTTPS")
        if not isinstance(digest, str) or not re.fullmatch(r"[a-f0-9]{64}", digest):
            raise InstallError(f"{name}: expected a lowercase SHA-256 of the compressed archive")
        if digest in hashes:
            raise InstallError("duplicate archive in lock")
        names.add(name)
        hashes.add(digest)
    return lock


def digest_file(path: Path) -> str:
    with path.open("rb") as source:
        digest = hashlib.sha256()
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def obtain_archive(package: dict, cache: Path, fetch: bool) -> Path:
    digest = package["sha256"]
    archive = cache / f"{digest}.tgz"
    if archive.exists():
        if digest_file(archive) != digest:
            raise InstallError(f"{package['name']}: cached archive checksum mismatch; refusing to install")
        return archive
    if not fetch:
        raise InstallError(f"{package['name']}: archive missing from cache; use --fetch explicitly")
    cache.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=cache, delete=False) as output:
        pending = Path(output.name)
        try:
            with urllib.request.urlopen(package["url"], timeout=60) as response:
                if not response.geturl().startswith("https://"):
                    raise InstallError("archive download redirected away from HTTPS")
                shutil.copyfileobj(response, output)
            output.close()
            if digest_file(pending) != digest:
                raise InstallError(f"{package['name']}: downloaded archive checksum mismatch")
            os.replace(pending, archive)
        finally:
            pending.unlink(missing_ok=True)
    return archive


def run_snow(command: list[str], cwd: Path, env: dict[str, str]) -> None:
    try:
        result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True, timeout=120)
    except subprocess.TimeoutExpired as error:
        raise InstallError("Snow command exceeded 120 seconds") from error
    if result.returncode:
        raise InstallError(f"Snow failed ({result.returncode}):\n{result.stdout}{result.stderr}")
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)


def install(lock: dict, cache: Path, destination: Path, fetch: bool, snow: str) -> None:
    if destination.exists() or destination.is_symlink():
        raise InstallError(f"destination already exists: {destination}; install updates into a new root")
    executable = shutil.which(snow)
    if not executable:
        raise InstallError(f"Snow executable not found: {snow}")
    executable = str(Path(executable).resolve())
    # Verify the entire closure before any install begins. Never consult a
    # live index to repair an incomplete lock or silently select a new version.
    archives = [obtain_archive(package, cache, fetch) for package in lock["packages"]]
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".snow-install-", dir=destination.parent) as tmp:
        work = Path(tmp)
        local = work / "archives"
        local.mkdir()
        for archive in archives:
            shutil.copyfile(archive, local / archive.name)
            # Verify the actual staged bytes too, even if a shared cache was
            # replaced between the initial verification and the copy.
            if digest_file(local / archive.name) != archive.stem:
                raise InstallError("archive changed while staging")
        config = work / "config.scm"
        config.write_text('((input-history "history.scm"))\n', encoding="utf-8")
        env = dict(os.environ)
        for key in ("CHIBI", "CHIBI_MODULE_PATH", "SNOW_SCRIPT"):
            env.pop(key, None)
        env["SNOW_CHIBI_CONFIG"] = str(config)
        repo = work / "repo.scm"
        run_snow([executable, "--always-no", "index", str(repo),
                  *map(str, sorted(local.glob("*.tgz")))], work, env)
        # Snow can follow package-level git sources even in a local index.
        # Restrict this wrapper to archive libraries so offline really means
        # no resolver-selected downloads, programs, or installed data. The
        # index is serialized by Snow, so declaration heads are normalized.
        # A matching example inside a description is conservatively rejected.
        if re.search(r"\((git|program|data-files)(?=\s|\))", repo.read_text(encoding="utf-8")):
            raise InstallError(
                "locked installation supports archive libraries only (no git, programs or data-files)"
            )
        staged = work / "lib"
        run_snow([
            executable, "--implementations", "generic", "--always-no",
            "--repo", str(repo), "--local-user-repository", str(work / "cache"),
            "--local-root-repository", str(work / "cache"),
            "--install-library-dir", str(staged),
            "--install-binary-dir", str(work / "bin"),
            "--install-data-dir", str(work / "data"),
            "install", "--skip-tests", "--use-sudo", "never", *lock["libraries"],
        ], work, env)
        if not staged.is_dir() or not any(staged.rglob("*.sld")):
            raise InstallError("Snow produced no R7RS libraries")
        if any((work / name).exists() for name in ("bin", "data")):
            raise InstallError("this workflow supports source libraries only, not installed programs or data")
        # Snow records absolute paths in its own metadata. This root is owned
        # by the lock, not by subsequent `snow upgrade` calls; retain our own
        # location-independent provenance and inventory instead.
        for meta in staged.rglob(".*.meta"):
            meta.unlink()
        if any(path.is_symlink() or (
            path.is_file() and path.suffix not in (".sld", ".scm", ".sls", ".sch", ".ss")
        ) for path in staged.rglob("*")):
            raise InstallError("Snow produced links or non-source files; expected a self-contained source tree")
        inventory = {
            str(path.relative_to(staged)): digest_file(path)
            for path in sorted(staged.rglob("*")) if path.is_file()
        }
        (staged / ".snow-lock.json").write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")
        (staged / ".snow-files.json").write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
        if destination.exists() or destination.is_symlink():
            raise InstallError("destination appeared during installation; refusing to replace it")
        staged.rename(destination)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lock", type=Path)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--dest", type=Path, required=True, help="new project library root")
    parser.add_argument("--fetch", action="store_true", help="download missing locked archives")
    parser.add_argument("--snow", default="snow-chibi", help="Snow executable (tested with Chibi 0.12)")
    args = parser.parse_args()
    try:
        install(read_lock(args.lock), args.cache.resolve(), args.dest.absolute(), args.fetch, args.snow)
    except (InstallError, OSError, ValueError) as error:
        parser.exit(1, f"Error: {error}\n")
    print(f"Installed locked libraries in {args.dest}")


if __name__ == "__main__":
    main()
