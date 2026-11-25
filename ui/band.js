// Continuous thickness-encoded timeline band geometry (`bandProfile` + `bandPath`).
// Pure zero-DOM leaf module shared by `ui/app.js` and offline unit tests.

export const BAND_JOIN_PX = 2.5;
export const BAND_EASE_PX = 5.0;

function halfThicknessForCount(count, isSubLane) {
  if (count <= 0) return 0;
  if (isSubLane) return 2.0;
  if (count === 1) return 2.5;
  if (count === 2) return 3.5;
  return 4.5;
}

// Merge overlapping or sub-BAND_JOIN_PX intervals within a single session first
// so one session never double-counts itself as 2 concurrent sessions.
function normalizeSessionPixelSpans(rawSpans, minRadius) {
  if (!rawSpans.length) return [];
  const padded = rawSpans
    .map(([x1, x2]) => {
      const mid = (x1 + x2) / 2;
      return x2 - x1 < minRadius * 2 ? [mid - minRadius, mid + minRadius] : [x1, x2];
    })
    .sort((a, b) => a[0] - b[0]);

  const merged = [padded[0].slice()];
  for (let i = 1; i < padded.length; i++) {
    const [a, b] = padded[i];
    const last = merged[merged.length - 1];
    if (a - last[1] <= BAND_JOIN_PX) {
      last[1] = Math.max(last[1], b);
    } else {
      merged.push([a, b]);
    }
  }
  return merged;
}

// Build contiguous active islands of [x0, x1, halfThickness] from per-session pixel spans
export function bandProfile(sessionSpanLists, isSubLane = false) {
  const minR = isSubLane ? 2.0 : 2.5;
  const events = [];
  for (const spans of sessionSpanLists) {
    const clean = normalizeSessionPixelSpans(spans, minR);
    for (const [a, b] of clean) {
      events.push([a, 1], [b, -1]);
    }
  }
  if (!events.length) return [];
  events.sort((p, q) => p[0] - q[0] || q[1] - p[1]);

  const rawSegs = [];
  let curCount = 0;
  let prevX = events[0][0];
  let idx = 0;
  while (idx < events.length) {
    const x = events[idx][0];
    if (x > prevX + 1e-4) {
      rawSegs.push([prevX, x, curCount]);
      prevX = x;
    }
    while (idx < events.length && Math.abs(events[idx][0] - x) <= 1e-4) {
      curCount += events[idx][1];
      idx++;
    }
  }

  // Bridge zero-count gaps narrower than BAND_JOIN_PX to prevent pinched waists
  const bridged = [];
  for (let i = 0; i < rawSegs.length; i++) {
    const [a, b, cnt] = rawSegs[i];
    if (cnt === 0 && b - a < BAND_JOIN_PX && bridged.length > 0 && i + 1 < rawSegs.length && rawSegs[i + 1][2] > 0) {
      bridged.push([a, b, Math.min(bridged[bridged.length - 1][2], rawSegs[i + 1][2])]);
    } else {
      bridged.push([a, b, cnt]);
    }
  }

  // Group positive segments into contiguous active islands, coalescing equal adjacent radii
  const islands = [];
  let cur = [];
  for (const [a, b, cnt] of bridged) {
    if (cnt <= 0) {
      if (cur.length) {
        islands.push(cur);
        cur = [];
      }
      continue;
    }
    const r = halfThicknessForCount(cnt, isSubLane);
    if (cur.length && Math.abs(cur[cur.length - 1][2] - r) < 1e-6) {
      cur[cur.length - 1][1] = b;
    } else {
      cur.push([a, b, r]);
    }
  }
  if (cur.length) islands.push(cur);
  return islands;
}

function capsulePath(a, b, cy, r) {
  const mid = (a + b) / 2;
  const xL = b - a <= 2 * r ? mid : a + r;
  const xR = b - a <= 2 * r ? mid : b - r;
  const top = (cy - r).toFixed(2);
  const bot = (cy + r).toFixed(2);
  const lx = xL.toFixed(2);
  const rx = xR.toFixed(2);
  const rf = r.toFixed(2);
  if (Math.abs(xR - xL) < 0.05) {
    return `M${lx} ${top}A${rf} ${rf} 0 0 1 ${lx} ${bot}A${rf} ${rf} 0 0 1 ${lx} ${top}Z`;
  }
  return `M${lx} ${top}L${rx} ${top}A${rf} ${rf} 0 0 1 ${rx} ${bot}L${lx} ${bot}A${rf} ${rf} 0 0 1 ${lx} ${top}Z`;
}

