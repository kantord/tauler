// The scene of "White Room, Pixel Traced": the page's scene script, unchanged,
// inside a wrapper that gives it a local `window`. Written by
// scripts/vendor-whiteroom.sh.
import { addMonstera } from '../monstera-scene.js'

export function buildRoomScene() {
  const window = {};

/* Scene description for the white room, in centimetres.
   Builds a flat list of analytic primitives grouped into transformed "groups"
   (each with a local bounding box for culling). Uploaded to the GPU as a float texture. */
(function () {
  var KIND = { BOX: 0, CAPSULE: 1, CYL: 2, QUAD: 3, SPHERE: 4, LEAF: 5, CONE: 6, CLUSTER: 7 };
  var MAT = {
    WALL: 0, FLOOR: 1, CHROME: 2, TEAK: 3, TEAK_DARK: 4, GAP: 5, LEATHER: 6, STONE: 7,
    LAMPHEAD: 8, MARBLE: 9, GLASS_DARK: 10, RUG: 11, FRAME: 12, ART: 13,
    WINDOW: 14, UPLIGHT: 15, BULB: 16, BOOK_CREAM: 17, BOOK_RED: 18, CERAMIC: 19, FRINGE: 20, GROUT: 21,
    LEAF: 22, PETIOLE: 23, STEM: 24, AERIAL_ROOT: 25, POT: 26, SOIL: 27, FURL: 28
  };

  function Group(name) { this.name = name; this.prims = []; this.pos = [0, 0, 0]; this.rotY = 0; }
  Group.prototype.box = function (min, max, mat) { this.prims.push({ k: KIND.BOX, m: mat, a: min, b: max, r: 0 }); return this; };
  // centre + size helper
  Group.prototype.cbox = function (cx, cy, cz, sx, sy, sz, mat) {
    return this.box([cx - sx / 2, cy - sy / 2, cz - sz / 2], [cx + sx / 2, cy + sy / 2, cz + sz / 2], mat);
  };
  Group.prototype.capsule = function (a, b, r, mat) { this.prims.push({ k: KIND.CAPSULE, m: mat, a: a, b: b, r: r }); return this; };
  Group.prototype.cyl = function (a, b, r, mat) { this.prims.push({ k: KIND.CYL, m: mat, a: a, b: b, r: r }); return this; };
  Group.prototype.quad = function (a, b, c, d, mat) { this.prims.push({ k: KIND.QUAD, m: mat, a: a, b: b, c: c, d: d, r: 0 }); return this; };
  Group.prototype.sphere = function (c, r, mat) { this.prims.push({ k: KIND.SPHERE, m: mat, a: c, b: c, r: r }); return this; };

  // polyline with rounded corners -> list of points; consecutive points become capsules (bent steel tube)
  function rounded(pts, rad, closed) {
    var out = [], n = pts.length;
    function sub(a, b) { return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]; }
    function len(v) { return Math.hypot(v[0], v[1], v[2]); }
    function addS(a, v, s) { return [a[0] + v[0] * s, a[1] + v[1] * s, a[2] + v[2] * s]; }
    var idx = [];
    for (var i = 0; i < n; i++) idx.push(i);
    if (!closed) out.push(pts[0]);
    var start = closed ? 0 : 1, end = closed ? n : n - 1;
    for (var k = start; k < end; k++) {
      var p = pts[k], a = pts[(k - 1 + n) % n], b = pts[(k + 1) % n];
      var din = sub(p, a), dout = sub(b, p), li = len(din), lo = len(dout);
      var r = Math.min(rad, li * 0.49, lo * 0.49);
      var s = addS(p, din, -r / li), e = addS(p, dout, r / lo);
      var STEPS = 5;
      for (var j = 0; j <= STEPS; j++) {
        var t = j / STEPS, u = 1 - t;
        out.push([u * u * s[0] + 2 * u * t * p[0] + t * t * e[0], u * u * s[1] + 2 * u * t * p[1] + t * t * e[1], u * u * s[2] + 2 * u * t * p[2] + t * t * e[2]]);
      }
    }
    if (closed) out.push(out[0]); else out.push(pts[n - 1]);
    return out;
  }
  Group.prototype.tube = function (pts, r, bend, mat, closed) {
    var p = rounded(pts, bend, closed);
    for (var i = 0; i < p.length - 1; i++) this.capsule(p[i], p[i + 1], r, mat);
    return this;
  };

  function catmull(pts, n) { // pts: [[z,y],...] -> n+1 samples
    var out = [];
    for (var i = 0; i <= n; i++) {
      var f = i / n * (pts.length - 1), k = Math.min(Math.floor(f), pts.length - 2), t = f - k;
      var p0 = pts[Math.max(k - 1, 0)], p1 = pts[k], p2 = pts[k + 1], p3 = pts[Math.min(k + 2, pts.length - 1)];
      var t2 = t * t, t3 = t2 * t;
      out.push([0, 1].map(function (c) {
        return 0.5 * ((2 * p1[c]) + (-p0[c] + p2[c]) * t + (2 * p0[c] - 5 * p1[c] + 4 * p2[c] - p3[c]) * t2 + (-p0[c] + 3 * p1[c] - 3 * p2[c] + p3[c]) * t3);
      }));
    }
    return out;
  }

  var groups = [];
  function G(name) { var g = new Group(name); groups.push(g); return g; }

  /* ---------------- room ---------------- */
  var WALL_X = 176, WIN_Y0 = 313, WIN_Y1 = 405, CEIL = 470;
  // tauler: the site renders the room into boxes from 0.9:1 to 12:1 and wider,
  // not the page's 3:2. The back wall, window band and floor run on to LEFT_X
  // (from -1400), so a wide frame never runs out of room on the left; the
  // walls rise to CEIL (from 470), so a tall frame never sees over them.
  // Applied by scripts/vendor-whiteroom.sh from scripts/whiteroom-scene.patch.
  var LEFT_X = -5000;
  CEIL = 620;
  var room = G('room');
  room.box([LEFT_X, -20, -60], [WALL_X + 30, 0, 1500], MAT.FLOOR);                 // floor slab
  room.box([LEFT_X, 0, -40], [WALL_X, WIN_Y0, 0], MAT.WALL);                       // back wall, lower
  room.box([LEFT_X, WIN_Y1, -40], [WALL_X, CEIL, 0], MAT.WALL);                    // above window
  room.box([WALL_X, 0, -40], [WALL_X + 30, CEIL, 1500], MAT.WALL);                // right wall
  room.box([WALL_X - 14, WIN_Y0, -40], [WALL_X, WIN_Y1, 0], MAT.WALL);            // return at window end
  room.box([LEFT_X, WIN_Y0, -14], [WALL_X - 14, WIN_Y1, -12], MAT.GROUT);         // sill & frame backing (behind glow)
  room.quad([LEFT_X, WIN_Y0, -11], [WALL_X - 14, WIN_Y0, -11], [WALL_X - 14, WIN_Y1, -11], [LEFT_X, WIN_Y1, -11], MAT.WINDOW);
  // window reveal (top and bottom faces of the opening)
  room.box([LEFT_X, WIN_Y0 - 1, -40], [WALL_X - 14, WIN_Y0, 0], MAT.WALL);
  // tauler: a cove, from scripts/whiteroom-scene.patch
  // cove: a concave fillet of radius COVE_R (3 cm) where the walls meet the floor and each other,
  // in the walls' paint; it meets both planes tangentially, so no edge forms (kind COVE, the
  // inside of a cylinder between its axis and the corner, in the room shader)
  KIND.COVE = 8;
  var COVE_R = 3;
  function cove(a, b, e1, e2) { room.prims.push({ k: KIND.COVE, m: MAT.WALL, a: a, b: b, c: e1, d: e2, r: COVE_R }); }
  cove([LEFT_X, COVE_R, COVE_R], [WALL_X, COVE_R, COVE_R], [0, -1, 0], [0, 0, -1]);                                 // back wall's foot
  cove([WALL_X - COVE_R, COVE_R, 0], [WALL_X - COVE_R, COVE_R, 1500], [1, 0, 0], [0, -1, 0]);                   // right wall's foot
  cove([WALL_X - COVE_R, 0, COVE_R], [WALL_X - COVE_R, CEIL, COVE_R], [1, 0, 0], [0, 0, -1]);                   // the corner between them

  /* ---------------- sideboard ---------------- */
  var sb = G('sideboard'); sb.pos = [-22, 0, 27];
  (function () {
    var W = 200, D = 46, LEG = 15, H = 60, TOP = 3;
    sb.cbox(0, LEG - 2.5, 0, W - 30, 5, D - 10, MAT.TEAK_DARK);
    [-1, 1].forEach(function (sx) { [-1, 1].forEach(function (sz) {
      sb.cyl([sx * (W / 2 - 20), 0, sz * (D / 2 - 8)], [sx * (W / 2 - 20), LEG, sz * (D / 2 - 8)], 1.9, MAT.TEAK_DARK);
    }); });
    sb.cbox(0, LEG + H / 2, -0.5, W - 1, H, D - 1, MAT.GAP);
    sb.cbox(-W / 2 + 1.1, LEG + H / 2, 0, 2.2, H, D, MAT.TEAK);
    sb.cbox(W / 2 - 1.1, LEG + H / 2, 0, 2.2, H, D, MAT.TEAK);
    sb.cbox(0, LEG + H + TOP / 2, 0.5, W + 2, TOP, D + 2, MAT.TEAK);
    sb.cbox(0, LEG + 0.9, 0, W, 1.8, D, MAT.TEAK);
    var fz = D / 2 + 0.6, fy0 = LEG + 2.4, fy1 = LEG + H - 1.2, fh = fy1 - fy0, doorW = 60;
    [-1, 1].forEach(function (s) {
      var x = s * (W / 2 - 2.6 - doorW / 2);
      sb.cbox(x, (fy0 + fy1) / 2, fz, doorW, fh, 2, MAT.TEAK);
      var px = x - s * (doorW / 2 - 10);
      sb.cbox(px, (fy0 + fy1) / 2 + 7, fz + 2.2, 2.8, 22, 3, MAT.TEAK);
      sb.cbox(px - s * 1.2, (fy0 + fy1) / 2 + 17, fz + 2.2, 5, 2.4, 3, MAT.TEAK);
    });
    var midW = W - 2 * (2.6 + doorW) - 2.4;
    for (var i = 0; i < 3; i++) {
      var dh = (fh - 2) / 3, y = fy0 + dh / 2 + i * (dh + 1);
      sb.cbox(0, y, fz, midW, dh - 0.9, 2, MAT.TEAK);
    }
  })();

  /* ---------------- framed print (standing on the sideboard, against the wall) ---------------- */
  (function () {
    var FW = 64, FH = 82, B = 2.2, cx = 32, y0 = 78, z0 = 3, z1 = 6.5;
    room.box([cx - FW / 2, y0, z0], [cx + FW / 2, y0 + B, z1], MAT.FRAME);
    room.box([cx - FW / 2, y0 + FH - B, z0], [cx + FW / 2, y0 + FH, z1], MAT.FRAME);
    room.box([cx - FW / 2, y0, z0], [cx - FW / 2 + B, y0 + FH, z1], MAT.FRAME);
    room.box([cx + FW / 2 - B, y0, z0], [cx + FW / 2, y0 + FH, z1], MAT.FRAME);
    room.box([cx - FW / 2 + B, y0 + B, z0], [cx + FW / 2 - B, y0 + FH - B, z0 + 1.5], MAT.FRAME);
    var z = z0 + 1.6;
    room.quad([cx - FW / 2 + B, y0 + B, z], [cx + FW / 2 - B, y0 + B, z], [cx + FW / 2 - B, y0 + FH - B, z], [cx - FW / 2 + B, y0 + FH - B, z], MAT.ART);
  })();

  /* ---------------- floor uplighter ---------------- */
  var lampInfo = {};
  (function () {
    var bx = -161, bz = 30;
    room.cyl([bx, 0, bz], [bx, 3.5, bz], 17, MAT.STONE);
    room.tube([[bx, 3, bz], [bx, 204, bz], [bx + 12, 204.5, bz - 2]], 0.95, 4, MAT.CHROME);
    var hx = bx + 24, hy = 205, hz = bz - 4;
    room.cbox(hx, hy, hz, 30, 2.5, 17, MAT.LAMPHEAD);
    room.cbox(hx, hy - 1.4, hz, 30, 0.4, 17, MAT.CHROME);
    var ey = hy + 1.3;
    room.quad([hx - 13, ey, hz - 6.5], [hx - 13, ey, hz + 6.5], [hx + 13, ey, hz + 6.5], [hx + 13, ey, hz - 6.5], MAT.UPLIGHT);
    lampInfo = { c0: [hx - 13, ey, hz - 6.5], e1: [26, 0, 0], e2: [0, 0, 13], n: [0, 1, 0] };
    // cable across the floor
    var cab = [[bx, 0.5, bz], [bx - 20, 0.4, bz + 30], [bx - 55, 0.4, bz + 55], [bx - 70, 0.4, bz + 90], [bx - 100, 0.4, bz + 105], [bx - 150, 0.4, bz + 95], [bx - 158.6, 0.4, bz + 87.38], [bx - 164.7, 0.4, bz + 72.82], [bx - 168.9, 0.4, bz + 53.69], [bx - 172.1, 0.4, bz + 32.33], [bx - 175, 0.4, bz + 11.1], [bx - 178.3, 0.4, bz - 7.66], [bx - 182.7, 0.4, bz - 21.6], [bx - 189, 0.45, bz - 27.5], [bx - 308, 0.45, bz - 27.5], [bx - 308, 1.16, bz - 28.84], [bx - 308, 3, bz - 29.6], [bx - 308, 18, bz - 29.6]];   // tauler: runs along the cove at the wall's foot, behind the planter, up to a socket
    for (var i = 0; i < cab.length - 1; i++) room.capsule(cab[i], cab[i + 1], 0.4, MAT.LAMPHEAD);
  })();

  /* ---------------- chrome candle lamp ---------------- */
  var bulbs = [];
  var candle = G('candle'); candle.pos = [122, 0, 50]; candle.rotY = -0.3;
  (function () {
    candle.cyl([0, 0, 0], [0, 2, 0], 17, MAT.CHROME);
    var layout = [[0, 0, 92], [-7, 2, 78], [7, 2, 78], [-13, 4, 62], [13, 4, 62], [-4, -6, 70], [5, -6, 66], [-17, -2, 46], [17, -2, 46], [0, 8, 50], [-10, -8, 42], [10, -8, 40]];
    layout.forEach(function (L) {
      var x = L[0], z = L[1], h = L[2];
      candle.cyl([x, 2, z], [x, h + 2, z], 3, MAT.CHROME);
      candle.cyl([x, h + 2, z], [x, h + 3.2, z], 1.6, MAT.CHROME);
      candle.sphere([x, h + 5.2, z], 1.5, MAT.BULB);
      bulbs.push([x, h + 5.2, z]);
    });
  })();

  /* ---------------- lounge chairs (generic tubular-steel sling chairs) ---------------- */
  function chair(name, px, pz, rot) {
    var g = G(name); g.pos = [px, 0, pz]; g.rotY = rot;
    var S = 0.92, W = 74 * S, Zf = 36 * S, Zb = -34 * S, R = 1.25;
    function v(x, y, z) { return [x, y * S, z]; }
    [-1, 1].forEach(function (s) {
      var x = s * W / 2;
      g.tube([v(x, 62, Zf - 2), v(x, 1.2 + R, Zf), v(x, 1.2 + R, Zb), v(x, 80, Zb - 14 * S)], R, 7 * S, MAT.CHROME);
      g.tube([v(x, 62, Zf - 2), v(x, 62, Zb - 6 * S)], R, 5, MAT.CHROME);
      g.tube([v(x * 0.97, 43, Zf - 1), v(x * 0.97, 37, Zb + 2)], R * 0.9, 4, MAT.CHROME);
      g.capsule(v(x, 62, 4 * S - 20 * S), v(x, 62, 4 * S + 20 * S), R + 0.6, MAT.LEATHER);
      var xs = s * (W / 2 + 0.3);
      g.quad(v(xs, 45.5, -21 * S), v(xs, 45.5, 29 * S), v(xs, 60.5, 29 * S), v(xs, 60.5, -21 * S), MAT.LEATHER);
      g.cyl(v(x, 0, Zf - 2), v(x, 1.2, Zf - 2), 1.2, MAT.LAMPHEAD);
      g.cyl(v(x, 0, Zb + 2), v(x, 1.2, Zb + 2), 1.2, MAT.LAMPHEAD);
    });
    g.tube([v(-W / 2, 43, Zf - 1), v(W / 2, 43, Zf - 1)], R, 1, MAT.CHROME);
    g.tube([v(-W / 2, 80, Zb - 14 * S), v(W / 2, 80, Zb - 14 * S)], R, 1, MAT.CHROME);
    g.tube([v(-W / 2, 1.2 + R, Zb), v(W / 2, 1.2 + R, Zb)], R, 1, MAT.CHROME);
    // seat-and-back sling: strip of quads following a curved profile, sagging across the width
    var prof = catmull([[Zf + 1, 44.5], [10 * S, 40], [-15 * S, 35.5], [-30 * S, 34], [-38 * S, 50], [-44 * S, 71], [Zb - 13.5 * S, 81.5]].map(function (p) { return [p[0], p[1] * S]; }), 18);
    var cols = 4, sw = W - 3.5, sag = 3.5;
    function pt(i, j) {
      var u = j / cols, x = (u - 0.5) * sw;
      var sg = sag * (1 - Math.pow(2 * u - 1, 2)) * Math.sin(Math.PI * i / (prof.length - 1));
      return [x, prof[i][1] - sg, prof[i][0] + sg * 0.3];
    }
    for (var i = 0; i < prof.length - 1; i++) for (var j = 0; j < cols; j++)
      g.quad(pt(i, j), pt(i, j + 1), pt(i + 1, j + 1), pt(i + 1, j), MAT.LEATHER);
    return g;
  }
  chair('chairL', -162, 142, 1.05);
  chair('chairR', 110, 140, -1.08);

  /* ---------------- tube-steel tables ---------------- */
  function table(name, px, pz, rot, w, d, h, topMat, topT) {
    var g = G(name); g.pos = [px, 0, pz]; g.rotY = rot;
    var R = 1.1;
    [-1, 1].forEach(function (s) {
      var x = s * (w / 2 - 3);
      g.tube([[x, R, -d / 2 + 2], [x, R, d / 2 - 2], [x, h - R - topT, d / 2 - 2], [x, h - R - topT, -d / 2 + 2]], R, 5, MAT.CHROME, true);
    });
    [-d / 2 + 2, d / 2 - 2].forEach(function (z) { g.capsule([-w / 2 + 3, h - R - topT, z], [w / 2 - 3, h - R - topT, z], R * 0.9, MAT.CHROME); });
    g.box([-w / 2, h - topT, -d / 2], [w / 2, h, d / 2], topMat);
    return g;
  }
  table('lowTable', -32, 155, 0.12, 110, 48, 32, MAT.GLASS_DARK, 1.2);
  var tall = table('tallTable', -2, 108, 0.05, 56, 40, 52, MAT.MARBLE, 2);
  tall.cbox(-10, 53.25, 0, 24, 2.5, 17, MAT.BOOK_CREAM);
  tall.cbox(-9, 55.5, -0.5, 21, 2, 15, MAT.BOOK_RED);
  tall.cbox(-8, 57.4, 0, 20, 1.8, 14, MAT.BOOK_CREAM);
  tall.cyl([-15, 58.3, -2], [-15, 59.1, -2], 6, MAT.CERAMIC);
  tall.cyl([-15, 59.1, -2], [-15, 64.4, -2], 3.8, MAT.CERAMIC);

  /* ---------------- rug ---------------- */
  (function () {
    var cx = -24, cz = 138, RW = 162, RD = 150;
    room.box([cx - RW / 2, 0, cz - RD / 2], [cx + RW / 2, 1.4, cz + RD / 2], MAT.RUG);
    room.box([cx - RW / 2 + 1, 0, cz + RD / 2], [cx + RW / 2 - 1, 0.7, cz + RD / 2 + 7.5], MAT.FRINGE);
    room.box([cx - RW / 2 + 1, 0, cz - RD / 2 - 7.5], [cx + RW / 2 - 1, 0.7, cz - RD / 2], MAT.FRINGE);
  })();

  /* ---------------- monstera in a planter, by the back wall (the uplighter's cable runs behind it to the wall) ---------------- */
  // tauler: an 8th group, from ../monstera-scene.js. Applied by scripts/vendor-whiteroom.sh
  // from scripts/whiteroom-monstera.patch.
  var plant = G('monstera'); plant.pos = [-425, 0, 60]; plant.rotY = 0;
  addMonstera(plant, { seed: 7, KIND: KIND, MAT: MAT });

  /* ---------------- pack ---------------- */
  function pack() {
    var prims = [], gdata = [];
    groups.forEach(function (g) {
      var start = prims.length, mn = [1e9, 1e9, 1e9], mx = [-1e9, -1e9, -1e9];
      g.prims.forEach(function (p) {
        prims.push(p);
        var pts = p.k === KIND.QUAD ? [p.a, p.b, p.c, p.d] : [p.a, p.b];
        pts.forEach(function (q) { for (var c = 0; c < 3; c++) { mn[c] = Math.min(mn[c], q[c] - p.r - 0.5); mx[c] = Math.max(mx[c], q[c] + p.r + 0.5); } });
      });
      gdata.push({ name: g.name, start: start, end: prims.length, min: mn, max: mx, pos: g.pos, rotY: g.rotY });
    });
    var W = 5, data = new Float32Array(W * prims.length * 4);
    prims.forEach(function (p, i) {
      var o = i * W * 4;
      data.set([p.k, p.m, 0, 0], o);
      data.set([p.a[0], p.a[1], p.a[2], p.r], o + 4);
      data.set([p.b[0], p.b[1], p.b[2], 0], o + 8);
      if (p.c) { data.set([p.c[0], p.c[1], p.c[2], 0], o + 12); data.set([p.d[0], p.d[1], p.d[2], 0], o + 16); }
    });
    return { data: data, count: prims.length, width: W, groups: gdata };
  }

  window.ROOM_SCENE = { pack: pack, MAT: MAT, bulbs: bulbs, lamp: lampInfo, leafMats: plant.leafMats,
    window: { c0: [LEFT_X, WIN_Y0, -11], e1: [WALL_X - 14 - LEFT_X, 0, 0], e2: [0, WIN_Y1 - WIN_Y0, 0], n: [0, 0, 1] } };
})();


  return window.ROOM_SCENE
}
