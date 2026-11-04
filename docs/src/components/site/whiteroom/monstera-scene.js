/* Procedural Monstera deliciosa in a mid-century planter, for the White Room path tracer.
 *
 * addMonstera(group, { seed, KIND, MAT }) appends analytic primitives to a scene Group
 * (centimetres, y up, the planter standing on the floor at the group origin, the back wall
 * toward -z). Same seed, same plant. The planter is a low cylinder glazed in the chairs' ochre
 * (the room's LEATHER) on a recessed chrome plinth (its CHROME).
 *
 * The habit follows photographs of potted plants of about 1.2 m (see the README): a few rooted
 * cuttings, close together, each with a short stem whose leaves alternate sides; petioles rise
 * steeply in a loose bundle and arch over near the top; at the geniculum the blade turns out and
 * down, held 20-60 degrees below level and arching to a drooping tip. Leaves get bigger and more
 * split with each new one, so the oldest are small, whole and low and the newest large, split
 * and high; the newest of all is still a furled spike. Leaves keep off the wall behind.
 * Irregularity is seeded noise on every choice, plus a few deliberate faults: an old leaf that
 * sags, a blade turned on its petiole, a torn margin.
 *
 * New primitive kinds (the room shader's MONSTERA additions):
 *   KIND.LEAF  a = blade base (petiole junction), b = a + midribDir * length,
 *              c = lateralDir * (1 + leaf index), d = [bend, cup, twist], r = bounding pad.
 *              The blade is the height field z = L (bend X^2 + twist X Y + cup Y^2) over the
 *              plane (midribDir, lateralDir), less each strap's droop; outline, pinnate splits,
 *              fenestrations, droop and veins come from the leaf's map, baked on the GPU by
 *              the leaf-map shader from g.leafMats (one code per leaf, in order: see leafCode).
 *   KIND.CLUSTER  a box (a = min, b = max) around the next c.x primitives: a ray that misses
 *              it skips them (one cluster per leaf with its petiole, one for the planter, ...).
 *   KIND.CONE  open frustum a -> b with radii c.x -> c.y; optional annular caps:
 *              c.z = inner radius of the cap at b, d.x = inner radius of the cap at a (-1 = none).
 *              r = max radius (bounding pad).
 * Materials: MAT.LEAF, PETIOLE, STEM, AERIAL_ROOT, SOIL, FURL (and POT, which this planter
 * does not use).
 */
