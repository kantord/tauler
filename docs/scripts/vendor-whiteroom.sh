#!/usr/bin/env bash
# Copies "White Room, Pixel Traced" into the site, for the home page
# background (src/components/site/whiteroom/).
#
#   ./scripts/vendor-whiteroom.sh "path/to/White Room, Pixel Traced.zip"
#
# The zip is a single page built on Erich Loftis's THREE.js PathTracing
# Renderer (CC0, https://github.com/erichlof/THREE.js-PathTracing-Renderer).
# Everything that renders is copied unchanged:
#
# - The renderer's shared GLSL chunks (js/PathTracingCommon.js) and its
#   blue-noise texture are taken from upstream at a pinned commit, after
#   checking that the zip's copies are byte-identical to it. The chunks are a
#   classic script that writes onto a global THREE.ShaderChunk; they are
#   wrapped in an exported function that takes THREE, body unchanged.
# - Four of the page's five shaders are cut out of their <script> tags as
#   they are: all but its pixel pass (fs-pix), whose work the wallpaper's
#   shader does (src/components/site/wallpaper/wallpaper-gl.ts).
# - The scene description (the script that builds window.ROOM_SCENE) is
#   wrapped in an exported function with a local `window`, body unchanged
#   but for two patches, each saying why: scripts/whiteroom-scene.patch
#   (longer and taller walls, for the site's wider and taller frames, and a
#   small cove where they meet the floor and each other) and
#   scripts/whiteroom-monstera.patch (a Monstera in a pot, the scene's 8th
#   group, from the site's own monstera-scene.js). Patches rather than
#   edits, so re-vendoring keeps them, and stops here if one no longer
#   applies.
#
# three.js comes from npm at the revision the zip ships (r185). The page's
# render loop is ported by hand into whiteroom.ts, without its tweak panel and
# drag controls, which a page background has no use for.
set -euo pipefail

ZIP=${1:?usage: $0 path/to/the.zip}
COMMIT=f8ded116a3c674db552d4481e59c61d28cf81833
BASE="https://raw.githubusercontent.com/erichlof/THREE.js-PathTracing-Renderer/$COMMIT"

PUBLIC=public/whiteroom
LIB=src/components/site/whiteroom/vendor
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

unzip -q "$ZIP" -d "$WORK"
mkdir -p "$PUBLIC/shaders" "$LIB"

# Upstream files, and proof the zip did not change them.
curl -fsSL "$BASE/js/PathTracingCommon.js" -o "$WORK/upstream-common.js"
curl -fsSL "$BASE/textures/BlueNoise_R_128.png" -o "$PUBLIC/BlueNoise_R_128.png"
cmp "$WORK/upstream-common.js" "$WORK/pathtracing-common.js"
cmp "$PUBLIC/BlueNoise_R_128.png" "$WORK/bluenoise.png"

{
  echo "// js/PathTracingCommon.js from erichlof/THREE.js-PathTracing-Renderer"
  echo "// at $COMMIT (CC0). Written by scripts/vendor-whiteroom.sh: the body is"
  echo "// the original file, unchanged; only the wrapper is added."
  echo "export function installPathTracingChunks(THREE) {"
  cat "$WORK/upstream-common.js"
  echo "}"
} > "$LIB/PathTracingCommon.js"

python3 -I - "$WORK/index.html" "$PUBLIC/shaders" "$LIB/room-scene.js" <<'PY'
import re, sys
html, shaders, scene = sys.argv[1:4]
page = open(html, encoding='utf-8').read()
# A <script>'s textContent is its raw text: no entity decoding happens.
for name in ['vs', 'fs-out', 'fs-room']:
    match = re.search(r'<script id="%s"[^>]*>(.*?)</script>' % re.escape(name), page, re.S)
    if not match:
        sys.exit('shader %s not found' % name)
    open('%s/%s.glsl' % (shaders, name), 'w', encoding='utf-8').write(match.group(1))
bodies = [b for b in re.findall(r'<script>(.*?)</script>', page, re.S) if 'window.ROOM_SCENE = ' in b]
if len(bodies) != 1:
    sys.exit('expected one scene script, found %d' % len(bodies))
open(scene, 'w', encoding='utf-8').write(
    '// The scene of "White Room, Pixel Traced": the page\'s scene script, unchanged,\n'
    '// inside a wrapper that gives it a local `window`. Written by\n'
    '// scripts/vendor-whiteroom.sh.\n'
    'export function buildRoomScene() {\n'
    '  const window = {};\n'
    + bodies[0] +
    '\n  return window.ROOM_SCENE\n}\n')
PY
for change in scene monstera; do
  patch --forward --fuzz=0 --no-backup-if-mismatch --reject-file=- --quiet \
    "$LIB/room-scene.js" < "scripts/whiteroom-$change.patch"
done

cp "$WORK/licenses/PathTracing-Renderer-CC0.txt" "$LIB/LICENSE"
cp "$WORK/licenses/PathTracing-Renderer-CC0.txt" "$PUBLIC/LICENSE"

echo "vendored White Room on erichlof/THREE.js-PathTracing-Renderer@${COMMIT:0:7}"
