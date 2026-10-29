import ast
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("repo_format", ROOT / "scripts/format.py")
formatter = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = formatter
spec.loader.exec_module(formatter)


class FormatterRegressionTests(unittest.TestCase):
    def read_lf_source(self, name):
        source = (ROOT / name).read_bytes()
        self.assertNotIn(b"\r", source, f"{name} must use LF before evaluation")
        return source.decode("utf-8")

    def test_newlines_check_is_read_only_and_fix_preserves_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "sample.bzl"
            original = b'cmd = """\r\necho hello\r\n"""\r\n'
            path.write_bytes(original)
            with mock.patch.object(formatter, "REPO_ROOT", root):
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(
                        formatter.bazel_newlines([path.name], check=True), 1
                    )
                self.assertEqual(path.read_bytes(), original)
                self.assertEqual(formatter.bazel_newlines([path.name], check=False), 0)
                self.assertEqual(path.read_bytes(), original.replace(b"\r\n", b"\n"))
                self.assertEqual(formatter.bazel_newlines([path.name], check=True), 0)
                path.write_bytes(b'cmd = "a\rb"\n')
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(
                        formatter.bazel_newlines([path.name], check=False), 1
                    )
                self.assertEqual(path.read_bytes(), b'cmd = "a\rb"\n')

    def test_formatter_checks_before_and_after_buildifier(self):
        with mock.patch.object(
            formatter.subprocess, "check_output", return_value=b"x.bzl\0"
        ):
            for check in (False, True):
                group = formatter.buildifier_formatter_group(check=check)
                self.assertEqual(len(group.commands), 3)
                self.assertEqual(
                    group.commands[0].args[2:4],
                    ("--bazel-newlines", "check" if check else "fix"),
                )
                self.assertIn(
                    "-mode=check" if check else "-mode=fix", group.commands[1].args
                )
                self.assertEqual(
                    group.commands[2].args[2:4], ("--bazel-newlines", "check")
                )

    def test_formatter_normalizes_before_buildifier_and_rejects_crlf_output(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sample.bzl"
            path.write_bytes(b'cmd = """\r\necho hi\r\n"""\r\n')
            with mock.patch.object(
                formatter.subprocess,
                "check_output",
                return_value=os.fsencode(path) + b"\0",
            ):
                group = formatter.buildifier_formatter_group(check=False)
            verifier = formatter.Command(
                (
                    sys.executable,
                    "-c",
                    "import pathlib, sys; p = pathlib.Path(sys.argv[1]); assert b'\\r' not in p.read_bytes()",
                    str(path),
                )
            )
            result = formatter.run_formatter_group(
                formatter.FormatterGroup(
                    group.name, (group.commands[0], verifier, group.commands[2])
                )
            )
            self.assertEqual(result.returncode, 0, result.output)
            self.assertNotIn(b"\r", path.read_bytes())
            corruptor = formatter.Command(
                (
                    sys.executable,
                    "-c",
                    "import pathlib, sys; p = pathlib.Path(sys.argv[1]); p.write_bytes(p.read_bytes().replace(b'\\n', b'\\r\\n'))",
                    str(path),
                )
            )
            result = formatter.run_formatter_group(
                formatter.FormatterGroup(
                    group.name, (group.commands[0], corruptor, group.commands[2])
                )
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("uses CRLF", result.output)

    def test_release_and_rbe_generate_usable_build_files(self):
        builds = []

        def record_platform(**kwargs):
            builds.append(kwargs)

        class Rctx:
            os = type("OS", (), {"arch": "x86_64"})()

            def file(self, name, content, **kwargs):
                self.name, self.content = name, content

        namespace = {"repository_rule": lambda **kwargs: None}
        exec(self.read_lf_source("rbe.bzl"), namespace)
        ctx = Rctx()
        namespace["_rbe_platform_repo_impl"](ctx)
        self.assertEqual(ctx.name, "BUILD.bazel")
        exec(ctx.content, {"platform": record_platform})
        self.assertEqual(builds[0]["exec_properties"]["Arch"], "amd64")
        self.assertIn("@platforms//cpu:x86_64", builds[0]["constraint_values"])
        namespace = {
            "repository_rule": lambda **kwargs: None,
            "attr": type("Attr", (), {"string": staticmethod(lambda **kwargs: None)})(),
        }
        exec(
            self.read_lf_source("bazel/extensions/codex_release_archive.bzl"), namespace
        )
        generated = namespace["_CODEX_RELEASE_BUILD_FILE"].format(
            entrypoint=json.dumps("bin/codex"),
            binaries=json.dumps("bin/**"),
            resources=json.dumps("resources/**"),
            path=json.dumps("path/**"),
        )
        groups = []
        exec(
            generated,
            {
                "package": lambda **kwargs: None,
                "filegroup": lambda **kwargs: groups.append(kwargs),
                "glob": lambda paths: paths,
            },
        )
        self.assertEqual(
            groups,
            [
                {"name": "codex", "srcs": ["bin/codex"]},
                {
                    "name": "package",
                    "srcs": ["codex-package.json", "bin/**", "resources/**", "path/**"],
                },
            ],
        )
        self.assertNotIn("\r", ctx.content + generated)

    def test_v8_shell_command_continues_and_produces_output(self):
        bash = shutil.which("bash")
        git_bash = (
            Path(os.environ.get("ProgramFiles", "C:/Program Files"))
            / "Git/bin/bash.exe"
        )
        if os.name == "nt" and git_bash.exists():
            bash = str(git_bash)
        if not bash:
            self.skipTest("bash is unavailable")
        source = self.read_lf_source("third_party/v8/BUILD.bazel")
        literal = re.search(r'cmd = (""".*?""") % \(arch, libc\)', source, re.S).group(
            1
        )
        command = ast.literal_eval(literal) % ("x86_64", "gnu")
        command = re.sub(r"\$\(location @rusty_v8_[^)]*\)", "archive", command)
        command = command.replace(
            "$(location @llvm//tools:llvm-objcopy)", "objcopy_mock"
        )
        command = command.replace(
            "$(location :gnu_libcxx_shared_exception_symbols.txt)", "symbols"
        )
        command = command.replace("$@", "output.a")
        self.assertNotIn("\r", command)
        with tempfile.TemporaryDirectory() as directory:
            script = (
                'set -eu\ngzip() { printf payload; }\nobjcopy_mock() { cp "$2" "$3"; }\n'
                + command
                + '\ntest "$(cat output.a)" = payload\ntest ! -e output.a.unadjusted\n'
            )
            subprocess.run(
                [bash, "-c", script], cwd=directory, check=True, capture_output=True
            )


if __name__ == "__main__":
    unittest.main()
