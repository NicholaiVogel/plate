#!/usr/bin/env python3
"""Create a local Mac app and copy its transitive non-system dylibs."""
import os
import plistlib
import shutil
import subprocess
import sys
from pathlib import Path


def run(*args):
    result = subprocess.run(args, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(f"{' '.join(args)}: {result.stdout}{result.stderr}")
    return result.stdout.strip()


def bundle(binary, output):
    binary, output = Path(binary).resolve(), Path(output).resolve()
    if output.exists():
        shutil.rmtree(output)
    macos = output / "Contents/MacOS"
    frameworks = output / "Contents/Frameworks"
    macos.mkdir(parents=True)
    frameworks.mkdir()
    executable = macos / "plate"
    shutil.copy2(binary, executable)
    pkg_config = os.environ.get("PKG_CONFIG", "pkg-config")
    libdirs = [Path(run(pkg_config, "--variable=libdir", name))
               for name in ("OpenColorIO", "OpenImageIO", "libavformat")]
    queue = [(binary, executable)]
    copied = {}
    while queue:
        source, destination = queue.pop()
        for line in run("/usr/bin/otool", "-L", str(source)).splitlines()[1:]:
            dependency = line.strip().split(" (", 1)[0]
            if dependency.startswith(("/usr/lib/", "/System/Library/")):
                continue
            if dependency.startswith("@rpath/"):
                candidates = [directory / dependency[len("@rpath/"):] for directory in libdirs]
            elif dependency.startswith("@loader_path/"):
                candidates = [source.parent / dependency[len("@loader_path/"):]]
            else:
                candidates = [Path(dependency)]
            resolved = next((p.resolve() for p in candidates if p.is_file()), None)
            if resolved is None:
                raise RuntimeError(f"Cannot resolve {dependency} required by {source}")
            if resolved == source.resolve():
                continue  # otool also lists a dylib's own install name.
            name = Path(dependency).name
            if name in copied and copied[name] != resolved:
                raise RuntimeError(f"Conflicting libraries named {name}")
            if name not in copied:
                copied[name] = resolved
                library = frameworks / name
                shutil.copy2(resolved, library)
                run("/usr/bin/install_name_tool", "-id", f"@rpath/{name}", str(library))
                queue.append((resolved, library))
            run("/usr/bin/install_name_tool", "-change", dependency, f"@rpath/{name}", str(destination))
    version = run(str(binary), "--help").splitlines()[0].split()[-1]
    info = {
        "CFBundleExecutable": "plate",
        "CFBundleName": "Plate",
        "CFBundleDisplayName": "Plate",
        "CFBundleIdentifier": "org.plate.player",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
        "CFBundleDocumentTypes": [{
            "CFBundleTypeName": "Media",
            "CFBundleTypeRole": "Viewer",
            "LSHandlerRank": "Alternate",
            "LSItemContentTypes": ["public.movie", "public.image"],
            "CFBundleTypeExtensions": ["mov", "mp4", "mxf", "mkv", "avi", "webm", "exr", "dpx", "cin", "tif", "tiff", "png", "jpg", "jpeg", "jp2"],
        }],
    }
    with (output / "Contents/Info.plist").open("wb") as f:
        plistlib.dump(info, f)
    for library in frameworks.iterdir():
        run("/usr/bin/codesign", "--force", "--sign", "-", str(library))
    run("/usr/bin/codesign", "--force", "--sign", "-", str(executable))
    run("/usr/bin/codesign", "--force", "--sign", "-", str(output))
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", str(output))
    print(f"Built {output} with {len(copied)} native libraries")


if __name__ == "__main__":
    if sys.platform != "darwin" or len(sys.argv) != 3:
        sys.exit("Usage (macOS): bundle-macos.py BINARY OUTPUT.app")
    bundle(*sys.argv[1:])
