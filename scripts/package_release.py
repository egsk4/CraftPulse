#!/usr/bin/env python3
"""Package the four CI artifacts for the versioned GitHub release."""
import hashlib
import os
from pathlib import Path
import tarfile
import tomllib
import zipfile


def main():
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    expected_tag = f"CraftPulse-v{version}"
    if os.environ.get("GITHUB_REF_NAME") != expected_tag:
        raise SystemExit(f"Release tag must match Cargo.toml: {expected_tag}")
    source = root / "release-input"
    output = root / "release-output"
    output.mkdir(exist_ok=True)
    targets = {
        "x86_64-pc-windows-msvc": ("windows-x86_64", "craftpulse.exe"),
        "x86_64-unknown-linux-gnu": ("linux-x86_64", "craftpulse"),
        "arch-x86_64": ("archlinux-x86_64", "craftpulse"),
        "aarch64-apple-darwin": ("macos-arm64", "craftpulse"),
    }
    archives = []
    for target, (platform, filename) in targets.items():
        binary = source / f"craftpulse-{target}" / filename
        if not binary.is_file():
            raise SystemExit(f"Missing compiled artifact: {binary}")
        name = f"craftpulse-v{version}-{platform}"
        if filename.endswith(".exe"):
            archive = output / f"{name}.zip"
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as package:
                package.write(binary, filename)
                package.write(root / "LICENSE", "LICENSE")
        else:
            archive = output / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as package:
                info = package.gettarinfo(binary, arcname=filename)
                info.mode = 0o755
                with binary.open("rb") as contents:
                    package.addfile(info, contents)
                package.add(root / "LICENSE", arcname="LICENSE")
        archives.append(archive)
    checksums = "".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
        for path in sorted(archives)
    )
    (output / "SHA256SUMS.txt").write_text(checksums)


if __name__ == "__main__":
    main()
