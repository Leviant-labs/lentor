"""Print directly changed workspace crates for scoped CI tests."""

import os
import subprocess
import sys
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parent.parent
env = os.environ.copy()
env.pop("VercelAPI", None)
changed = subprocess.check_output(
    ["git", "diff", "--name-only", "--no-renames", sys.argv[1], sys.argv[2]],
    cwd=root,
    text=True,
    env=env,
).splitlines()
crates = set()
for name in changed:
    path = root / name
    if not name.startswith("codex-rs/"):
        continue
    for parent in (path.parent, *path.parents):
        if parent == root / "codex-rs":
            break
        manifest = parent / "Cargo.toml"
        if manifest.is_file():
            with manifest.open("rb") as stream:
                package = tomllib.load(stream).get("package", {}).get("name")
            if package:
                crates.add(package)
            break
    else:
        continue
    if path.parent == root / "codex-rs":
        crates.update(
            (
                "codex-cli",
                "codex-core",
                "codex-protocol",
                "codex-app-server",
                "codex-tui",
            )
        )
print(" ".join(sorted(crates)))
