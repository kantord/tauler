/* tauler generative wallpaper. Six deterministic fields, one per workspace.
   Seeded: the same workspace always draws the same art, so a panel can
   redraw an offset slice of it and match the wallpaper pixel for pixel.
   Nothing animates. */

export const FIELDS = ["flow", "truchet", "moire", "contour", "quasi", "stipple"];

export function drawField(canvas, kind = "flow", hex = "#F0855A", seedIndex = 0) {
  const ctx = canvas.getContext("2d");
  const dpr = 2, w = canvas.width / dpr, h = canvas.height / dpr;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  ctx.lineCap = "round";
  let s = 9001 + seedIndex * 7331;
  const R = () => (s = (s * 1664525 + 1013904223) % 4294967296) / 4294967296;
  const n = parseInt(hex.slice(1), 16), cr = n >> 16 & 255, cg = n >> 8 & 255, cb = n & 255;
  const A = (a) => "rgba(" + cr + "," + cg + "," + cb + "," + a + ")";
  const W = (a) => "rgba(238,236,240," + a + ")";

  if (kind === "flow") {
    const ang = (x, y) => Math.sin(x * 0.0041) * 1.7 + Math.cos(y * 0.0053) * 1.4 + Math.sin((x + y) * 0.0016) * 0.9;
    for (let p = 0; p < 1500; p++) {
      let x = R() * w, y = R() * h;
      ctx.strokeStyle = R() < 0.22 ? W(0.05 + R() * 0.05) : A(0.05 + R() * 0.08);
      ctx.lineWidth = R() < 0.15 ? 1.6 : 0.9;
      ctx.beginPath(); ctx.moveTo(x, y);
      for (let k = 0; k < 58; k++) {
        const t = ang(x, y); x += Math.cos(t) * 3.4; y += Math.sin(t) * 3.4;
        if (x < -20 || x > w + 20 || y < -20 || y > h + 20) break;
        ctx.lineTo(x, y);
      }
      ctx.stroke();
    }
  } else if (kind === "truchet") {
    const g = 44, r = g / 2;
    for (let gy = 0; gy < h + g; gy += g) for (let gx = 0; gx < w + g; gx += g) {
      ctx.lineWidth = 1.3;
      ctx.strokeStyle = R() < 0.18 ? W(0.09) : A(0.10 + R() * 0.12);
      ctx.beginPath();
      if (R() < 0.5) {
        ctx.arc(gx, gy, r, 0, Math.PI / 2);
        ctx.moveTo(gx + g - r, gy + g); ctx.arc(gx + g, gy + g, r, Math.PI, Math.PI * 1.5);
      } else {
        ctx.arc(gx + g, gy, r, Math.PI / 2, Math.PI);
        ctx.moveTo(gx + r, gy + g); ctx.arc(gx, gy + g, r, Math.PI * 1.5, Math.PI * 2);
      }
      ctx.stroke();
      if (R() < 0.045) { ctx.fillStyle = A(0.10); ctx.fillRect(gx + 3, gy + 3, g - 6, g - 6); }
    }
  } else if (kind === "moire") {
    const cs = [[w * 0.34, h * 0.46], [w * 0.62, h * 0.58], [w * 0.9, h * 0.2]];
    cs.forEach((c, ci) => {
      for (let rr = 6; rr < Math.max(w, h); rr += 11) {
        ctx.beginPath();
        ctx.ellipse(c[0], c[1], rr, rr * (0.92 + ci * 0.07), 0, 0, Math.PI * 2);
        ctx.strokeStyle = ci === 2 ? W(0.045) : A(0.055 + (ci ? 0.02 : 0));
        ctx.lineWidth = 1; ctx.stroke();
      }
    });
  } else if (kind === "contour") {
    for (let li = 0; li < 84; li++) {
      const y0 = (li / 83) * (h + 60) - 30;
      ctx.beginPath();
      for (let x = -10; x <= w + 10; x += 5) {
        const y = y0
          + Math.sin(x * 0.0105 + li * 0.34) * 20 * Math.sin(li * 0.11 + 0.6)
          + Math.sin(x * 0.0037 + li * 0.9) * 13
          + Math.cos(x * 0.021 + li * 0.2) * 4;
        x === -10 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
      }
      ctx.strokeStyle = li % 7 === 0 ? W(0.07) : A(0.07 + 0.06 * Math.abs(Math.sin(li * 0.19)));
      ctx.lineWidth = li % 7 === 0 ? 1.4 : 0.9;
      ctx.stroke();
    }
  } else if (kind === "quasi") {
    const step = 13, ks = [0, 1, 2, 3, 4].map(k => (k * Math.PI) / 5);
    for (let y = step; y < h; y += step) for (let x = step; x < w; x += step) {
      let v = 0;
      for (const a2 of ks) v += Math.cos((x * Math.cos(a2) + y * Math.sin(a2)) * 0.055);
      const t = (v + 5) / 10, rad = 0.4 + t * 3.4;
      if (rad < 0.7) continue;
      ctx.beginPath(); ctx.arc(x, y, rad, 0, Math.PI * 2);
      ctx.fillStyle = t > 0.86 ? W(0.16) : A(0.07 + t * 0.16);
      ctx.fill();
    }
  } else {
    for (let p = 0; p < 26000; p++) {
      const x = R() * w, y = R() * h;
      const dx = (x - w * 0.44) / (w * 0.62), dy = (y - h * 0.5) / (h * 0.7);
      const d = Math.sqrt(dx * dx + dy * dy);
      if (R() < d * 0.95) continue;
      ctx.strokeStyle = R() < 0.12 ? W(0.06) : A(0.05 + R() * 0.07);
      ctx.lineWidth = 0.9;
      ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(x + 3.4, y - 5.4); ctx.stroke();
    }
  }


  /* Film grain. A 128px noise tile, source-over at very low alpha, laid
     down BEFORE the erase so it fades with the field instead of stopping
     at the stage edge. Kills the banding in the falloff and makes the
     accent read as light rather than as CSS. */
  const tile = document.createElement("canvas");
  tile.width = tile.height = 128;
  const tctx = tile.getContext("2d");
  const img = tctx.createImageData(128, 128);
  for (let q = 0; q < img.data.length; q += 4) {
    const v = 150 + Math.floor(R() * 105);
    img.data[q] = v; img.data[q + 1] = v; img.data[q + 2] = v;
    img.data[q + 3] = Math.floor(R() * 16);
  }
  tctx.putImageData(img, 0, 0);
  const pat = ctx.createPattern(tile, "repeat");
  if (pat) { ctx.fillStyle = pat; ctx.fillRect(0, 0, w, h); }

  /* Every field is erased outward so it dissolves into the surface
     instead of ending at an edge. This is not optional. */
  ctx.globalCompositeOperation = "destination-out";
  const grd = ctx.createRadialGradient(w * 0.44, h * 0.5, Math.min(w, h) * 0.2, w * 0.44, h * 0.5, Math.max(w, h) * 0.72);
  grd.addColorStop(0, "rgba(0,0,0,0)");
  grd.addColorStop(0.55, "rgba(0,0,0,0.28)");
  grd.addColorStop(1, "rgba(0,0,0,0.94)");
  ctx.fillStyle = grd; ctx.fillRect(0, 0, w, h);
  ctx.globalCompositeOperation = "source-over";
}
