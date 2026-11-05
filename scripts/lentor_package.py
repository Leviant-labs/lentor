#!/usr/bin/env python3
"""Assemble a Windows alpha archive strictly from explicit prebuilt inputs."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
os.environ["CODEX_REPO_ROOT"] = str(ROOT)
sys.path.insert(0, str(ROOT / "scripts"))
from codex_package.layout import build_package_dir, validate_package_dir
from codex_package.targets import PackageInputs, PackageVariant, TARGET_SPECS

NAMES = (
    "lentor.exe",
    "codex-code-mode-host.exe",
    "codex-command-runner.exe",
    "codex-windows-sandbox-setup.exe",
    "codex-app-server.exe",
    "rg.exe",
)


def assemble(binaries, output, version):
    binaries = binaries.resolve()
    output = output.resolve()
    if output.exists():
        raise ValueError(f"Output already exists: {output}")
    if output.suffix.lower() != ".zip":
        raise ValueError("Output must be a .zip archive")
    for name in NAMES:
        if not (binaries / name).is_file():
            raise ValueError(f"Missing prebuilt input: {binaries / name}")
    spec = TARGET_SPECS["x86_64-pc-windows-msvc"]
    variant = PackageVariant("lentor", "lentor", "lentor")
    inputs = PackageInputs(
        binaries / NAMES[0],
        binaries / NAMES[1],
        binaries / NAMES[5],
        None,
        binaries / NAMES[2],
        binaries / NAMES[3],
    )
    with tempfile.TemporaryDirectory(prefix="lentor-package-") as tmp:
        package = Path(tmp) / "package"
        package.mkdir()
        build_package_dir(package, version, variant, spec, inputs)
        validate_package_dir(package, variant, spec)
        shutil.copyfile(binaries / NAMES[4], package / "bin" / NAMES[4])
        for name in ("LICENSE", "NOTICE", "LENTOR-SETUP.md"):
            shutil.copyfile(ROOT / name, package / name)
        files = sorted(p for p in package.rglob("*") if p.is_file())
        hashes = {
            p.relative_to(package).as_posix(): hashlib.sha256(
                p.read_bytes()
            ).hexdigest()
            for p in files
        }
        (package / "manifest.json").write_text(
            json.dumps(
                {"version": version, "target": spec.target, "files": hashes}, indent=2
            )
            + "\n",
            encoding="utf-8",
        )
        files = sorted(p for p in package.rglob("*") if p.is_file())
        (package / "SHA256SUMS").write_text(
            "".join(
                f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(package).as_posix()}\n"
                for p in files
            ),
            encoding="utf-8",
        )
        output.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(
            output, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9
        ) as archive:
            for path in sorted(p for p in package.rglob("*") if p.is_file()):
                info = zipfile.ZipInfo(
                    path.relative_to(package).as_posix(), (1980, 1, 1, 0, 0, 0)
                )
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                archive.writestr(
                    info,
                    path.read_bytes(),
                    compress_type=zipfile.ZIP_DEFLATED,
                    compresslevel=9,
                )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binaries",
        type=Path,
        required=True,
        help="Directory of explicitly prebuilt Windows x64 binaries",
    )
    parser.add_argument("--output", type=Path, required=True, help="New .zip file")
    parser.add_argument("--version", required=True, help="Version recorded in metadata")
    args = parser.parse_args()
    try:
        assemble(args.binaries, args.output, args.version)
    except (ValueError, OSError, RuntimeError) as exc:
        parser.exit(1, f"{exc}\n")
    print(args.output)


if __name__ == "__main__":
    main()
