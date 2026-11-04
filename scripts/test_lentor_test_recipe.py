import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class TestRecipeIsolation(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Windows recipe harness")
    def test_hostile_inherited_home_is_replaced(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as temporary:
            tmp = Path(temporary)
            fake_bin = tmp / "bin"
            fake_bin.mkdir()
            hostile = tmp / "hostile"
            hostile.mkdir()
            marker = hostile / "marker"
            marker.write_text("untouched")
            capture = tmp / "capture.json"
            probe = fake_bin / "probe.py"
            probe.write_text(
                "import json, os, sys\n"
                "from pathlib import Path\n"
                "Path(os.environ['LENTOR_HARNESS_CAPTURE']).write_text(json.dumps({"
                "'home': os.environ.get('LENTOR_HOME'), "
                "'sqlite': os.environ.get('LENTOR_SQLITE_HOME'), "
                "'secret': os.environ.get('VercelAPI'), 'args': sys.argv[1:]}))\n"
            )
            (fake_bin / "cargo.cmd").write_text(
                f'@"{os.sys.executable}" "{probe}" %*\r\n'
            )
            env = os.environ.copy()
            env.update(
                {
                    "PATH": str(fake_bin)
                    + os.pathsep
                    + str(Path(os.sys.executable).parent)
                    + os.pathsep
                    + env["PATH"],
                    "LENTOR_HOME": str(hostile),
                    "LENTOR_SQLITE_HOME": str(hostile),
                    "VercelAPI": "hostile",
                    "LENTOR_HARNESS_CAPTURE": str(capture),
                }
            )
            just = Path(env["USERPROFILE"]) / ".cargo/bin/just.exe"
            subprocess.run(
                [
                    str(just),
                    "--justfile",
                    str(root / "justfile"),
                    "test",
                    "-p",
                    "fixture",
                ],
                cwd=root,
                env=env,
                check=True,
            )
            result = json.loads(capture.read_text())
            self.assertNotEqual(Path(result["home"]), hostile)
            self.assertTrue(Path(result["home"]).is_dir())
            self.assertFalse(result["sqlite"])
            self.assertFalse(result["secret"])
            self.assertEqual(
                result["args"], ["nextest", "run", "--no-fail-fast", "-p", "fixture"]
            )
            self.assertEqual(marker.read_text(), "untouched")


if __name__ == "__main__":
    unittest.main()
