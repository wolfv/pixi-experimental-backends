"""Generate version environment variables for pixi-experimental-backends CI."""

import json
import subprocess
import tomllib
from datetime import datetime
from pathlib import Path


def get_git_short_hash() -> str:
    """Get the short git hash of the current HEAD."""
    result = subprocess.run(
        ["git", "rev-parse", "--short=7", "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def main():
    repo_root = Path(__file__).parent.parent

    # Generate version suffix
    now = datetime.now()
    date_suffix = now.strftime("%Y%m%d")
    time_suffix = now.strftime("%H%M")
    git_hash = get_git_short_hash()
    version_suffix = f"{date_suffix}.{time_suffix}.{git_hash}"

    # Get Rust package versions from cargo metadata
    result = subprocess.run(
        ["cargo", "metadata", "--format-version=1", "--no-deps"],
        capture_output=True,
        text=True,
        check=True,
        cwd=repo_root,
    )
    cargo_metadata = json.loads(result.stdout)

    env_vars = {}

    rust_packages = [
        "pixi_build_meson",
        "pixi_build_autotools",
    ]
    for package in cargo_metadata.get("packages", []):
        if package["name"] in rust_packages:
            env_name = package["name"].replace("-", "_").upper() + "_VERSION"
            env_vars[env_name] = f"{package['version']}.{version_suffix}"

    for name, value in env_vars.items():
        print(f"{name}={value}")


if __name__ == "__main__":
    main()
