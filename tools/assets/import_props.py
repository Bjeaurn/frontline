"""Imports prop models from a raw asset zip into crates/frontline/assets/models/<pack>/.

Each model becomes one `.glb` (geometry in the binary chunk), and every texture is downscaled to
at most --max-texture pixels and re-encoded (JPEG, or PNG when it has transparency). Textures a
pack shares between its models (a modular kit uses a handful for all 150 pieces) are written once
to `<pack>/textures/` and referenced by relative URI, so Bevy loads each image once; pass --embed
to put them inside every .glb instead (for single-model packs).

    # glTF pieces named on the command line:
    python3 tools/assets/import_props.py --zip "content/assets/Downtown City MegaKit[Standard].zip" \\
        --pack downtown Building_Small_1 Street_4Lane

    # Exactly the pieces the built-in maps use (scans `prop` entities' `model` values for
    # models/<pack>/*.glb):
    python3 tools/assets/import_props.py --zip ... --pack downtown --from-maps \\
        crates/frontline/assets/maps/*.map --clean

    # The Homage Maps' own copy, in the private repo (their models are private://models/<pack>/*.glb):
    python3 tools/assets/import_props.py --zip ... --pack downtown --from-maps \\
        content/private/maps/*.map --prefix private://models --out content/private/models --clean

Accepted source formats inside the zip, in order of preference: .gltf (+ .bin and images),
.glb, .fbx (converted with `assimp export x.fbx x.glb`; its textures are embedded). Needs
ImageMagick (`magick`) for the textures and, for FBX, `assimp`. Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

REPO = Path(__file__).resolve().parents[2]
MODELS_DIR = REPO / "crates" / "frontline" / "assets" / "models"
SOURCE_EXTENSIONS = (".gltf", ".glb", ".fbx")

GLB_MAGIC = 0x46546C67
CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942


class Gltf:
    """A glTF document: its JSON and the single binary buffer its accessors read."""

    def __init__(self, document: dict, binary: bytes):
        self.document = document
        self.binary = binary

    def to_glb(self) -> bytes:
        document = dict(self.document)
        binary = self.binary + b"\0" * (-len(self.binary) % 4)
        document["buffers"] = [{"byteLength": len(binary)}]
        text = json.dumps(document, separators=(",", ":")).encode("utf-8")
        text += b" " * (-len(text) % 4)
        total = 12 + 8 + len(text) + 8 + len(binary)
        return b"".join(
            [
                struct.pack("<III", GLB_MAGIC, 2, total),
                struct.pack("<II", len(text), CHUNK_JSON),
                text,
                struct.pack("<II", len(binary), CHUNK_BIN),
                binary,
            ]
        )


def read_glb(data: bytes) -> Gltf:
    magic, _version, length = struct.unpack_from("<III", data)
    if magic != GLB_MAGIC:
        raise ValueError("not a GLB file")
    document, binary, offset = None, b"", 12
    while offset < length:
        size, kind = struct.unpack_from("<II", data, offset)
        chunk = data[offset + 8 : offset + 8 + size]
        if kind == CHUNK_JSON:
            document = json.loads(chunk)
        elif kind == CHUNK_BIN:
            binary = bytes(chunk)
        offset += 8 + size
    return Gltf(document, binary)


def read_gltf(archive: zipfile.ZipFile, member: str) -> Gltf:
    """A `.gltf` from the zip, with its `.bin` pulled into the binary buffer and external image
    files turned into `data` the caller can find through `image_bytes`."""
    document = json.loads(archive.read(member))
    folder = PurePosixPath(member).parent
    buffers = document.get("buffers", [])
    if len(buffers) != 1:
        raise ValueError(f"{member}: expected exactly one buffer, found {len(buffers)}")
    binary = archive.read(str(folder / buffers[0]["uri"]))
    for image in document.get("images", []):
        if "uri" in image:
            image["_zip"] = str(folder / image["uri"])
    return Gltf(document, binary)


def run(*command: str, stdin: bytes | None = None) -> bytes:
    result = subprocess.run(command, input=stdin, capture_output=True)
    if result.returncode != 0:
        sys.exit(f"{' '.join(command)} failed: {result.stderr.decode(errors='replace')}")
    return result.stdout


def has_alpha(data: bytes) -> bool:
    """Whether an image has any pixel that is not fully opaque."""
    opaque = run("magick", "-", "-format", "%[opaque]", "info:", stdin=data).decode().strip()
    return opaque.lower() != "true"


def downscale(data: bytes, max_size: int, quality: int) -> tuple[bytes, str, str]:
    """The image resized to fit max_size px (never enlarged): (bytes, extension, mime type)."""
    resize = f"{max_size}x{max_size}>"
    if has_alpha(data):
        return run("magick", "-", "-resize", resize, "-strip", "png:-", stdin=data), "png", "image/png"
    return (
        run("magick", "-", "-resize", resize, "-strip", "-quality", str(quality), "jpg:-", stdin=data),
        "jpg",
        "image/jpeg",
    )


def image_quality(name: str) -> int:
    # Colour tolerates more compression than the data maps (normals, roughness/metal).
    return 82 if "basecolor" in name.lower() or "albedo" in name.lower() else 90


def image_source(gltf: Gltf, image: dict, archive: zipfile.ZipFile | None) -> bytes:
    if "bufferView" in image:
        view = gltf.document["bufferViews"][image["bufferView"]]
        start = view.get("byteOffset", 0)
        return gltf.binary[start : start + view["byteLength"]]
    if "_zip" in image and archive is not None:
        return archive.read(image["_zip"])
    raise ValueError(f"cannot read image {image.get('name') or image}")


def compact(gltf: Gltf) -> None:
    """Rebuilds the binary buffer from only the buffer views accessors still use."""
    document = gltf.document
    used: list[int] = []
    for accessor in document.get("accessors", []):
        if "bufferView" in accessor:
            used.append(accessor["bufferView"])
        sparse = accessor.get("sparse")
        if sparse:
            used += [sparse["indices"]["bufferView"], sparse["values"]["bufferView"]]
    for image in document.get("images", []):
        if "bufferView" in image:
            used.append(image["bufferView"])
    order = sorted(set(used))
    remap = {old: new for new, old in enumerate(order)}
    binary, views = bytearray(), []
    for old in order:
        view = dict(document["bufferViews"][old])
        binary += b"\0" * (-len(binary) % 4)
        start = view.get("byteOffset", 0)
        view["byteOffset"] = len(binary)
        view["buffer"] = 0
        binary += gltf.binary[start : start + view["byteLength"]]
        views.append(view)
    for accessor in document.get("accessors", []):
        if "bufferView" in accessor:
            accessor["bufferView"] = remap[accessor["bufferView"]]
        sparse = accessor.get("sparse")
        if sparse:
            sparse["indices"]["bufferView"] = remap[sparse["indices"]["bufferView"]]
            sparse["values"]["bufferView"] = remap[sparse["values"]["bufferView"]]
    for image in document.get("images", []):
        if "bufferView" in image:
            image["bufferView"] = remap[image["bufferView"]]
    document["bufferViews"] = views
    gltf.binary = bytes(binary)


def convert(gltf: Gltf, archive: zipfile.ZipFile | None, textures_dir: Path | None, max_size: int) -> bytes:
    """A .glb of `gltf` with every image downscaled; shared files in textures_dir (or embedded when
    it is None)."""
    sources = [image_source(gltf, image, archive) for image in gltf.document.get("images", [])]
    pending: list[tuple[dict, bytes, str]] = []
    for image, data in zip(gltf.document.get("images", []), sources):
        stem = Path(image.get("_zip") or image.get("name") or "image").stem
        small, extension, mime = downscale(data, max_size, image_quality(stem))
        image.pop("_zip", None)
        image.pop("bufferView", None)
        image.pop("uri", None)
        image["mimeType"] = mime
        if textures_dir is not None:
            textures_dir.mkdir(parents=True, exist_ok=True)
            (textures_dir / f"{stem}.{extension}").write_bytes(small)
            image["uri"] = f"textures/{stem}.{extension}"
        else:
            pending.append((image, small, mime))
    compact(gltf)
    if pending:
        binary = bytearray(gltf.binary)
        views = gltf.document.setdefault("bufferViews", [])
        for image, data, _mime in pending:
            binary += b"\0" * (-len(binary) % 4)
            views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data)})
            image["bufferView"] = len(views) - 1
            binary += data
        gltf.binary = bytes(binary)
    return gltf.to_glb()


def find_member(archive: zipfile.ZipFile, name: str) -> str:
    members = [m for m in archive.namelist() if not m.endswith("/") and PurePosixPath(m).stem == name]
    for extension in SOURCE_EXTENSIONS:
        for member in members:
            if member.lower().endswith(extension):
                return member
    sys.exit(f"{name}: no {'/'.join(SOURCE_EXTENSIONS)} in the zip")


def import_model(archive: zipfile.ZipFile, name: str, out: Path, embed: bool, max_size: int) -> int:
    member = find_member(archive, name)
    textures_dir = None if embed else out / "textures"
    suffix = PurePosixPath(member).suffix.lower()
    if suffix == ".gltf":
        glb = convert(read_gltf(archive, member), archive, textures_dir, max_size)
    elif suffix == ".glb":
        glb = convert(read_glb(archive.read(member)), None, textures_dir, max_size)
    else:
        if shutil.which("assimp") is None:
            sys.exit("assimp is needed for FBX (brew install assimp)")
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / f"{name}.fbx"
            source.write_bytes(archive.read(member))
            target = Path(tmp) / f"{name}.glb"
            run("assimp", "export", str(source), str(target))
            glb = convert(read_glb(target.read_bytes()), None, textures_dir, max_size)
    (out / f"{name}.glb").write_bytes(glb)
    return len(glb)


def models_in_maps(paths: list[str], pack: str, prefix: str) -> list[str]:
    pattern = re.compile(rf'"model"\s+"{re.escape(prefix)}/{re.escape(pack)}/([^"/]+)\.glb"')
    names: set[str] = set()
    for path in paths:
        names.update(pattern.findall(Path(path).read_text(encoding="utf-8")))
    return sorted(names)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--zip", required=True, type=Path, help="the asset pack zip")
    parser.add_argument("--pack", required=True, help="folder name under assets/models/")
    parser.add_argument("--max-texture", type=int, default=1024, help="longest texture side in px")
    parser.add_argument("--embed", action="store_true", help="embed textures in each .glb")
    parser.add_argument("--from-maps", nargs="+", metavar="MAP", help=".map files whose prop models to import")
    parser.add_argument("--clean", action="store_true", help="delete the pack's folder first")
    parser.add_argument("--out", type=Path, default=MODELS_DIR, help="models folder")
    parser.add_argument("--prefix", default="models", help="what --from-maps model paths start with (private://models for a private folder)")
    parser.add_argument("pieces", nargs="*", help="model names (file stem) to import")
    args = parser.parse_args()

    names = list(args.pieces)
    if args.from_maps:
        names += models_in_maps(args.from_maps, args.pack, args.prefix)
    names = sorted(set(names))
    if not names:
        sys.exit("nothing to import: name pieces or pass --from-maps")

    out = args.out / args.pack
    if args.clean:
        shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True, exist_ok=True)
    total = 0
    with zipfile.ZipFile(args.zip) as archive:
        for name in names:
            total += import_model(archive, name, out, args.embed, args.max_texture)
    textures = sum(f.stat().st_size for f in (out / "textures").glob("*")) if (out / "textures").exists() else 0
    print(f"{len(names)} models, {total / 1e6:.1f} MB of .glb + {textures / 1e6:.1f} MB shared textures -> {out}")


if __name__ == "__main__":
    main()
