// The scene of the home page's room (room.ts), inspired by a photo by Urban Vintage on
// Unsplash. It began as the scene script of a standalone page, hence the IIFE that sets
// `window.ROOM_SCENE` on a local `window`.
import { addCable, addMonstera, cluster } from './monstera-scene.js'

export function buildRoomScene() {
  const window = {}

  /* Scene description for the room, in centimetres.
   Builds a flat list of analytic primitives grouped into transformed "groups"
   (each with a local bounding box for culling). Uploaded to the GPU as a float texture. */
  ;(function () {
    var KIND = {
      BOX: 0,
      CAPSULE: 1,
      CYL: 2,
      QUAD: 3,
      SPHERE: 4,
      LEAF: 5,
      CONE: 6,
      CLUSTER: 7,
    }
    var MAT = {
      WALL: 0,
      FLOOR: 1,
      CHROME: 2,
      TEAK: 3,
      TEAK_DARK: 4,
      GAP: 5,
      LEATHER: 6,
      STONE: 7,
      LAMPHEAD: 8,
      MARBLE: 9,
      GLASS_DARK: 10,
      RUG: 11,
      FRAME: 12,
      ART: 13,
      WINDOW: 14,
      UPLIGHT: 15,
      BULB: 16,
      BOOK_CREAM: 17,
      BOOK_RED: 18,
      CERAMIC: 19,
      FRINGE: 20,
      GROUT: 21,
      LEAF: 22,
      PETIOLE: 23,
      STEM: 24,
      AERIAL_ROOT: 25,
      POT: 26,
      SOIL: 27,
      FURL: 28,
    }

    class Group {
      constructor(name) {
        this.name = name
        this.prims = []
        this.pos = [0, 0, 0]
        this.rotY = 0
      }
    }
    Group.prototype.box = function (min, max, mat) {
      this.prims.push({ k: KIND.BOX, m: mat, a: min, b: max, r: 0 })
      return this
    }
    // centre + size helper
    Group.prototype.cbox = function (cx, cy, cz, sx, sy, sz, mat) {
      return this.box(
        [cx - sx / 2, cy - sy / 2, cz - sz / 2],
        [cx + sx / 2, cy + sy / 2, cz + sz / 2],
        mat,
      )
    }
    Group.prototype.capsule = function (a, b, r, mat) {
      this.prims.push({ k: KIND.CAPSULE, m: mat, a: a, b: b, r: r })
      return this
    }
    Group.prototype.cyl = function (a, b, r, mat) {
      this.prims.push({ k: KIND.CYL, m: mat, a: a, b: b, r: r })
      return this
    }
    Group.prototype.quad = function (a, b, c, d, mat) {
      this.prims.push({ k: KIND.QUAD, m: mat, a: a, b: b, c: c, d: d, r: 0 })
      return this
    }
    Group.prototype.sphere = function (c, r, mat) {
      this.prims.push({ k: KIND.SPHERE, m: mat, a: c, b: c, r: r })
      return this
    }

    // polyline with rounded corners -> list of points; consecutive points become capsules (bent steel tube)
    function rounded(pts, rad, closed, steps) {
      var out = [],
        n = pts.length
      function sub(a, b) {
        return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
      }
      function len(v) {
        return Math.hypot(v[0], v[1], v[2])
      }
      function addS(a, v, s) {
        return [a[0] + v[0] * s, a[1] + v[1] * s, a[2] + v[2] * s]
      }
      var idx = []
      for (var i = 0; i < n; i++) idx.push(i)
      if (!closed) out.push(pts[0])
      var start = closed ? 0 : 1,
        end = closed ? n : n - 1
      for (var k = start; k < end; k++) {
        var p = pts[k],
          a = pts[(k - 1 + n) % n],
          b = pts[(k + 1) % n]
        var din = sub(p, a),
          dout = sub(b, p),
          li = len(din),
          lo = len(dout)
        var r = Math.min(rad, li * 0.49, lo * 0.49)
        var s = addS(p, din, -r / li),
          e = addS(p, dout, r / lo)
        var STEPS = steps || 5
        for (var j = 0; j <= STEPS; j++) {
          var t = j / STEPS,
            u = 1 - t
          out.push([
            u * u * s[0] + 2 * u * t * p[0] + t * t * e[0],
            u * u * s[1] + 2 * u * t * p[1] + t * t * e[1],
            u * u * s[2] + 2 * u * t * p[2] + t * t * e[2],
          ])
        }
      }
      if (closed) out.push(out[0])
      else out.push(pts[n - 1])
      return out
    }
    Group.prototype.tube = function (pts, r, bend, mat, closed, steps) {
      // a tube of more than three capsules gets its own bounding box
      var p = rounded(pts, bend, closed, steps),
        self = this
      function add() {
        for (var i = 0; i < p.length - 1; i++)
          self.capsule(p[i], p[i + 1], r, mat)
      }
      if (p.length > 4) cluster(this, KIND, add)
      else add()
      return this
    }

    var groups = []
    function G(name) {
      var g = new Group(name)
      groups.push(g)
      return g
    }

    /* ---------------- room ---------------- */
    // The site renders the room into boxes from 0.9:1 to 12:1 and wider (room.ts keeps
    // the vertical field of view and widens the horizontal one). The back wall, window band
    // and floor run on to LEFT_X (enough for about 20:1), so a wide frame never runs out of
    // room on the left; the walls rise to CEIL, more than the tallest frame (capped at 37
    // degrees half-angle) reaches. No ceiling or left wall: the room is lit mostly by the
    // open sky they would shut out. The window's light source spans the whole band.
    var WALL_X = 176,
      WIN_Y0 = 313,
      WIN_Y1 = 405,
      CEIL = 620,
      LEFT_X = -5000
    var room = G('room')
    room.box([LEFT_X, -20, -60], [WALL_X + 30, 0, 1500], MAT.FLOOR) // floor slab
    room.box([LEFT_X, 0, -40], [WALL_X, WIN_Y0, 0], MAT.WALL) // back wall, lower
    room.box([LEFT_X, WIN_Y1, -40], [WALL_X, CEIL, 0], MAT.WALL) // above window
    room.box([WALL_X, 0, -40], [WALL_X + 30, CEIL, 1500], MAT.WALL) // right wall
    room.box([WALL_X - 14, WIN_Y0, -40], [WALL_X, WIN_Y1, 0], MAT.WALL) // return at window end
    room.box([LEFT_X, WIN_Y0, -14], [WALL_X - 14, WIN_Y1, -12], MAT.GROUT) // sill & frame backing (behind glow)
    room.quad(
      [LEFT_X, WIN_Y0, -11],
      [WALL_X - 14, WIN_Y0, -11],
      [WALL_X - 14, WIN_Y1, -11],
      [LEFT_X, WIN_Y1, -11],
      MAT.WINDOW,
    )
    // window reveal (top and bottom faces of the opening)
    room.box([LEFT_X, WIN_Y0 - 1, -40], [WALL_X - 14, WIN_Y0, 0], MAT.WALL)
    // cove: a concave fillet of radius COVE_R (3 cm) where the walls meet the floor and each other,
    // in the walls' paint; it meets both planes tangentially, so no edge forms (kind COVE, the
    // inside of a cylinder between its axis and the corner, in the room shader). Without it the
    // walls meet the glossy floor in a razor-straight line; with it the junction reads as a soft
    // shading change a few pixels wide.
    KIND.COVE = 8
    var COVE_R = 3
    function cove(a, b, e1, e2) {
      room.prims.push({
        k: KIND.COVE,
        m: MAT.WALL,
        a: a,
        b: b,
        c: e1,
        d: e2,
        r: COVE_R,
      })
    }
    cove(
      [LEFT_X, COVE_R, COVE_R],
      [WALL_X, COVE_R, COVE_R],
      [0, -1, 0],
      [0, 0, -1],
    ) // back wall's foot
    cove(
      [WALL_X - COVE_R, COVE_R, 0],
      [WALL_X - COVE_R, COVE_R, 1500],
      [1, 0, 0],
      [0, -1, 0],
    ) // right wall's foot
    cove(
      [WALL_X - COVE_R, 0, COVE_R],
      [WALL_X - COVE_R, CEIL, COVE_R],
      [1, 0, 0],
      [0, 0, -1],
    ) // the corner between them

    /* ---------------- sideboard ---------------- */
    var sb = G('sideboard')
    sb.pos = [-22, 0, 27]
    ;(function () {
      var W = 200,
        D = 46,
        LEG = 15,
        H = 60,
        TOP = 3
      sb.cbox(0, LEG - 2.5, 0, W - 30, 5, D - 10, MAT.TEAK_DARK)
      ;[-1, 1].forEach(function (sx) {
        ;[-1, 1].forEach(function (sz) {
          sb.cyl(
            [sx * (W / 2 - 20), 0, sz * (D / 2 - 8)],
            [sx * (W / 2 - 20), LEG, sz * (D / 2 - 8)],
            1.9,
            MAT.TEAK_DARK,
          )
        })
      })
      sb.cbox(0, LEG + H / 2, -0.5, W - 1, H, D - 1, MAT.GAP)
      sb.cbox(-W / 2 + 1.1, LEG + H / 2, 0, 2.2, H, D, MAT.TEAK)
      sb.cbox(W / 2 - 1.1, LEG + H / 2, 0, 2.2, H, D, MAT.TEAK)
      sb.cbox(0, LEG + H + TOP / 2, 0.5, W + 2, TOP, D + 2, MAT.TEAK)
      sb.cbox(0, LEG + 0.9, 0, W, 1.8, D, MAT.TEAK)
      var fz = D / 2 + 0.6,
        fy0 = LEG + 2.4,
        fy1 = LEG + H - 1.2,
        fh = fy1 - fy0,
        doorW = 60
      ;[-1, 1].forEach(function (s) {
        var x = s * (W / 2 - 2.6 - doorW / 2)
        sb.cbox(x, (fy0 + fy1) / 2, fz, doorW, fh, 2, MAT.TEAK)
        var px = x - s * (doorW / 2 - 10)
        sb.cbox(px, (fy0 + fy1) / 2 + 7, fz + 2.2, 2.8, 22, 3, MAT.TEAK)
        sb.cbox(
          px - s * 1.2,
          (fy0 + fy1) / 2 + 17,
          fz + 2.2,
          5,
          2.4,
          3,
          MAT.TEAK,
        )
      })
      var midW = W - 2 * (2.6 + doorW) - 2.4
      for (var i = 0; i < 3; i++) {
        var dh = (fh - 2) / 3,
          y = fy0 + dh / 2 + i * (dh + 1)
        sb.cbox(0, y, fz, midW, dh - 0.9, 2, MAT.TEAK)
      }
    })()

    /* ---------------- framed print (standing on the sideboard, against the wall) ---------------- */
    cluster(room, KIND, function () {
      var FW = 64,
        FH = 82,
        B = 2.2,
        cx = 32,
        y0 = 78,
        z0 = 3,
        z1 = 6.5
      room.box([cx - FW / 2, y0, z0], [cx + FW / 2, y0 + B, z1], MAT.FRAME)
      room.box(
        [cx - FW / 2, y0 + FH - B, z0],
        [cx + FW / 2, y0 + FH, z1],
        MAT.FRAME,
      )
      room.box([cx - FW / 2, y0, z0], [cx - FW / 2 + B, y0 + FH, z1], MAT.FRAME)
      room.box([cx + FW / 2 - B, y0, z0], [cx + FW / 2, y0 + FH, z1], MAT.FRAME)
      room.box(
        [cx - FW / 2 + B, y0 + B, z0],
        [cx + FW / 2 - B, y0 + FH - B, z0 + 1.5],
        MAT.FRAME,
      )
      var z = z0 + 1.6
      room.quad(
        [cx - FW / 2 + B, y0 + B, z],
        [cx + FW / 2 - B, y0 + B, z],
        [cx + FW / 2 - B, y0 + FH - B, z],
        [cx - FW / 2 + B, y0 + FH - B, z],
        MAT.ART,
      )
    })

    /* ---------------- floor uplighter ---------------- */
    var lampInfo = {}
    ;(function () {
      var bx = -161,
        bz = 30
      room.cyl([bx, 0, bz], [bx, 3.5, bz], 17, MAT.STONE)
      room.tube(
        [
          [bx, 3, bz],
          [bx, 204, bz],
          [bx + 12, 204.5, bz - 2],
        ],
        0.95,
        4,
        MAT.CHROME,
      )
      var hx = bx + 24,
        hy = 205,
        hz = bz - 4
      room.cbox(hx, hy, hz, 30, 2.5, 17, MAT.LAMPHEAD)
      room.cbox(hx, hy - 1.4, hz, 30, 0.4, 17, MAT.CHROME)
      var ey = hy + 1.3
      room.quad(
        [hx - 13, ey, hz - 6.5],
        [hx - 13, ey, hz + 6.5],
        [hx + 13, ey, hz + 6.5],
        [hx + 13, ey, hz - 6.5],
        MAT.UPLIGHT,
      )
      lampInfo = {
        c0: [hx - 13, ey, hz - 6.5],
        e1: [26, 0, 0],
        e2: [0, 0, 13],
        n: [0, 1, 0],
      }
      // cable across the floor
      // in one box around the cable's own nested boxes
      cluster(room, KIND, () =>
        addCable(room, {
          KIND: KIND,
          mat: MAT.LAMPHEAD,
          r: 0.4,
          pts: [
            [bx, 0.4, bz],
            [bx - 13, 0.4, bz + 15],
            [bx - 31, 0.4, bz + 33],
            [bx - 52, 0.4, bz + 49],
            [bx - 68, 0.4, bz + 69],
            [bx - 83, 0.4, bz + 91],
            [bx - 103, 0.4, bz + 104],
            [bx - 128, 0.4, bz + 105],
            [bx - 148, 0.4, bz + 95],
            [bx - 158, 0.4, bz + 78],
            [bx - 163, 0.4, bz + 56],
            [bx - 164, 0.4, bz + 32],
            [bx - 162, 0.4, bz + 10],
            [bx - 159, 0.4, bz - 8],
            [bx - 158, 0.4, bz - 19],
            [bx - 164, 0.4, bz - 25.4],
            [bx - 177, 0.4, bz - 26.8],
            [bx - 195, 0.4, bz - 27],
            [bx - 294, 0.4, bz - 27],
            [bx - 302, 0.5, bz - 27.3],
            [bx - 306.6, 1.16, bz - 28.84],
            [bx - 308, 3.6, bz - 29.58],
            [bx - 308, 8, bz - 29.6],
            [bx - 308, 18, bz - 29.6],
          ],
        }),
      ) // a smooth curve (monstera-scene.js) that runs along the cove at the wall's foot, behind the planter, up to a socket
    })()

    /* ---------------- chrome candle lamp ---------------- */
    var bulbs = []
    var candle = G('candle')
    candle.pos = [122, 0, 50]
    candle.rotY = -0.3
    ;(function () {
      candle.cyl([0, 0, 0], [0, 2, 0], 17, MAT.CHROME)
      var layout = [
        [0, 0, 92],
        [-7, 2, 78],
        [7, 2, 78],
        [-13, 4, 62],
        [13, 4, 62],
        [-4, -6, 70],
        [5, -6, 66],
        [-17, -2, 46],
        [17, -2, 46],
        [0, 8, 50],
        [-10, -8, 42],
        [10, -8, 40],
      ]
      layout.forEach(function (L) {
        var x = L[0],
          z = L[1],
          h = L[2]
        cluster(candle, KIND, function () {
          candle.cyl([x, 2, z], [x, h + 2, z], 3, MAT.CHROME)
          candle.cyl([x, h + 2, z], [x, h + 3.2, z], 1.6, MAT.CHROME)
          candle.sphere([x, h + 5.2, z], 1.5, MAT.BULB)
        })
        bulbs.push([x, h + 5.2, z])
      })
    })()

    /* ---------------- lounge chairs (tubular-steel and leather chairs, after the Wassily B3) ---------------- */
    // Proportions measured from a photo of the B3 (frame centre lines, cm): 76 wide, 60 between the
    // floor runners' ends, arm tubes 62 high. Per side one tube bent into a tall rounded rectangle
    // (runner, front leg, arm, rear leg). A second tube per side rises from the seat's front corner
    // 12 cm inside the front leg, arches out over to a thin arm rail under the arm tube, and runs back
    // to the rear leg. The seat frame runs from the back frame's lower corners, narrowing toward the
    // front. The back frame leans back 19 degrees and is as wide as the side frames. Leather: a seat
    // strap, a top and a lower back strap with bare frame between them, and a flat band on each arm.
    function chair(name, px, pz, rot) {
      var g = G(name)
      g.pos = [px, 0, pz]
      g.rotY = rot
      var HW = 38, // side frame tube centres 76 cm apart
        XB = 37.5, // back frame
        XI = 25.5, // seat front corners and arm risers
        R = 1.25,
        FLOOR = 1.2 + R,
        ZF = 29, // front leg
        ZI = 27, // risers, arches and the seat's front tube
        ZR = -31, // rear leg
        ARM = 63,
        RAIL = ARM - 2, // arm rail under the arm tube
        SEATY = 40, // seat front tube
        LEAN = 0.35, // back frame leans 20.5 degrees: sin
        LEANC = 0.9367, // and cos
        TOP = 75, // back frame top tube
        BY = 38, // back frame lower edge
        BU = (TOP - BY) / LEANC, // its length up the lean
        ZB = ZR + 2.5 + BU * LEAN // its lower edge z
      // tube along corners [x, y, z]
      function tube(c, bend, steps, closed) {
        g.tube(c, R, bend, MAT.CHROME, closed, steps)
      }
      ;[-1, 1].forEach(function (s) {
        var x = s * HW
        tube(
          [
            [x, FLOOR, ZR],
            [x, FLOOR, ZF],
            [x, ARM, ZF],
            [x, ARM, ZR],
          ],
          5,
          6,
          true,
        )
        g.cyl([x, 0, ZF - 3], [x, 1.2, ZF - 3], 1.2, MAT.LAMPHEAD)
        g.cyl([x, 0, ZR + 3], [x, 1.2, ZR + 3], 1.2, MAT.LAMPHEAD)
        // riser, arch and arm rail
        tube(
          [
            [s * XI, SEATY, ZI],
            [s * XI, RAIL, ZI],
            [s * 35.5, RAIL, ZI],
            [s * 35.5, RAIL, ZR + 2.5],
          ],
          4.5,
          5,
          false,
        )
        // arm band: lies on the arm tube and the rail, inner edge 1.7 cm lower, sagging 1 cm
        cluster(g, KIND, function () {
          ;[-0.35, 0.35].forEach(function (d) {
            for (var i = 0; i < 2; i++) {
              var z0 = ZF - 6 - 49.5 * (i / 2),
                z1 = ZF - 6 - 49.5 * ((i + 1) / 2)
              var y0 = 1 - Math.pow((i / 2) * 2 - 1, 2),
                y1 = 1 - Math.pow(((i + 1) / 2) * 2 - 1, 2)
              g.quad(
                [s * 28.5, ARM + d - y0, z0],
                [s * 39, ARM + 1.7 + d - y0, z0],
                [s * 39, ARM + 1.7 + d - y1, z1],
                [s * 28.5, ARM + d - y1, z1],
                MAT.LEATHER,
              )
            }
          })
        })
      })
      // leather wrapped round each arch and down the riser
      cluster(g, KIND, function () {
        ;[-1, 1].forEach(function (s) {
          g.capsule(
            [s * 28, RAIL, ZI],
            [s * 34, RAIL, ZI],
            R + 0.6,
            MAT.LEATHER,
          )
          g.capsule(
            [s * XI, RAIL - 2, ZI],
            [s * XI, RAIL - 11, ZI],
            R + 0.55,
            MAT.LEATHER,
          )
        })
      })
      // seat frame: from the back frame's lower corners to the narrower front
      tube(
        [
          [-XB, BY, ZB],
          [-XI, SEATY, ZI],
          [XI, SEATY, ZI],
          [XB, BY, ZB],
        ],
        4,
        4,
        false,
      )
      // back frame: u runs up the lean from its lower edge
      function bk(u, w) {
        return [w * XB, BY + u * LEANC, ZB - u * LEAN]
      }
      tube([bk(0, -1), bk(0, 1), bk(BU, 1), bk(BU, -1)], 3, 3, true)
      // straps: flat bands across a frame, bowed a little across the width, two skins a hair
      // apart. P(t, w) is the point t along the band and w across it (-1 to 1); n is its unit normal.
      function strap(P, t0, t1, n, sag, skin) {
        function pt(t, w, d) {
          var q = P(t, w),
            o = d - sag * (1 - w * w)
          return [q[0] + n[0] * o, q[1] + n[1] * o, q[2] + n[2] * o]
        }
        cluster(g, KIND, function () {
          ;[-skin, skin].forEach(function (d) {
            for (var j = 0; j < 2; j++) {
              var wa = -1 + j,
                wb = wa + 1
              g.quad(
                pt(t0, wa, d),
                pt(t0, wb, d),
                pt(t1, wb, d),
                pt(t1, wa, d),
                MAT.LEATHER,
              )
            }
          })
        })
      }
      var backN = [0, LEAN, LEANC]
      function bs(u, w) {
        return bk(u, w * 0.96)
      }
      strap(bs, 1.5, 12.5, backN, 0.8, 0.35) // lower back strap
      strap(bs, BU - 11, BU - 0.1, backN, 0.8, 0.35) // top back strap
      // seat strap: from the front tube back to the back frame, sagging 1.4 cm
      strap(
        function (f, w) {
          var xe = XI + (XB - XI) * f
          return [w * xe, SEATY + (BY - SEATY) * f, ZI + (ZB - ZI) * f]
        },
        0.03,
        0.8,
        [0, 1, 0],
        1.4,
        0.35,
      )
      return g
    }
    chair('chairL', -162, 142, 1.05)
    chair('chairR', 110, 140, -1.08)

    /* ---------------- tube-steel tables ---------------- */
    function table(name, px, pz, rot, w, d, h, topMat, topT) {
      var g = G(name)
      g.pos = [px, 0, pz]
      g.rotY = rot
      var R = 1.1
      ;[-1, 1].forEach(function (s) {
        var x = s * (w / 2 - 3)
        g.tube(
          [
            [x, R, -d / 2 + 2],
            [x, R, d / 2 - 2],
            [x, h - R - topT, d / 2 - 2],
            [x, h - R - topT, -d / 2 + 2],
          ],
          R,
          5,
          MAT.CHROME,
          true,
        )
      })
      ;[-d / 2 + 2, d / 2 - 2].forEach(function (z) {
        g.capsule(
          [-w / 2 + 3, h - R - topT, z],
          [w / 2 - 3, h - R - topT, z],
          R * 0.9,
          MAT.CHROME,
        )
      })
      g.box([-w / 2, h - topT, -d / 2], [w / 2, h, d / 2], topMat)
      return g
    }
    table('lowTable', -32, 155, 0.12, 110, 48, 32, MAT.GLASS_DARK, 1.2)
    var tall = table('tallTable', -2, 108, 0.05, 56, 40, 52, MAT.MARBLE, 2)
    tall.cbox(-10, 53.25, 0, 24, 2.5, 17, MAT.BOOK_CREAM)
    tall.cbox(-9, 55.5, -0.5, 21, 2, 15, MAT.BOOK_RED)
    tall.cbox(-8, 57.4, 0, 20, 1.8, 14, MAT.BOOK_CREAM)
    tall.cyl([-15, 58.3, -2], [-15, 59.1, -2], 6, MAT.CERAMIC)
    tall.cyl([-15, 59.1, -2], [-15, 64.4, -2], 3.8, MAT.CERAMIC)

    /* ---------------- rug ---------------- */
    ;(function () {
      var cx = -24,
        cz = 138,
        RW = 162,
        RD = 150
      room.box(
        [cx - RW / 2, 0, cz - RD / 2],
        [cx + RW / 2, 1.4, cz + RD / 2],
        MAT.RUG,
      )
      room.box(
        [cx - RW / 2 + 1, 0, cz + RD / 2],
        [cx + RW / 2 - 1, 0.7, cz + RD / 2 + 7.5],
        MAT.FRINGE,
      )
      room.box(
        [cx - RW / 2 + 1, 0, cz - RD / 2 - 7.5],
        [cx + RW / 2 - 1, 0.7, cz - RD / 2],
        MAT.FRINGE,
      )
    })()

    /* ---------------- monstera in a planter, by the back wall (the uplighter's cable runs behind it to the wall) ---------------- */
    // the scene's 8th group (the renderer takes up to 8), from monstera-scene.js; at the left
    // of the room, where the site's wide frames show it.
    var plant = G('monstera')
    plant.pos = [-425, 0, 60]
    plant.rotY = 0
    addMonstera(plant, { seed: 7, KIND: KIND, MAT: MAT })

    /* ---------------- pack ---------------- */
    function pack() {
      var prims = [],
        gdata = []
      groups.forEach(function (g) {
        var start = prims.length,
          mn = [1e9, 1e9, 1e9],
          mx = [-1e9, -1e9, -1e9]
        g.prims.forEach(function (p) {
          prims.push(p)
          var pts = p.k === KIND.QUAD ? [p.a, p.b, p.c, p.d] : [p.a, p.b]
          pts.forEach(function (q) {
            for (var c = 0; c < 3; c++) {
              mn[c] = Math.min(mn[c], q[c] - p.r - 0.5)
              mx[c] = Math.max(mx[c], q[c] + p.r + 0.5)
            }
          })
        })
        gdata.push({
          name: g.name,
          start: start,
          end: prims.length,
          min: mn,
          max: mx,
          pos: g.pos,
          rotY: g.rotY,
        })
      })
      var W = 5,
        data = new Float32Array(W * prims.length * 4)
      prims.forEach(function (p, i) {
        var o = i * W * 4
        data.set([p.k, p.m, 0, 0], o)
        data.set([p.a[0], p.a[1], p.a[2], p.r], o + 4)
        data.set([p.b[0], p.b[1], p.b[2], 0], o + 8)
        if (p.c) {
          data.set([p.c[0], p.c[1], p.c[2], 0], o + 12)
          data.set([p.d[0], p.d[1], p.d[2], 0], o + 16)
        }
      })
      return { data: data, count: prims.length, width: W, groups: gdata }
    }

    window.ROOM_SCENE = {
      pack: pack,
      MAT: MAT,
      bulbs: bulbs,
      lamp: lampInfo,
      leafMats: plant.leafMats,
      window: {
        c0: [LEFT_X, WIN_Y0, -11],
        e1: [WALL_X - 14 - LEFT_X, 0, 0],
        e2: [0, WIN_Y1 - WIN_Y0, 0],
        n: [0, 0, 1],
      },
    }
  })()

  return window.ROOM_SCENE
}
