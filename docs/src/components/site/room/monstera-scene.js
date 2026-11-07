/* Procedural Monstera deliciosa in a mid-century planter, and the uplighter's cable, for the
 * home background's path tracer.
 *
 * addMonstera(group, { seed, KIND, MAT }) appends analytic primitives to a scene Group
 * (centimetres, y up, the planter standing on the floor at the group origin, the back wall
 * toward -z). Same seed, same plant. The planter is a low cylinder glazed ochre, the chairs'
 * colour (MAT.POT, a glaze in the room shader's materials) on a recessed chrome plinth (its CHROME).
 *
 * The habit follows photographs of potted plants of about 1.2 m: three rooted
 * cuttings, each a thick stem that leans out of the soil and curves up toward the light, ringed
 * where its leaves came from, with a few brown aerial roots. The leaves alternate sides along each
 * stem, unevenly; each petiole leaves its node in a sheath and rises in a loose, uneven S, arching
 * over toward its top, where a paler knee (the geniculum) turns the blade out and down. Leaves get
 * bigger and more split with each new one, so the oldest are small, whole and low and the newest
 * large, split and high; the newest of all is still a rolled spike. Nothing is regular: every
 * length, angle, thickness and bend is seeded noise, the petioles cross, the blades clump and
 * leave gaps, some droop more, some are turned on their stalks and show their undersides, and one
 * is torn and two are ageing (a yellowed margin, a dry tip). Leaves keep off the wall behind.
 *
 * New primitive kinds (the room shader's MONSTERA additions):
 *   KIND.LEAF  a = blade base (petiole junction), b = a + midribDir * length,
 *              c = lateralDir * (1 + leaf index), d = [bend, cup, twist], r = bounding pad.
 *              The blade is the height field z = L (bend X^2 + twist X Y + cup Y^2) over the
 *              plane (midribDir, lateralDir), less the leaf map's height (strap droop, the fold
 *              along the midrib, a rippled margin, the veins' relief); outline, pinnate splits,
 *              fenestrations, veins and blemishes come from the leaf's map, baked on the GPU by
 *              the leaf-map shader from g.leafMats (one code per leaf, in order: see leafCode).
 *   KIND.CLUSTER  a box (a = min, b = max) around the next c.x primitives: a ray that misses
 *              it skips them. Clusters nest: one per leaf, inside it one per few segments of
 *              its petiole and one for the blade.
 *   KIND.CONE  open frustum a -> b with radii c.x -> c.y; optional annular caps:
 *              c.z = inner radius of the cap at b, d.x = inner radius of the cap at a (-1 = none).
 *              r = max radius (bounding pad).
 * Materials: MAT.LEAF, PETIOLE, STEM, AERIAL_ROOT, SOIL, FURL (pale new tissue: the rolled
 * leaf and each petiole's knee), POT.
 */