// Convert one island `[[x0, x1, r0], [x1, x2, r1], ...]` into a smooth C1 closed SVG path
// using closed-form triangular-window convolution: h(x) = (G(x+E) - 2*G(x) + G(x-E)) / E^2
export function bandPath(island, cy) {
  const m = island.length;
  const a = island[0][0];
  const b = island[m - 1][1];
  const rStart = island[0][2];
  const rEnd = island[m - 1][2];

  // Fast path: uniform thickness or too short to fit internal transitions
  if (m === 1 || b - a <= rStart + rEnd + 2.0) {
    const rUniform = m === 1 ? rStart : Math.max(...island.map((s) => s[2]));
    return capsulePath(a, b, cy, rUniform);
  }

  const xL = a + rStart;
  const xR = b - rEnd;

  // Precompute knots x_k, first integral F_k, and double integral G_k relative to x_0 = a
  const knots = new Float64Array(m + 1);
  const radii = new Float64Array(m);
  const F = new Float64Array(m + 1);
  const G = new Float64Array(m + 1);
  knots[0] = a;
  for (let k = 0; k < m; k++) {
    const [, xNext, rk] = island[k];
    const dx = xNext - knots[k];
    knots[k + 1] = xNext;
    radii[k] = rk;
    F[k + 1] = F[k] + rk * dx;
    G[k + 1] = G[k] + F[k] * dx + 0.5 * rk * dx * dx;
  }

  function evalG(x) {
    if (x <= a) {
      const d = x - a;
      return 0.5 * rStart * d * d;
    }
    if (x >= b) {
      const d = x - b;
      return G[m] + F[m] * d + 0.5 * rEnd * d * d;
    }
    let lo = 0;
    let hi = m - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (knots[mid] <= x) lo = mid;
      else hi = mid - 1;
    }
    const d = x - knots[lo];
    return G[lo] + F[lo] * d + 0.5 * radii[lo] * d * d;
  }

  const E = BAND_EASE_PX;
  const invE2 = 1.0 / (E * E);
  const smoothH = (x) => (evalG(x + E) - 2.0 * evalG(x) + evalG(x - E)) * invE2;

  // Merge transition windows [x_k - E, x_k + E] clamped to [xL, xR]; sample 1px inside, skip flat spans
  const windows = [];
  for (let k = 1; k < m; k++) {
    const w0 = Math.max(xL, knots[k] - E);
    const w1 = Math.min(xR, knots[k] + E);
    if (w1 > w0) {
      if (windows.length && w0 <= windows[windows.length - 1][1] + 0.5) {
        windows[windows.length - 1][1] = Math.max(windows[windows.length - 1][1], w1);
      } else {
        windows.push([w0, w1]);
      }
    }
  }

  const xs = [xL];
  for (const [w0, w1] of windows) {
    if (w0 > xs[xs.length - 1] + 0.25) xs.push(w0);
    const startPx = Math.ceil(w0);
    const endPx = Math.floor(w1);
    for (let px = startPx; px <= endPx; px++) {
      if (px > xs[xs.length - 1] + 0.2 && px < xR - 0.2) xs.push(px);
    }
    if (w1 > xs[xs.length - 1] + 0.25 && w1 < xR - 0.2) xs.push(w1);
  }
  if (xR > xs[xs.length - 1] + 0.05) xs.push(xR);

  const hs = xs.map(smoothH);
  const n = xs.length;
  const hStart = hs[0];
  const hEnd = hs[n - 1];

  let d = `M${xs[0].toFixed(2)} ${(cy - hStart).toFixed(2)}`;
  for (let i = 1; i < n; i++) {
    d += `L${xs[i].toFixed(2)} ${(cy - hs[i]).toFixed(2)}`;
  }
  d += `A${hEnd.toFixed(2)} ${hEnd.toFixed(2)} 0 0 1 ${xs[n - 1].toFixed(2)} ${(cy + hEnd).toFixed(2)}`;
  for (let i = n - 2; i >= 0; i--) {
    d += `L${xs[i].toFixed(2)} ${(cy + hs[i]).toFixed(2)}`;
  }
  d += `A${hStart.toFixed(2)} ${hStart.toFixed(2)} 0 0 1 ${xs[0].toFixed(2)} ${(cy - hStart).toFixed(2)}Z`;
  return d;
}
