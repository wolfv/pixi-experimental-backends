"""Auto-bump patch versions for backends that changed since last release."""

import re
import subprocess
import sys
import tomllib
from pathlib import Path

# Map crate directory names to their Cargo.toml paths
BACKENDS = {
    "pixi_build_meson": "crates/pixi_build_meson/Cargo.toml",
    "pixi_build_autotools": "crates/pixi_build_autotools/Cargo.toml",
}


def get_changed_files(base_ref: str = "HEAD~1") -> set[str]:
    """Get files changed since base_ref."""
    result = subprocess.run(
        ["git", "diff", "--name-only", base_ref, "HEAD"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        # Fallback: if HEAD~1 doesn't exist (first commit), consider everything changed
        return {"crates/"}
    return set(result.stdout.strip().splitlines())


def backend_changed(backend_name: str, changed_files: set[str]) -> bool:
    """Check if any file in the backend's crate directory changed."""
    crate_dir = f"crates/{backend_name}/"
    # Also trigger on shared files
    shared_triggers = ["Cargo.toml", "Cargo.lock", "recipe/"]
    for f in changed_files:
        if f.startswith(crate_dir):
            return True
        for trigger in shared_triggers:
            if f.startswith(trigger) or f == trigger:
                return True
    return False


def bump_patch(version: str) -> str:
    """Bump the patch component of a semver version string."""
    match = re.match(r"^(\d+)\.(\d+)\.(\d+)(.*)$", version)
    if not match:
        raise ValueError(f"Cannot parse version: {version}")
    major, minor, patch, rest = match.groups()
    return f"{major}.{minor}.{int(patch) + 1}"


def read_cargo_version(cargo_toml: Path) -> str:
    """Read the version from a Cargo.toml file."""
    with open(cargo_toml, "rb") as f:
        data = tomllib.load(f)
    return data["package"]["version"]


def write_cargo_version(cargo_toml: Path, old_version: str, new_version: str):
    """Write a new version to a Cargo.toml file."""
    content = cargo_toml.read_text()
    new_content = content.replace(f'version = "{old_version}"', f'version = "{new_version}"', 1)
    cargo_toml.write_text(new_content)


def main():
    repo_root = Path(__file__).parent.parent

    # --force bumps all backends regardless of changes
    force = "--force" in sys.argv
    args = [a for a in sys.argv[1:] if a != "--force"]
    base_ref = args[0] if args else "HEAD~1"

    if force:
        print("Force-bumping all backends")

    changed_files = get_changed_files(base_ref)

    bumped = {}
    for backend_name, cargo_path in BACKENDS.items():
        if not force and not backend_changed(backend_name, changed_files):
            continue

        cargo_toml = repo_root / cargo_path
        old_version = read_cargo_version(cargo_toml)
        new_version = bump_patch(old_version)
        write_cargo_version(cargo_toml, old_version, new_version)
        bumped[backend_name] = (old_version, new_version)
        print(f"{backend_name}: {old_version} -> {new_version}")

    if not bumped:
        print("No backends changed, nothing to bump.")
        return

    # Update Cargo.lock to match bumped versions
    print("Updating Cargo.lock...")
    subprocess.run(
        ["cargo", "generate-lockfile"],
        cwd=repo_root,
        check=True,
    )

    # Re-read all versions (bumped or not) so the recipe always has them
    env_lines = []
    for name, cargo_path in BACKENDS.items():
        cargo_toml = repo_root / cargo_path
        version = read_cargo_version(cargo_toml)
        env_name = name.replace("-", "_").upper() + "_VERSION"
        env_lines.append(f"{env_name}={version}")

    # Write env file for CI consumption
    env_file = repo_root / ".versions.env"
    env_file.write_text("\n".join(env_lines) + "\n")
    print(f"\nWrote {env_file}:")
    print("\n".join(env_lines))


if __name__ == "__main__":
    main()
