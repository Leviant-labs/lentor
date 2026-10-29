"""Exercise the launcher's profile isolation and argument/exit-code forwarding."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class LentorLauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="lentor-launcher-")
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        self.home = self.folder / "lentor home"
        self.env = os.environ.copy()
        self.env.pop("VercelAPI", None)
        self.env["CODEX_HOME"] = str(self.folder / "existing codex home")
        self.env["LENTOR_HOME"] = str(self.home)
        if os.name == "nt":
            shell = shutil.which("pwsh")
            if shell is None:
                self.skipTest("PowerShell is required to exercise the Windows launcher")
            engine = self.folder / "engine.ps1"
            engine.write_text(
                "@{home = $env:CODEX_HOME; argv = @($args)} | ConvertTo-Json -Compress\n"
                'if ($args -contains "--fail") { exit 17 }\nexit 0\n',
                encoding="utf-8",
            )
            self.command = [shell, "-NoProfile", "-File", str(ROOT / "lentor.ps1")]
        else:
            engine = self.folder / "engine"
            engine.write_text(
                '#!/bin/sh\nprintf \'%s\\n\' "$CODEX_HOME" "$@"\n'
                'for arg do [ "$arg" != "--fail" ] || exit 17; done\nexit 0\n',
                encoding="utf-8",
            )
            engine.chmod(0o700)
            self.command = ["sh", str(ROOT / "lentor")]
        self.env["LENTOR_BINARY"] = str(engine)

    def run_launcher(self, *args):
        return subprocess.run(
            [*self.command, *args],
            env=self.env,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )

    def test_isolates_home_and_preserves_arguments(self):
        result = self.run_launcher("--help", "argument with spaces")
        self.assertEqual(result.returncode, 0, result.stderr)
        if os.name == "nt":
            output = json.loads(result.stdout)
            home, args = output["home"], output["argv"]
        else:
            home, *args = result.stdout.splitlines()
        self.assertEqual(Path(home), self.home)
        self.assertEqual(args, ["--no-daemon", "--help", "argument with spaces"])
        self.assertTrue(self.home.is_dir())
        self.assertFalse(Path(self.env["CODEX_HOME"]).exists())

    def test_preserves_engine_exit_code(self):
        result = self.run_launcher("--fail")
        self.assertEqual(result.returncode, 17, result.stderr)

    def test_short_flags_reach_engine(self):
        result = self.run_launcher("-V", "-h")
        self.assertEqual(result.returncode, 0, result.stderr)
        if os.name == "nt":
            args = json.loads(result.stdout)["argv"]
        else:
            args = result.stdout.splitlines()[1:]
        self.assertEqual(args, ["--no-daemon", "-V", "-h"])


if __name__ == "__main__":
    unittest.main()
