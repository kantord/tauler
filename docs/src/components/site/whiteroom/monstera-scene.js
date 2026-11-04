/* Procedural Monstera deliciosa in a terracotta pot, for the White Room path tracer.
 *
 * addMonstera(group, { seed, KIND, MAT }) appends analytic primitives to a scene Group
 * (centimetres, y up, the pot standing on the floor at the group origin). Same seed, same plant.
 *
 * New primitive kinds (the room shader's MONSTERA additions):
 *   KIND.LEAF  a = blade base (petiole junction), b = a + midribDir * length,
 *              c = lateralDir * (1 + leaf index), d = [bend, cup, twist], r = bounding pad.
 *              The blade is the height field z = L (bend X^2 + twist X Y + cup Y^2) over the
 *              plane (midribDir, lateralDir), less each strap's droop; outline, pinnate splits,
 *              fenestrations, droop and veins come from the leaf's map, baked on the GPU by
 *              the leaf-map shader from g.leafMats (one maturity per leaf, in order).
 *   KIND.CLUSTER  a box (a = min, b = max) around the next c.x primitives: a ray that misses
 *              it skips them (one cluster per leaf with its petiole, one for the pot, ...).
 *   KIND.CONE  open frustum a -> b with radii c.x -> c.y; optional annular caps:
 *              c.z = inner radius of the cap at b, d.x = inner radius of the cap at a (-1 = none).
 *              r = max radius (bounding pad).
 * Materials: MAT.LEAF, PETIOLE, STEM, AERIAL_ROOT, POT, SOIL.
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
  function chain(pts, r0, r1, mat) {
    for (var i = 0; i < pts.length - 1; i++)
      g.capsule(
        pts[i],
        pts[i + 1],
        r0 + ((r1 - r0) * i) / Math.max(1, pts.length - 2),
        mat,
      )
  }

  /* ---- pot: tapered terracotta "standard" pot with a rolled collar, on a saucer ---- */
  var H = 31,
    rTop = 17.5,
    rBot = 13.2,
    collarH = 5.5,
    lip = 1.1,
    wall = 1.2,
    sau = 1.0
  var bodyTopR = rBot + (rTop - lip - rBot) * 1.0 // body radius under the collar
  var soilY = H - 3.4,
    inR = rTop - lip - wall + 0.15
  var inRsoil = inR - ((H - soilY) * (rTop - rBot)) / H
  cluster(function () {
    cone([0, 0, 0], [0, 3.2, 0], 15.2, 18.6, 17.4, 0, MAT.POT) // saucer wall + rim
    cone([0, 0.2, 0], [0, sau, 0], 17.45, 17.45, 0, -1, MAT.POT) // saucer floor
    cone([0, sau, 0], [0, H - collarH, 0], rBot, bodyTopR, -1, 0, MAT.POT)
    cone(
      [0, H - collarH, 0],
      [0, H, 0],
      rTop,
      rTop + 0.15,
      rTop - lip - wall + 0.15,
      bodyTopR - 0.05,
      MAT.POT,
    )
    cone([0, soilY - 0.5, 0], [0, H, 0], inRsoil, inR, -1, -1, MAT.POT) // inner wall
    cone(
      [0, soilY - 1.5, 0],
      [0, soilY, 0],
      inRsoil + 0.05,
      inRsoil + 0.05,
      0,
      -1,
      MAT.SOIL,
    )
  })

  /* ---- stems: a nursery pot holds a few rooted cuttings; each a short thick stem ---- */
  var nodes = [],
    nStems = 3,
    az0 = rnd(0, Math.PI * 2)
  cluster(function () {
    for (var s = 0; s < nStems; s++) {
      var saz = az0 + (s * Math.PI * 2) / nStems + rnd(-0.4, 0.4),
        srad = rnd(2.5, 6.5)
      var base = [Math.cos(saz) * srad, soilY - 1, Math.sin(saz) * srad]
      var lean = rnd(0.35, 0.8),
        len = rnd(9, 16),
        lean2 = lean + rnd(-0.3, 0.2)
      var dir = add(sc(horiz(saz + rnd(-0.5, 0.5)), Math.sin(lean)), [
        0,
        Math.cos(lean),
        0,
      ])
      var mid = add(base, sc(dir, len * 0.55))
      var dir2 = add(sc(horiz(saz + rnd(-0.6, 0.6)), Math.sin(lean2)), [
        0,
        Math.cos(lean2),
        0,
      ])
      var tip = add(mid, sc(dir2, len * 0.45))
      var sr = rnd(2.0, 2.6)
      g.capsule(base, mid, sr, MAT.STEM)
      g.capsule(mid, tip, sr * 0.9, MAT.STEM)
      var nN = 4
      for (var k = 0; k < nN; k++) {
        var t = (k + 1) / nN
        var p =
          t < 0.55
            ? add(base, sc(add(mid, sc(base, -1)), t / 0.55))
            : add(mid, sc(add(tip, sc(mid, -1)), (t - 0.55) / 0.45))
        nodes.push({ p: p, stemAz: saz, stem: s, k: k, sr: sr })
      }
    }
  })

  /* ---- leaves: distichous along each stem, sizes from juvenile to mature ---- */
  var golden = 2.39996,
    leaves = []
  var LEAF_DROOP = 0.06 // deepest strap droop below the sheet, in leaf lengths (the leaf maps' droop stays below it)
  nodes.forEach(function (nd, i) {
    // the stem's two ranks of leaves alternate sides; the whole plant fans out in all directions
    var az = az0 + i * golden + rnd(-0.35, 0.35) // spread around the pot (golden angle)
    var rank = nd.k / 3 // older leaves sit lower and are smaller
    var L =
      nd.stem === 0 && nd.k === 3
        ? rnd(28, 34)
        : rnd(38, 47) + rank * rnd(12, 24) + (nd.stem === 1 ? 4 : 0)
    var mat = Math.min(1, Math.max(0, (L - 30) / 26 + rnd(-0.1, 0.1)))
    var out = horiz(az),
      up = [0, 1, 0]
    var Lp = L * rnd(0.9, 1.25) + 10
    var el0 = rnd(1.25, 1.5),
      elc = rnd(0.95, 1.25) - rank * 0.3 // petiole leaves the stem steeply, arches out
    var p0 = nd.p
    var p3 = add(
      p0,
      add(sc(out, Math.cos(elc) * Lp), [0, Math.sin(elc) * Lp, 0]),
    )
    var p1 = add(
      p0,
      add(sc(out, Math.cos(el0) * Lp * 0.5), [0, Math.sin(el0) * Lp * 0.5, 0]),
    )
    var p2 = add(
      p3,
      add(sc(out, -Math.cos(elc) * Lp * 0.3), [0, -Lp * 0.06, 0]),
    )
    var pts = []
    for (var j = 0; j <= 5; j++) pts.push(bez(p0, p1, p2, p3, j / 5))
    cluster(function () {
      var pr = 0.5 + 0.009 * L
      g.capsule(pts[0], pts[1], pr * 1.5, MAT.PETIOLE) // sheathing base
      chain(pts.slice(1), pr * 1.1, pr * 0.95, MAT.PETIOLE)
      var knee = pts[5]
      // blade: from the geniculum outward, near level at the base and arching down to the tip, its
      // halves only slightly folded down; big mature blades hang a little lower than young ones
      var yaw = az + rnd(-0.3, 0.3)
      var pitch = -rnd(0.05, 0.35) - mat * 0.15
      var T = norm(
        add(sc(horiz(yaw), Math.cos(pitch)), [0, Math.sin(pitch), 0]),
      )
      var S = norm(cross(up, T)),
        N = cross(T, S)
      var roll = rnd(-0.4, 0.4)
      var S2 = add(sc(S, Math.cos(roll)), sc(N, Math.sin(roll)))
      var bend = -rnd(0.16, 0.32),
        cup = -rnd(0.06, 0.2) - 0.06 * mat,
        twist = rnd(-0.15, 0.15)
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
      var pad = L * 1.25
      g.prims.push({
        k: KIND.LEAF,
        m: MAT.LEAF,
        a: knee,
        b: add(knee, sc(T, L)),
        r: pad,
        c: sc(S2, 1 + leaves.length),
        d: [bend, cup, twist],
      })
      leaves.push(mat)
      return ext
    })
  })

  /* ---- aerial roots: one dives into the soil, one hangs over the rim to the floor ---- */
  cluster(function () {
    var nd = nodes[1],
      o1 = horiz(o.rootAz === undefined ? 1.75 : o.rootAz) // default: front-left as seen from the room camera
    var r0 = add(nd.p, sc(o1, nd.sr))
    var rim = add(sc(o1, rTop + 0.8), [0, H + 1.2, 0])
    var pts = [
      r0,
      add(r0, add(sc(o1, 5), [0, 2.5, 0])),
      add(sc(o1, rTop - 3), [0, H + 2.4, 0]),
      rim,
      add(sc(o1, rTop + 1.6), [0, H - 6, 0]),
      add(sc(o1, rTop + 0.6), [0, H - 17, 0]),
      add(sc(o1, rBot + 2.4), [0, 9, 0]),
      add(sc(o1, rBot + 4.5), [0, 3.6, 0]),
    ]
    chain(pts, 0.55, 0.45, MAT.AERIAL_ROOT)
    var nd2 = nodes[nodes.length - 2],
      o2 = horiz(nd2.stemAz + 0.8)
    var q0 = add(nd2.p, sc(o2, nd2.sr))
    chain(
      [
        q0,
        add(q0, add(sc(o2, 4), [0, -1, 0])),
        add(q0, add(sc(o2, 7.5), [0, -4.5, 0])),
        add(q0, add(sc(o2, 9), [0, -8, 0])),
      ],
      0.5,
      0.45,
      MAT.AERIAL_ROOT,
    )
  })
  g.leafMats = leaves // for the leaf maps, one per KIND.LEAF in order
  return g
}
