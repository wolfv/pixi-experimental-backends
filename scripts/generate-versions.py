"""Generate version environment variables from Cargo.toml for the rattler-build recipe."""

import json
import subprocess
from pathlib import Path

BACKENDS = [
    "pixi_build_meson",
    "pixi_build_autotools",
]


def main():
    repo_root = Path(__file__).parent.parent

    result = subprocess.run(
        ["cargo", "metadata", "--format-version=1", "--no-deps"],
        capture_output=True,
        text=True,
        check=True,
        cwd=repo_root,
    )
    cargo_metadata = json.loads(result.stdout)

    for package in cargo_metadata.get("packages", []):
        if package["name"] in BACKENDS:
            env_name = package["name"].replace("-", "_").upper() + "_VERSION"
            print(f"{env_name}={package['version']}")


if __name__ == "__main__":
    main()
