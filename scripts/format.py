#!/usr/bin/env python3
"""Format repository sources or check that they are already formatted."""

import argparse
import json
import os
import shlex
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Command:
    args: tuple[str, ...]
    cwd: Path = REPO_ROOT


@dataclass(frozen=True)
class FormatterGroup:
    name: str
    commands: tuple[Command, ...]


@dataclass(frozen=True)
class FormatterResult:
    name: str
    output: str
    returncode: int


def just_formatter_group(*, check: bool) -> FormatterGroup:
    args = ["just", "--unstable", "--fmt"]
    if check:
        args.append("--check")
    return FormatterGroup("Just", (Command(tuple(args)),))


def rust_formatter_group(*, check: bool) -> FormatterGroup:
    rustfmt_args = ["--config", "imports_granularity=Item"]
    if check:
        rustfmt_args.append("--check")
    workspace = REPO_ROOT / "codex-rs"
    if os.name != "nt":
        command = Command(("cargo", "fmt", "--", *rustfmt_args), workspace)
        return FormatterGroup("Rust", (command,))

    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=workspace
        )
    )
    members = set(metadata["workspace_members"])
    batches: list[list[str]] = [[]]
    target_path_chars = 0
    for package in metadata["packages"]:
        if package["id"] not in members:
            continue
        package_path_chars = sum(
            len(target["src_path"]) + 3 for target in package["targets"]
        )
        # Cargo forwards target paths to rustfmt. Leave ample space below the
        # Windows 32K command-line limit for quoting, UTF-16, and rustfmt flags.
        if batches[-1] and target_path_chars + package_path_chars > 12_000:
            batches.append([])
            target_path_chars = 0
        batches[-1].append(package["name"])
        target_path_chars += package_path_chars
    commands = []
    for batch in batches:
        if not batch:
            continue
        args = ["cargo", "fmt"]
        for package in batch:
            args.extend(["--package", package])
        commands.append(Command((*args, "--", *rustfmt_args), workspace))
    return FormatterGroup("Rust", tuple(commands))


def buildifier_formatter_group(*, check: bool) -> FormatterGroup:
    repository_files = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=REPO_ROOT,
    ).split(b"\0")
    buildifier_files: list[str] = []
    for encoded_path in repository_files:
        if not encoded_path:
            continue
        path = Path(os.fsdecode(encoded_path))
        name = path.name
        if (
            name in {"BUILD", "WORKSPACE", "MODULE.bazel"}
            or name.startswith(("BUILD.", "WORKSPACE."))
            or name.endswith((".BUILD.bazel", ".MODULE.bazel", ".bzl", ".sky"))
            or ".bzl." in name
            or ".sky." in name
        ):
            buildifier_files.append(path.as_posix())
    buildifier_files.sort()

    # Invoke DotSlash explicitly because Windows does not honor shebangs.
    buildifier_args = [
        "dotslash",
        str(REPO_ROOT / "tools" / "buildifier"),
        "-mode=check" if check else "-mode=fix",
        "-lint=off",
        *buildifier_files,
    ]
    newline_args = [sys.executable, str(Path(__file__).resolve()), "--bazel-newlines"]
    newline_check = Command((*newline_args, "check", *buildifier_files))
    newline_fix = Command((*newline_args, "fix", *buildifier_files))
    return FormatterGroup(
        "Bazel/Starlark",
        (
            newline_check if check else newline_fix,
            Command(tuple(buildifier_args)),
            newline_check,
        ),
    )


def bazel_newlines(paths: list[str], *, check: bool) -> int:
    """Ensure buildifier never receives CRLF, which corrupts multiline strings."""
    failed = False
    for name in paths:
        path = REPO_ROOT / name
        data = path.read_bytes()
        normalized = data.replace(b"\r\n", b"\n")
        if b"\r" in normalized:
            print(
                f"{name}: contains a carriage return that is not part of CRLF",
                file=sys.stderr,
            )
            failed = True
        elif data != normalized:
            if check:
                print(f"{name}: uses CRLF instead of LF", file=sys.stderr)
                failed = True
            else:
                path.write_bytes(normalized)
    return int(failed)


def python_sdk_formatter_group(*, check: bool) -> FormatterGroup:
    # Each `--project` retains its local dependency and Ruff configuration context.
    uv_run_args = [
        "uv",
        "run",
        "--frozen",
        "--project",
        "sdk/python",
        "--only-group",
        "format",
    ]
    format_args = [
        *uv_run_args,
        "ruff",
        "format",
    ]
    if check:
        format_args.append("--check")
        # `ruff check --diff` reports lint-driven rewrites without changing files.
        # It is the check-mode counterpart of `--fix --fix-only`, not a full lint gate.
        lint_args = ["ruff", "check", "--diff"]
    else:
        # Ruff's lint fixer and formatter are separate passes: the first applies
        # fixable lint rewrites, while the second formats source layout.
        lint_args = ["ruff", "check", "--fix", "--fix-only"]

    return FormatterGroup(
        "Python SDK",
        (
            Command((*uv_run_args, *lint_args, "sdk/python")),
            Command((*format_args, "sdk/python")),
        ),
    )


def python_scripts_formatter_group(*, check: bool) -> FormatterGroup:
    args = ["uv", "run", "--frozen", "--project", "scripts", "ruff", "format"]
    if check:
        args.append("--check")
    args.append(".")
    return FormatterGroup("Python scripts", (Command(tuple(args)),))


def formatter_groups(*, check: bool) -> tuple[FormatterGroup, ...]:
    return (
        just_formatter_group(check=check),
        rust_formatter_group(check=check),
        buildifier_formatter_group(check=check),
        python_sdk_formatter_group(check=check),
        python_scripts_formatter_group(check=check),
    )


def run_formatter_group(group: FormatterGroup) -> FormatterResult:
    """Run one formatter group sequentially and return its buffered output."""
    for command in group.commands:
        try:
            process = subprocess.run(
                command.args,
                cwd=command.cwd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                check=False,
            )
        except OSError as error:
            output = f"$ {shlex.join(command.args)}\n{error}\n"
            return FormatterResult(group.name, output, 1)

        if process.returncode != 0:
            output = f"$ {shlex.join(command.args)}\n{process.stdout}"
            if process.stdout and not process.stdout.endswith("\n"):
                output += "\n"
            return FormatterResult(group.name, output, process.returncode)

    return FormatterResult(group.name, "", 0)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bazel-newlines", choices=("check", "fix"), help=argparse.SUPPRESS)
    parser.add_argument("paths", nargs="*", help=argparse.SUPPRESS)
    parser.add_argument(
        "--check",
        action="store_true",
        help="check formatting without modifying files",
    )
    args = parser.parse_args()
    if args.bazel_newlines:
        return bazel_newlines(args.paths, check=args.bazel_newlines == "check")
    if args.paths:
        parser.error("paths are only supported with --bazel-newlines")
    groups = formatter_groups(check=args.check)

    failures: list[str] = []
    with ThreadPoolExecutor(max_workers=len(groups)) as executor:
        futures = [executor.submit(run_formatter_group, group) for group in groups]
        for future in as_completed(futures):
            result = future.result()
            if result.returncode != 0:
                failures.append(result.name)
                print(f"==> {result.name} formatter failed", file=sys.stderr)
                print(result.output, end="", file=sys.stderr)

    if failures:
        print(f"Formatting failed: {', '.join(failures)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
