"""Verify the resolved API's WIT against the supported Pumpkin release."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile


EXPECTED_API = "0.2.1+26.3-26.51"
EXPECTED_WIT = "5aeec84177a489e920a22532e5aff2b5f9279c1b"


def run(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def main():
    root = Path(__file__).resolve().parent.parent
    metadata = json.loads(
        run("cargo", "metadata", "--locked", "--offline", "--format-version=1", cwd=root)
    )
    packages = [p for p in metadata["packages"] if p["name"] == "pumpkin-plugin-api"]
    if len(packages) != 1 or packages[0]["version"] != EXPECTED_API:
        raise SystemExit(f"Expected exactly one pumpkin-plugin-api {EXPECTED_API}")
    wit = Path(packages[0]["manifest_path"]).parent / "wit"
    if not wit.is_dir():
        raise SystemExit("The resolved API has no WIT directory")

    with tempfile.TemporaryDirectory(prefix="holograms-wit-") as directory:
        scratch = Path(directory)
        shutil.copytree(wit, scratch, dirs_exist_ok=True)
        run("git", "init", "--quiet", cwd=scratch)
        run("git", "config", "core.autocrlf", "false", cwd=scratch)
        run("git", "config", "core.safecrlf", "false", cwd=scratch)
        run("git", "add", "--all", cwd=scratch)
        actual = run("git", "write-tree", cwd=scratch)

    if actual != EXPECTED_WIT:
        raise SystemExit(f"Incompatible WIT: expected {EXPECTED_WIT}, got {actual}")
    print(f"Compatible API {EXPECTED_API}, WIT {actual}")


if __name__ == "__main__":
    main()
