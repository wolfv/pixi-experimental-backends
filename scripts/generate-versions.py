"""Generate version environment variables from Cargo.toml for the rattler-build recipe."""

import re
import tomllib
from pathlib import Path

BACKENDS = {
    "pixi_build_meson": "crates/pixi_build_meson/Cargo.toml",
    "pixi_build_autotools": "crates/pixi_build_autotools/Cargo.toml",
}


def main():
    repo_root = Path(__file__).parent.parent

    for name, cargo_path in BACKENDS.items():
        cargo_toml = repo_root / cargo_path
        with open(cargo_toml, "rb") as f:
            data = tomllib.load(f)
        version = data["package"]["version"]
        env_name = name.replace("-", "_").upper() + "_VERSION"
        print(f"{env_name}={version}")


if __name__ == "__main__":
    main()
