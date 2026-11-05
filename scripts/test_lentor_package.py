import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import lentor_package
import lentor_smoke


class PackageTests(unittest.TestCase):
    def test_prebuilt_package_is_reproducible_and_checksummed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = root / "inputs"
            inputs.mkdir()
            for name in lentor_package.NAMES:
                (inputs / name).write_bytes(name.encode())
            first, second = root / "one.zip", root / "two.zip"
            lentor_package.assemble(inputs, first, "0.1-alpha")
            lentor_package.assemble(inputs, second, "0.1-alpha")
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with zipfile.ZipFile(first) as archive:
                manifest = json.loads(archive.read("manifest.json"))
                self.assertEqual(manifest["target"], "x86_64-pc-windows-msvc")
                self.assertEqual(archive.read("bin/lentor.exe"), b"lentor.exe")
                self.assertEqual(
                    archive.read("bin/codex-app-server.exe"), b"codex-app-server.exe"
                )
                self.assertEqual(
                    archive.read("codex-resources/codex-command-runner.exe"),
                    b"codex-command-runner.exe",
                )
                for line in archive.read("SHA256SUMS").decode().splitlines():
                    digest, name = line.split("  ", 1)
                    self.assertEqual(
                        hashlib.sha256(archive.read(name)).hexdigest(), digest
                    )
                self.assertEqual(
                    manifest["files"]["bin/lentor.exe"],
                    hashlib.sha256(b"lentor.exe").hexdigest(),
                )

    def test_missing_input_and_existing_output_fail_without_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with self.assertRaisesRegex(ValueError, "Missing prebuilt input"):
                lentor_package.assemble(root, root / "out.zip", "alpha")
            output = root / "out.zip"
            output.write_bytes(b"keep")
            with self.assertRaisesRegex(ValueError, "already exists"):
                lentor_package.assemble(root, output, "alpha")
            self.assertEqual(output.read_bytes(), b"keep")

    def test_offline_smoke_uses_isolated_homes_and_removes_credential(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in lentor_package.NAMES:
                (root / name).write_bytes(name.encode())
            output = root / "package.zip"
            lentor_package.assemble(root, output, "alpha")
            calls = []

            def run(command, **kwargs):
                calls.append((command, kwargs))
                codex_home = Path(kwargs["env"]["CODEX_HOME"])
                self.assertEqual(
                    (codex_home / "auth.json").read_bytes(),
                    b'{"OPENAI_API_KEY":"fake-smoke-key"}\n',
                )
                self.assertEqual(
                    (codex_home / "config.toml").read_bytes(), b"invalid toml = [\n"
                )
                self.assertTrue(
                    (codex_home / "sessions/2026/01/01/rollout-smoke.jsonl").is_file()
                )
                if command[-2:] == ["login", "status"]:
                    self.assertEqual(command[-3:], ["--no-daemon", "login", "status"])
                    self.assertEqual(
                        list(Path(kwargs["env"]["LENTOR_HOME"]).iterdir()), []
                    )
                    return subprocess.CompletedProcess(
                        command, 1, stdout=b"", stderr=b"Not logged in\n"
                    )
                result = (
                    '{"id":1,"result":{}}\n'
                    if command[-1] == "app-server"
                    else b"Lentor"
                )
                return subprocess.CompletedProcess(command, 0, stdout=result)

            with (
                patch.dict(
                    os.environ,
                    {
                        "VercelAPI": "test-placeholder",
                        "OPENAI_API_KEY": "fake",
                        "LENTOR_API_KEY": "fake",
                        "ANTHROPIC_API_KEY": "fake",
                        "LENTOR_ACCESS_TOKEN": "fake",
                        "CODEX_ACCESS_TOKEN": "fake",
                    },
                ),
                patch.dict(
                    os.environ,
                    {"LENTOR_SQLITE_HOME": "fake-db", "CODEX_SQLITE_HOME": "fake-db"},
                ),
                patch("lentor_smoke.subprocess.run", side_effect=run),
            ):
                lentor_smoke.smoke(output)
            self.assertEqual(len(calls), 5)
            for _, kwargs in calls:
                self.assertNotIn("VercelAPI", kwargs["env"])
                self.assertNotIn("LENTOR_ACCESS_TOKEN", kwargs["env"])
                self.assertNotIn("CODEX_ACCESS_TOKEN", kwargs["env"])
                self.assertNotIn("LENTOR_SQLITE_HOME", kwargs["env"])
                self.assertNotIn("CODEX_SQLITE_HOME", kwargs["env"])
                self.assertFalse(
                    any(key.upper().endswith("_API_KEY") for key in kwargs["env"])
                )
                self.assertNotEqual(
                    kwargs["env"]["LENTOR_HOME"], kwargs["env"]["CODEX_HOME"]
                )


if __name__ == "__main__":
    unittest.main()
