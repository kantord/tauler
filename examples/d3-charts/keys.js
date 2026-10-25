// A <D3Figure> key that changes whenever the data does: FNV-1a over its JSON.
// It cannot be fooled the way a count or a last timestamp can — two graphs of
// the same size, or a series whose last point is unchanged but whose window
// moved.
//
// Hashed once per object, not once per tick: re-hashing every chart's data on
// every render took the tracer's per-tick eval from 0.7 to 4.9 ms. Data is plain
// JSON that arrives as a new object whenever it changes (a stream line is parsed
// afresh; ADR 0013), so the object is a safe cache key for its own hash. Data
// mutated in place would keep its old key — don't.
//
// The hash is of the content, never of the object's identity: the render
// runtime and the draw runtime each evaluate the layout, and a figure is found
// in the draw runtime by its key, so both must compute the same one.
const hashes = new WeakMap();

export function contentKey(value) {
  if (value !== null && typeof value === "object") {
    let hash = hashes.get(value);
    if (hash === undefined) hashes.set(value, (hash = hashText(JSON.stringify(value))));
    return hash;
  }
  return hashText(JSON.stringify(value));
}

function hashText(text) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(36);
}
