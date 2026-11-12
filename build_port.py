#!/usr/bin/env python3
"""Build clean PortMaster zip package (dist/rusttune.zip), SHA-256 checksum, and artwork assets."""

from __future__ import annotations

import hashlib
import math
import struct
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def _write_png(path: Path, width: int, height: int, rgba_rows: list[bytes]) -> None:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + row for row in rgba_rows)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)


def generate_cover_art(path: Path, size: int = 256) -> None:
    """Generate a minimalist Metro Utility vinyl-disc cover PNG."""
    rows: list[bytes] = []
    cx = cy = size / 2.0
    pad = size * 0.08
    disc_r = size * 0.34
    label_r = size * 0.14
    hole_r = size * 0.045

    for y in range(size):
        row = bytearray()
        for x in range(size):
            in_box = pad <= x <= (size - pad) and pad <= y <= (size - pad)
            r = math.hypot(x - cx, y - cy)
            if not in_box:
                row.extend((9, 10, 13, 255))
            elif r <= hole_r:
                row.extend((9, 10, 13, 255))
            elif r <= label_r:
                row.extend((244, 63, 94, 255))
            elif r <= disc_r:
                ring = 28 if int(r) % 6 == 0 else 20
                row.extend((ring, ring + 2, ring + 6, 255))
            else:
                row.extend((18, 20, 26, 255))
        rows.append(bytes(row))
    _write_png(path, size, size, rows)


def ensure_lf(path: Path) -> None:
    raw = path.read_bytes().replace(b"\r\n", b"\n")
    path.write_bytes(raw)


def build_zip() -> Path:
    ensure_lf(ROOT / "RustTune.sh")
    port_dir = ROOT / "port"
    if not (port_dir / "cover.png").is_file():
        generate_cover_art(port_dir / "cover.png", 256)
    if not (port_dir / "assets" / "icon.png").is_file():
        generate_cover_art(port_dir / "assets" / "icon.png", 128)

    rust_bin = ROOT / "ytmusic-rs" / "target" / "aarch64-unknown-linux-gnu" / "release" / "rusttune"
    dest_bin = port_dir / "rusttune"
    if rust_bin.is_file():
        dest_bin.write_bytes(rust_bin.read_bytes())
    elif not dest_bin.is_file():
        raise FileNotFoundError(
            f"Missing compiled aarch64 binary at {rust_bin} or {dest_bin}. "
            "Run `cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.36` in ytmusic-rs/ first."
        )

    dist_dir = ROOT / "dist"
    dist_dir.mkdir(parents=True, exist_ok=True)
    out_zip = dist_dir / "rusttune.zip"

    with zipfile.ZipFile(out_zip, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        sh_info = zipfile.ZipInfo("RustTune.sh")
        sh_info.external_attr = (0o100755 & 0xFFFF) << 16
        zf.writestr(sh_info, (ROOT / "RustTune.sh").read_bytes())

        for p in sorted(port_dir.rglob("*")):
            if p.is_dir():
                continue
            rel = p.relative_to(port_dir)
            if rel.parts[0] in {"data", "cache"} or p.name == "ytmusic-hh":
                continue
            arc = Path("rusttune") / rel
            zi = zipfile.ZipInfo(arc.as_posix())
            perm = 0o100755 if p.name == "rusttune" else 0o100644
            zi.external_attr = (perm & 0xFFFF) << 16
            zf.writestr(zi, p.read_bytes())

    verify_zip(out_zip)
    return out_zip


def verify_zip(out_zip: Path) -> None:
    if b"\r\n" in (ROOT / "RustTune.sh").read_bytes():
        raise RuntimeError("RustTune.sh must use LF line endings for ROCKNIX")
    required = {
        "RustTune.sh",
        "rusttune/port.json",
        "rusttune/rusttune",
        "rusttune/cover.png",
        "rusttune/gameinfo.xml",
        "rusttune/assets/icon.png",
        "rusttune/config/clients.json",
    }
    with zipfile.ZipFile(out_zip, "r") as zf:
        names = set(zf.namelist())
        missing = sorted(required - names)
        if missing:
            raise RuntimeError(f"Release zip missing required entries: {missing}")


def write_sha256(out_zip: Path) -> tuple[Path, str]:
    """Write standard `<digest>  <filename>` SHA-256 checksum file next to the archive."""
    digest = hashlib.sha256(out_zip.read_bytes()).hexdigest()
    sha_path = out_zip.with_name(f"{out_zip.name}.sha256")
    sha_path.write_text(f"{digest}  {out_zip.name}\n", encoding="utf-8")
    return sha_path, digest


if __name__ == "__main__":
    out = build_zip()
    sha_file, digest = write_sha256(out)
    print(f"Built PortMaster package: {out} ({out.stat().st_size // 1024} KB)")
    print(f"SHA-256 ({sha_file.name}): {digest}")
