// Tracer layout for the handoff question: the City geometry arrives as a stream
// value (the way a fetcher subprocess would deliver it), and the City scene is
// referenced by a short key instead of a data URI — standing in for a tauler
// primitive that rasterizes a drawing once and caches it under that key.
//
// What it measures, per render: the stream string crossing into QuickJS, a
// memoized parse, and the memoized SVG build — everything except the raster.
import { citySceneSvg } from "./CityScene.jsx";

let lastRaw = null;
let lastKey = null;

function sceneKey(raw, width, height) {
  if (raw === lastRaw) return lastKey;
  lastRaw = raw;
  const city = JSON.parse(raw);
  const svg = citySceneSvg(city, width, height, ctx.theme.colors);
  // Stand-in for registering `svg` with tauler; the key is all a render carries.
  lastKey = `tauler:picture/${city.center.join(",")}/${svg.length}`;
  return lastKey;
}

export default function render() {
  const raw = useStringStream("city-fetcher");
  const src = raw ? sceneKey(raw, 248, 96) : null;
  return (
    <root>
      <I3Layout>
        <Panel id="sidebar" anchor="left" size={272}>
          <div class="flex flex-col w-full h-full bg-background p-[12px]">
            <span class="text-foreground">{src ?? "no city yet"}</span>
          </div>
        </Panel>
      </I3Layout>
    </root>
  );
}
