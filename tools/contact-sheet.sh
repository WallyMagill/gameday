#!/usr/bin/env bash
# Montage a directory of PNGs into one labeled contact sheet, so a design
# sitting is ONE image to open instead of N.
#
# Usage: tools/contact-sheet.sh <dir-of-pngs> [out.png] [columns]
#   defaults: out = <dir>/contact-sheet.png, columns = 3
#
# Each cell is labeled with the file's stem — that is the whole point: the
# filename is how an option gets referred to in the menu you present.
set -euo pipefail

dir=${1:-}
if [ -z "$dir" ] || [ ! -d "$dir" ]; then
  echo "contact-sheet: expected a directory of PNGs, got ${dir:-<nothing>}" >&2
  echo "usage: tools/contact-sheet.sh <dir-of-pngs> [out.png] [columns]" >&2
  exit 2
fi
out=${2:-$dir/contact-sheet.png}
cols=${3:-3}

if command -v magick >/dev/null 2>&1; then
  montage=(magick montage)
elif command -v montage >/dev/null 2>&1; then
  montage=(montage)
else
  echo "contact-sheet: needs ImageMagick (no 'magick' or 'montage' on PATH); brew install imagemagick" >&2
  exit 1
fi

# The sheet itself is never an input to the next sheet.
shopt -s nullglob
pngs=()
for f in "$dir"/*.png; do
  [ "$(basename "$f")" = "$(basename "$out")" ] && continue
  pngs+=("$f")
done
if [ ${#pngs[@]} -eq 0 ]; then
  echo "contact-sheet: no PNGs in $dir" >&2
  exit 1
fi

# ImageMagick installed without a fontconfig set (the Homebrew default on
# this machine) reports no fonts at all and dies on `-label` with "unable to
# read font ''". Naming a font FILE sidesteps the lookup entirely.
font=()
for candidate in \
  /System/Library/Fonts/Supplemental/Arial.ttf \
  /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf \
  /Library/Fonts/Arial.ttf
do
  if [ -f "$candidate" ]; then font=(-font "$candidate"); break; fi
done

"${montage[@]}" \
  "${font[@]}" \
  -label '%t' \
  -background '#101014' -fill '#e8e8ec' -bordercolor '#303038' -border 1 \
  -pointsize 18 -geometry '600x+12+12' -tile "${cols}x" \
  "${pngs[@]}" "$out"

echo "wrote $out (${#pngs[@]} frames, ${cols} across)"