export function addMonstera(g, o) {
  var KIND = o.KIND,
    MAT = o.MAT
  var seed = o.seed >>> 0 || 1
  function rand() {
    // mulberry32
    seed = (seed + 0x6d2b79f5) >>> 0
    var t = seed
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
  function rnd(a, b) {
    return a + (b - a) * rand()
  }
  function add(a, b) {
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
  }
  function sc(a, s) {
    return [a[0] * s, a[1] * s, a[2] * s]
  }
  function cross(a, b) {
    return [
      a[1] * b[2] - a[2] * b[1],
      a[2] * b[0] - a[0] * b[2],
      a[0] * b[1] - a[1] * b[0],
    ]
  }
  function norm(a) {
    var l = Math.hypot(a[0], a[1], a[2])
    return sc(a, 1 / l)
  }
  function horiz(az) {
    return [Math.cos(az), 0, Math.sin(az)]
  }
  // unit vector tilted `tilt` from vertical toward azimuth az
  function tilted(az, tilt) {
    return add(sc(horiz(az), Math.sin(tilt)), [0, Math.cos(tilt), 0])
  }
  function bez(p0, p1, p2, p3, t) {
    var u = 1 - t
    return add(
      add(sc(p0, u * u * u), sc(p1, 3 * u * u * t)),
      add(sc(p2, 3 * u * t * t), sc(p3, t * t * t)),
    )
  }
  function cone(a, b, r0, r1, capB, capA, mat) {
    g.prims.push({
      k: KIND.CONE,
      m: mat,
      a: a,
      b: b,
      r: Math.max(r0, r1),
      c: [r0, r1, capB],
      d: [capA, 0, 0],
    })
  }
  // cluster(fn): the primitives fn adds get a bounding box that rays test first
  function cluster(fn) {
    var at = g.prims.length,
      c = {
        k: KIND.CLUSTER,
        m: 0,
        a: [0, 0, 0],
        b: [0, 0, 0],
        r: 0,
        c: [0, 0, 0],
        d: [0, 0, 0],
      }
    g.prims.push(c)
    var ext = fn() || []
    var mn = [1e9, 1e9, 1e9],
      mx = [-1e9, -1e9, -1e9]
    function grow(q, r, ry) {
      for (var k = 0; k < 3; k++) {
        var e = k === 1 ? ry : r
        mn[k] = Math.min(mn[k], q[k] - e)
        mx[k] = Math.max(mx[k], q[k] + e)
      }
    }
    g.prims.slice(at + 1).forEach(function (p) {
      if (p.k === KIND.LEAF) return // the leaf's extent comes from fn
      var ry = p.k === KIND.CONE ? 0.5 : p.r + 0.5 // frusta stand upright
      grow(p.a, p.r + 0.5, ry)
      grow(p.b, p.r + 0.5, ry)
    })
    ext.forEach(function (q) {
      grow(q, 0.5, 0.5)
    })
    c.a = mn
    c.b = mx
    c.c = [g.prims.length - at - 1, 0, 0]
  }
  // one float per leaf for the leaf maps: maturity (0..1, in 64ths), freshness (0..1, in 16ths), torn
  function leafCode(mat, fresh, torn) {
    return (
      Math.round(mat * 63) +
      64 * Math.round(Math.min(1, fresh) * 15) +
      (torn ? 1024 : 0)
    )
  }
  function chain(pts, r0, r1, mat) {
    for (var i = 0; i < pts.length - 1; i++)
      g.capsule(
        pts[i],
        pts[i + 1],
        r0 + ((r1 - r0) * i) / Math.max(1, pts.length - 2),
        mat,
      )
  }

  /* ---- planter: returns the soil's height and radius ---- */
  // A round pot: wall r0 -> r1 from y0 to y1 with a flat rim, an inner wall down to the soil
  function roundPot(y0, y1, r0, r1, rim, mat) {
    var soilY = y1 - 2.6,
      inR = r1 - rim,
      inRs = inR - ((y1 - soilY) * (r1 - r0)) / (y1 - y0)
    cone([0, y0, 0], [0, y1, 0], r0, r1, inR, 0, mat)
    cone([0, soilY - 0.5, 0], [0, y1, 0], inRs - 0.05, inR, -1, -1, mat)
    cone([0, soilY - 1.5, 0], [0, soilY, 0], inRs, inRs, 0, -1, MAT.SOIL)
    return { y: soilY, r: inRs }
  }
  // low wide cylinder in the chairs' ochre (satin glaze), floating on a recessed chrome plinth
  var soil
  cluster(function () {
    cone([0, 0, 0], [0, 4.5, 0], 15.5, 15.5, -1, -1, MAT.CHROME)
    soil = roundPot(4.5, 33, 21.5, 21.5, 1.2, MAT.LEATHER)
  })

  /* ---- stems: three rooted cuttings, close together, each a short leaning stem ---- */
  var WALL = -Math.PI / 2,
    FRONT = Math.PI / 2 // azimuths of the wall behind and of the room
  function angDiff(a, b) {
    var d = (a - b) % (2 * Math.PI)
    return d > Math.PI ? d - 2 * Math.PI : d < -Math.PI ? d + 2 * Math.PI : d
  }
  // keep leaves from pointing into the wall: squeeze the back half-circle toward the sides
  function offWall(az) {
    var d = angDiff(az, WALL)
    return Math.abs(d) > 1.6
      ? az
      : WALL + (d < 0 ? -1 : 1) * (1.15 + Math.abs(d) * 0.3)
  }
  var stems = [],
    phi = rnd(0, Math.PI * 2),
    counts = [6, 5, 3]
  if (rand() < 0.5) counts = [5, 6, 3]
  cluster(function () {
    for (var s = 0; s < 3; s++) {
      phi += s === 0 ? 0 : rnd(1.7, 2.7)
      var base = add(sc(horiz(phi + rnd(-0.4, 0.4)), rnd(1.5, 4.5)), [
        0,
        soil.y - 1.5,
        0,
      ])
      var len = [rnd(10, 15), rnd(6, 10), rnd(3, 6)][s],
        lean = rnd(0.15, 0.45)
      var tip = add(base, sc(tilted(phi, lean), len))
      var sr = rnd(1.6, 2.1)
      g.capsule(base, tip, sr, MAT.STEM)
      stems.push({ base: base, tip: tip, phi: phi, sr: sr, n: counts[s] })
    }
  })

  /* ---- leaves ---- */
  var leaves = []
  var LEAF_DROOP = 0.06 // deepest strap droop below the sheet, in leaf lengths (the leaf maps' droop stays below it)
  var sagLeaf = Math.floor(rnd(0, 2)),
    turnLeaf = 3 + Math.floor(rnd(0, 4)),
    tornLeaf = 2 + Math.floor(rnd(0, 5)),
    li = 0
  stems.forEach(function (st, s) {
    var az = st.phi + rnd(-0.5, 0.5)
    for (var k = 0; k < st.n; k++, li++) {
      // r: 0 the stem's oldest leaf .. 1 its newest; the youngest stem's leaves are smaller
      var r = k / (st.n - 1),
        furled = s === 0 && k === st.n - 1
      az += k === 0 ? 0 : Math.PI + rnd(-0.7, 0.7) // alternate sides, unevenly
      var aZ = offWall(az)
      aZ += 0.2 * angDiff(FRONT, aZ) // the room side is the light side
      var sizeS = [1, 0.93, 0.8][s]
      var L = (24 + 22 * r + rnd(-3, 3)) * sizeS
      var mat = Math.min(1, Math.max(0, (L - 26) / 14 + rnd(-0.12, 0.12)))
      var sag = s === 0 && k === sagLeaf
      // petioles: older ones shorter and leaning out more, younger ones longer and steeper
      var Lp =
        (32 + 36 * r + rnd(-5, 6) + (s === 2 ? -4 : 0)) * (furled ? 0.85 : 1)
      var tb = 0.1 + 0.32 * (1 - r) + rnd(-0.06, 0.1) + (sag ? 0.25 : 0) // tilt from vertical at the base
      var tt = tb + 0.25 + 0.35 * (1 - r) + rnd(-0.1, 0.15) + (sag ? 0.4 : 0) // ... and at the top (it arches over)
      var p0 = add(
        st.base,
        sc(add(st.tip, sc(st.base, -1)), 0.35 + (0.65 * k) / st.n),
      )
      var dB = tilted(aZ + rnd(-0.25, 0.25), tb),
        dT = tilted(aZ, tt)
      var p3 = add(p0, sc(tilted(aZ, 0.5 * (tb + tt) + 0.05), Lp))
      var p1 = add(p0, sc(dB, Lp * 0.4)),
        p2 = add(p3, sc(dT, -Lp * 0.35))
      var pts = []
      for (var j = 0; j <= 6; j++) pts.push(bez(p0, p1, p2, p3, j / 6))
      var pr = 0.32 + 0.011 * L
      cluster(function () {
        g.capsule(pts[0], add(pts[0], sc(dB, 7)), pr * 1.8, MAT.STEM) // sheathing base
        chain(pts, pr * 1.15, pr * 0.85, MAT.PETIOLE)
        var knee = pts[6]
        if (furled) {
          // a new leaf still rolled into a spike, carried up on the petiole's line
          var f1 = add(knee, sc(tilted(aZ, tt - 0.1), L * 0.16)),
            f2 = add(f1, sc(tilted(aZ + 0.3, tt + 0.15), L * 0.18))
          cone(knee, f1, pr * 1.3, pr * 1.9, -1, -1, MAT.FURL)
          cone(f1, f2, pr * 1.9, 0.12, -1, -1, MAT.FURL)
          return []
        }
        // the blade turns out and down at the geniculum, older leaves hanging lower, and turns
        // its face toward the room, where the light comes from
        var yaw =
          aZ +
          0.3 * angDiff(FRONT, aZ) +
          rnd(-0.35, 0.35) +
          (li === turnLeaf ? 0.9 * (rand() < 0.5 ? -1 : 1) : 0)
        var pitch =
          -(0.25 + 0.3 * (1 - r) + rnd(-0.12, 0.15)) - (sag ? 0.45 : 0)
        var T = norm(
          add(sc(horiz(yaw), Math.cos(pitch)), [0, Math.sin(pitch), 0]),
        )
        var S = norm(cross(norm([0, 1, 0.5]), T)),
          N = cross(T, S)
        var roll = rnd(-0.3, 0.3) + (li === turnLeaf ? 0.45 : 0)
        var S2 = add(sc(S, Math.cos(roll)), sc(N, Math.sin(roll)))
        var bend = -rnd(0.14, 0.3) - 0.08 * mat - (sag ? 0.1 : 0),
          cup = -rnd(0.12, 0.28) - 0.08 * mat,
          twist = rnd(-0.18, 0.18)
        var N2 = cross(T, S2),
          ext = []
        for (var X = -0.3; X <= 1.001; X += 0.1)
          for (var Y = -0.65; Y <= 0.651; Y += 0.13) {
            var z = bend * X * X + twist * X * Y + cup * Y * Y
            ;[0, -LEAF_DROOP].forEach(function (dz) {
              ext.push(
                add(knee, sc(add(add(sc(T, X), sc(S2, Y)), sc(N2, z + dz)), L)),
              )
            })
          }
        g.prims.push({
          k: KIND.LEAF,
          m: MAT.LEAF,
          a: knee,
          b: add(knee, sc(T, L)),
          r: L * 1.25,
          c: sc(S2, 1 + leaves.length),
          d: [bend, cup, twist],
        })
        // freshness: the newest leaves are a lighter, yellower green; old ones dark
        leaves.push(
          leafCode(
            mat,
            Math.max(0, r * 1.2 - 0.4 + rnd(-0.1, 0.1)) * sizeS,
            li === tornLeaf,
          ),
        )
        return ext
      })
    }
  })

  /* ---- an aerial root from the oldest stem, arching down into the soil ---- */
  cluster(function () {
    var st = stems[0],
      d = horiz(offWall(st.phi + 0.6))
    var r0 = add(st.tip, sc(d, st.sr * 0.8))
    chain(
      [
        r0,
        add(r0, add(sc(d, 3), [0, -0.5, 0])),
        add(r0, add(sc(d, 5.5), [0, -3.5, 0])),
        add(r0, add(sc(d, 6.5), [0, -9, 0])),
      ],
      0.45,
      0.4,
      MAT.AERIAL_ROOT,
    )
  })
  g.leafMats = leaves // for the leaf maps, one per KIND.LEAF in order
  return g
}
