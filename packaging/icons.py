"""Makes the Windows .ico and macOS .icns from a large PNG of the app icon.

Usage: python3 packaging/icons.py <icon-1024.png> <out-dir>
Render the PNG from assets/kinetto.svg first, e.g. with
`rsvg-convert -w 1024 -h 1024 assets/kinetto.svg -o icon-1024.png`.
"""

import sys
from pathlib import Path

from PIL import Image


def main() -> None:
    source, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    icon = Image.open(source).convert("RGBA")
    if icon.size != (1024, 1024):
        sys.exit(f"{source} is {icon.size}, expected 1024x1024")
    icon.save(out / "kinetto.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    icon.save(out / "kinetto.icns")
    print(f"wrote {out / 'kinetto.ico'} and {out / 'kinetto.icns'}")


if __name__ == "__main__":
    main()