// cluster(g, KIND, fn): the primitives fn adds get a bounding box that rays test first; fn may
// return extra points to include (a leaf's extent)
function cluster(g, KIND, fn) {
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

// a smooth tube along the points: capsules in clusters of `per`, radius r(i / n) along it
function tube(g, KIND, pts, r, mat, per) {
  var n = pts.length - 1
  for (var i0 = 0; i0 < n; i0 += per)
    cluster(g, KIND, function () {
      for (var i = i0; i < Math.min(n, i0 + per); i++)
        g.capsule(pts[i], pts[i + 1], r((i + 0.5) / n), mat)
    })
}

/* The uplighter's cable, from its base to a wall socket behind the planter: a centripetal
 * Catmull-Rom spline through `pts`, cut into capsules short enough where it bends that no
 * corner shows (each segment stays within 0.03 cm of the curve), long where it runs straight. */
export function addCable(g, o) {
  var P = o.pts
  function lerp(a, b, t) {
    return [
      a[0] + (b[0] - a[0]) * t,
      a[1] + (b[1] - a[1]) * t,
      a[2] + (b[2] - a[2]) * t,
    ]
  }
  function dist(a, b) {
    return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2])
  }
  var dense = []
  for (var i = 0; i < P.length - 1; i++) {
    var p0 = P[Math.max(0, i - 1)],
      p1 = P[i],
      p2 = P[i + 1],
      p3 = P[Math.min(P.length - 1, i + 2)]
    var t1 = Math.sqrt(dist(p0, p1)) || 1e-3,
      t2 = t1 + Math.sqrt(dist(p1, p2)),
      t3 = t2 + (Math.sqrt(dist(p2, p3)) || 1e-3)
    for (var j = 0; j < 40; j++) {
      // Barry-Goldman pyramid
      var t = t1 + ((t2 - t1) * j) / 40
      var a1 = lerp(p0, p1, t / t1),
        a2 = lerp(p1, p2, (t - t1) / (t2 - t1)),
        a3 = lerp(p2, p3, (t - t2) / (t3 - t2))
      var b1 = lerp(a1, a2, t / t2),
        b2 = lerp(a2, a3, (t - t1) / (t3 - t1))
      dense.push(lerp(b1, b2, (t - t1) / (t2 - t1)))
    }
  }
  dense.push(P[P.length - 1])
  // keep the fewest points whose chords stay within 0.03 cm of the curve
  var pts = [dense[0]],
    s = 0
  for (var e = 2; e < dense.length; e++) {
    var a = dense[s],
      b = dense[e],
      ab = dist(a, b),
      ok = true
    for (var k = s + 1; k < e && ok; k++) {
      var q = dense[k],
        t0 =
          ((q[0] - a[0]) * (b[0] - a[0]) +
            (q[1] - a[1]) * (b[1] - a[1]) +
            (q[2] - a[2]) * (b[2] - a[2])) /
          (ab * ab)
      ok = dist(q, lerp(a, b, t0)) < 0.03
    }
    if (!ok) {
      pts.push(dense[e - 1])
      s = e - 1
    }
  }
  pts.push(dense[dense.length - 1])
  tube(
    g,
    o.KIND,
    pts,
    function () {
      return o.r
    },
    o.mat,
    8,
  )
}

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
  function curve(p0, p1, p2, p3, n) {
    var pts = []
    for (var j = 0; j <= n; j++) pts.push(bez(p0, p1, p2, p3, j / n))
    return pts
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
  function clu(fn) {
    cluster(g, KIND, fn)
  }
  // one float per leaf for the leaf maps: maturity (0..1, in 64ths), freshness (0..1, in 16ths),
  // torn, age damage (0..3: none, a yellowing margin, and a dry tip)
  function leafCode(mat, fresh, torn, age) {
    return (
      Math.round(mat * 63) +
      64 * Math.round(Math.min(1, fresh) * 15) +
      (torn ? 1024 : 0) +
      2048 * age
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
  // low wide cylinder in an ochre glaze, floating on a recessed chrome plinth
  var soil
  clu(function () {
    cone([0, 0, 0], [0, 4.5, 0], 15.5, 15.5, -1, -1, MAT.CHROME)
    soil = roundPot(4.5, 33, 21.5, 21.5, 1.2, MAT.POT)
  })

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

  /* ---- stems: three rooted cuttings, each leaning out of the soil and curving up, with nodes ---- */
  var stems = [],
    phi = rnd(0, Math.PI * 2),
    counts = rand() < 0.5 ? [5, 5, 3] : [5, 4, 4]
  for (var s = 0; s < 3; s++) {
    phi += s === 0 ? 0 : rnd(1.6, 2.6)
    var p = add(sc(horiz(phi + rnd(-0.5, 0.5)), rnd(2, 6.5)), [
      0,
      soil.y - 2.5,
      0,
    ])
    var lean = [rnd(0.25, 0.45), rnd(0.5, 0.75), rnd(0.8, 1.05)][s],
      az = phi,
      len = [rnd(16, 22), rnd(11, 16), rnd(6, 10)][s],
      sr = rnd(1.7, 2.3),
      nodes = [p],
      dirs = []
    for (var j = 0; j < 5; j++) {
      // it curves up toward the light, wavering
      var d = tilted(az, lean)
      dirs.push(d)
      p = add(p, sc(d, (len / 5) * rnd(0.8, 1.2)))
      nodes.push(p)
      lean = Math.max(0.05, lean - rnd(0.04, 0.16))
      az += rnd(-0.25, 0.25)
    }
    stems.push({ nodes: nodes, dirs: dirs, phi: phi, sr: sr, n: counts[s] })
  }
  clu(function () {
    stems.forEach(function (st) {
      for (var j = 0; j < 5; j++) {
        var r = st.sr * (1.04 - 0.08 * j) * rnd(0.94, 1.06)
        g.capsule(st.nodes[j], st.nodes[j + 1], r, MAT.STEM)
        // the node's ring: a leaf scar, slightly swollen
        if (j > 0)
          g.capsule(
            st.nodes[j],
            add(st.nodes[j], sc(st.dirs[j], 0.7)),
            r * 1.1,
            MAT.STEM,
          )
      }
    })
  })

  /* ---- aerial roots: brown cords out of the nodes, one down into the soil, one arching over to
     the rim, one a young stub ---- */
  clu(function () {
    var rootPlan = [
      [0, 2, 9, -12, 0.42],
      [1, 3, 16, -16, 0.36],
      [2, 2, 3.5, 0, 0.3],
    ]
    rootPlan.forEach(function (rp) {
      var st = stems[rp[0]],
        n0 = st.nodes[rp[1]],
        dd = horiz(offWall(st.phi + rnd(-1.2, 1.2))),
        out = rp[2],
        a = add(n0, sc(dd, st.sr * 0.8))
      var end = add(add(a, sc(dd, out)), [0, rp[3] === 0 ? -2.5 : 0, 0])
      if (rp[3] !== 0) end[1] = soil.y - 0.5 // into the soil
      var mid1 = add(a, add(sc(dd, out * 0.45), [0, 1.5, 0])),
        mid2 = add(end, add(sc(dd, -out * 0.1), [0, rp[3] === 0 ? 1 : 6, 0]))
      var pts = curve(a, mid1, mid2, end, rp[3] === 0 ? 4 : 10)
      // a wavering cord, not a clean arc
      for (var k = 1; k < pts.length - 1; k++)
        pts[k] = add(pts[k], [rnd(-0.6, 0.6), rnd(-0.4, 0.4), rnd(-0.6, 0.6)])
      tube(
        g,
        KIND,
        pts,
        function (t) {
          return rp[4] * (1 - 0.35 * t)
        },
        MAT.AERIAL_ROOT,
        5,
      )
    })
  })

  /* ---- leaves ---- */
  var leaves = []
  var LEAF_SINK = 0.1 // deepest the leaf map's height goes below the sheet, in leaf lengths
  var sagLeaf = Math.floor(rnd(0, 2)),
    turnLeaves = [3 + Math.floor(rnd(0, 3)), 8 + Math.floor(rnd(0, 3))],
    tornLeaf = 4 + Math.floor(rnd(0, 4)),
    li = 0
  stems.forEach(function (st, s) {
    var az = st.phi + rnd(-0.6, 0.6)
    for (var k = 0; k < st.n; k++, li++) {
      // r: 0 the stem's oldest leaf .. 1 its newest; the youngest stem's leaves are smaller
      var r = k / (st.n - 1),
        furled = s === 0 && k === st.n - 1
      az += k === 0 ? 0 : Math.PI + rnd(-0.9, 0.9) // alternate sides, unevenly
      var aZ = offWall(az)
      aZ += 0.2 * angDiff(FRONT, aZ) // the room side is the light side
      var sizeS = [1, 0.9, 0.78][s]
      var L = (17 + 36 * r + rnd(-3, 3)) * sizeS
      var mat = Math.min(1, Math.max(0, (L - 22) / 17 + rnd(-0.12, 0.12)))
      var sag = s === 0 && k === sagLeaf
      var turned = turnLeaves.indexOf(li) >= 0
      // the node it grows from, older leaves lower on the stem
      var ni = Math.min(
        5,
        1 + Math.round(((k + 1) * 4) / st.n + rnd(-0.4, 0.4)),
      )
      var node = st.nodes[ni]
      var p0 = add(node, sc(horiz(aZ), st.sr * 0.7))
      // petiole: older ones shorter and leaning out more, younger ones longer and steeper;
      // it rises in an uneven S and arches over toward its top
      var Lp =
        (30 + 42 * r + rnd(-8, 8) + (s === 2 ? -5 : 0)) * (furled ? 0.8 : 1)
      var tb = 0.1 + 0.35 * (1 - r) + rnd(-0.08, 0.14) + (sag ? 0.25 : 0),
        tt = tb + 0.35 + 0.55 * (1 - r) + rnd(-0.15, 0.25) + (sag ? 0.5 : 0)
      // the blade turns out and down at the knee, older leaves hanging lower, its face
      // turned partly to the room
      var yaw =
        aZ +
        0.25 * angDiff(FRONT, aZ) +
        rnd(-0.45, 0.45) +
        (turned ? 0.8 * (rand() < 0.5 ? -1 : 1) : 0)
      var pitch = -(0.38 + 0.5 * (1 - r) + rnd(-0.15, 0.2)) - (sag ? 0.5 : 0)
      var T = norm(
        add(sc(horiz(yaw), Math.cos(pitch)), [0, Math.sin(pitch), 0]),
      )
      var S = norm(cross([0, 1, 0], T)),
        N = cross(T, S)
      var roll = rnd(-0.35, 0.35) + (turned ? 0.6 * (rand() < 0.5 ? -1 : 1) : 0)
      var S2 = add(sc(S, Math.cos(roll)), sc(N, Math.sin(roll)))
      var bend = -(0.12 + 0.22 * mat + rnd(0, 0.14)) - (sag ? 0.12 : 0),
        cup = -rnd(0.06, 0.22),
        twist = rnd(-0.22, 0.22)
      var N2 = cross(T, S2)
      // the knee: the petiole's last stretch turns to meet the blade from below and behind, in
      // the plane of its midrib, so it never pierces the blade
      var ka = rnd(0.6, 1.15),
        dk = add(sc(T, Math.cos(ka)), sc(N2, Math.sin(ka)))
      var sway = rnd(0.2, 0.45) * (rand() < 0.5 ? -1 : 1),
        wav = rnd(0.6, 1.3),
        ph = rnd(0, Math.PI * 2),
        bow = rnd(1.1, 2.2),
        M = 14,
        pts = [p0],
        q = p0,
        dir
      for (var j = 0; j < M; j++) {
        var t = (j + 0.5) / M
        var tilt =
          tb + (tt - tb) * Math.pow(t, bow) + 0.04 * Math.sin(t * 7 + ph)
        dir = tilted(
          aZ +
            sway * Math.sin(Math.PI * 2 * wav * t + ph) -
            sway * Math.sin(ph),
          tilt,
        )
        var kw = Math.max(0, (j - (M - 4)) / 3)
        if (kw > 0) dir = norm(add(sc(dir, 1 - kw), sc(dk, kw)))
        q = add(q, sc(dir, (Lp / M) * rnd(0.9, 1.1)))
        pts.push(q)
      }
      var pr = (0.3 + 0.009 * L) * rnd(0.85, 1.15)
      var knee = pts[M]
      clu(function () {
        // the sheath at its base, wrapped round the stem
        g.capsule(pts[0], pts[2], pr * 1.75, MAT.STEM)
        g.capsule(pts[2], pts[3], pr * 1.35, MAT.PETIOLE)
        tube(
          g,
          KIND,
          pts.slice(2, M),
          function (u) {
            return pr * (1.12 - 0.27 * u)
          },
          MAT.PETIOLE,
          4,
        )
        // the knee: a paler, thicker joint
        g.capsule(pts[M - 2], knee, pr * 1.1, MAT.FURL)
        if (furled) {
          // a new leaf still rolled into a slim spike, carried up beyond the knee and curving
          var fa = tilted(aZ + rnd(-0.3, 0.3), tt - 0.25),
            fb = tilted(aZ + rnd(-0.4, 0.4), tt + 0.1),
            fp = curve(
              knee,
              add(knee, sc(fa, L * 0.12)),
              add(knee, sc(fb, L * 0.24)),
              add(knee, sc(fb, L * 0.36)),
              6,
            )
          tube(
            g,
            KIND,
            fp,
            function (u) {
              return pr * (1.15 - 0.95 * u * u)
            },
            MAT.FURL,
            6,
          )
          return []
        }
        var ext = [],
          base = add(knee, sc(N2, pr)) // the blade sits on its knee
        clu(function () {
          for (var X = -0.3; X <= 1.051; X += 0.135)
            for (var Y = -0.65; Y <= 0.651; Y += 0.13) {
              var z = bend * X * X + twist * X * Y + cup * Y * Y
              ext.push(
                add(base, sc(add(add(sc(T, X), sc(S2, Y)), sc(N2, z)), L)),
              )
              ext.push(
                add(
                  base,
                  sc(add(add(sc(T, X), sc(S2, Y)), sc(N2, z - LEAF_SINK)), L),
                ),
              )
            }
          g.prims.push({
            k: KIND.LEAF,
            m: MAT.LEAF,
            a: base,
            b: add(base, sc(T, L)),
            r: L * 1.25,
            c: sc(S2, 1 + leaves.length),
            d: [bend, cup, twist],
          })
          // freshness: the newest leaves are a lighter, yellower green; old ones dark. The two
          // oldest leaves of the first stem are ageing.
          leaves.push(
            leafCode(
              mat,
              Math.max(0, r * 1.1 - 0.45 + rnd(-0.1, 0.1)) * sizeS,
              li === tornLeaf,
              s === 0 && k < 2 ? 2 - k : s === 1 && k === 0 ? 1 : 0,
            ),
          )
          return ext
        })
        return []
      })
    }
  })
  g.leafMats = leaves // for the leaf maps, one per KIND.LEAF in order
  return g
}
