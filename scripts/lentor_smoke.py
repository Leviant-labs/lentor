#!/usr/bin/env python3
"""Verify a Windows package offline using an isolated temporary profile."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile


def smoke(archive_path):
    with tempfile.TemporaryDirectory(prefix="lentor-smoke-") as tmp:
        root = Path(tmp)
        package = root / "package"
        with zipfile.ZipFile(archive_path) as archive:
            for item in archive.infolist():
                destination = (package / item.filename).resolve()
                if not destination.is_relative_to(package.resolve()):
                    raise ValueError("Unsafe archive path")
            archive.extractall(package)
        checked = set()
        for line in (package / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
            digest, name = line.split("  ", 1)
            path = (package / name).resolve()
            if (
                not path.is_relative_to(package.resolve())
                or hashlib.sha256(path.read_bytes()).hexdigest() != digest
            ):
                raise ValueError(f"Checksum mismatch: {name}")
            if name in checked:
                raise ValueError(f"Duplicate checksum: {name}")
            checked.add(name)
        actual = {
            p.relative_to(package).as_posix() for p in package.rglob("*") if p.is_file()
        } - {"SHA256SUMS"}
        if checked != actual:
            raise ValueError("Checksum list does not cover all package files")
        env = {
            key: value
            for key, value in os.environ.items()
            if key.upper()
            not in (
                "VERCELAPI",
                "LENTOR_ACCESS_TOKEN",
                "CODEX_ACCESS_TOKEN",
                "LENTOR_SQLITE_HOME",
                "CODEX_SQLITE_HOME",
            )
            and not key.upper().endswith("_API_KEY")
        }
        env["LENTOR_HOME"] = str(root / "lentor-home")
        env["CODEX_HOME"] = str(root / "codex-home")
        (root / "lentor-home").mkdir()
        (root / "codex-home").mkdir()
        codex_home = root / "codex-home"
        sentinels = {
            "config.toml": b"invalid toml = [\n",
            "auth.json": b'{"OPENAI_API_KEY":"fake-smoke-key"}\n',
            "sessions/2026/01/01/rollout-smoke.jsonl": b'{"fake":"session"}\n',
        }
        for name, contents in sentinels.items():
            path = codex_home / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)

        def check_codex_home():
            current = {
                path.relative_to(codex_home).as_posix(): path.read_bytes()
                for path in codex_home.rglob("*")
                if path.is_file()
            }
            if current != sentinels:
                raise RuntimeError("smoke command changed CODEX_HOME")

        cli = package / "bin" / "lentor.exe"
        for flag in ("--help", "--version"):
            result = subprocess.run(
                [str(cli), flag],
                env=env,
                cwd=root,
                check=True,
                capture_output=True,
                timeout=60,
            )
            check_codex_home()
            if b"lentor" not in result.stdout.lower():
                raise RuntimeError(f"{flag} did not identify Lentor")
        result = subprocess.run(
            [str(cli), "--no-daemon", "login", "status"],
            env=env,
            cwd=root,
            capture_output=True,
            timeout=60,
            check=False,
        )
        check_codex_home()
        if result.returncode != 1 or result.stderr.strip() != b"Not logged in":
            raise RuntimeError("login status did not report an unauthenticated home")
        helper = subprocess.run(
            [str(package / "bin" / "codex-code-mode-host.exe"), "--help"],
            env=env,
            cwd=root,
            capture_output=True,
            timeout=60,
            check=True,
        )
        check_codex_home()
        if not helper.stdout.strip():
            raise RuntimeError("code-mode host returned empty help")
        request = {
            "method": "initialize",
            "id": 1,
            "params": {"clientInfo": {"name": "lentor-smoke", "version": "0.1"}},
        }
        result = subprocess.run(
            [str(cli), "app-server"],
            input=json.dumps(request) + "\n",
            text=True,
            env=env,
            cwd=root,
            capture_output=True,
            timeout=60,
            check=True,
        )
        check_codex_home()
        messages = [
            json.loads(line) for line in result.stdout.splitlines() if line.strip()
        ]
        if not any(m.get("id") == 1 and "result" in m for m in messages):
            raise RuntimeError("app-server did not complete initialize")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    smoke(parser.parse_args().archive)
