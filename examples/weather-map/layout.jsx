// Tracer layout: the sidebar weather card with a City scene behind it, in a
// panel the width of the real sidebar (272 px). `examples/weather_map_tracer.rs`
// evaluates and rasterizes it; `__CITY__` is replaced with a file in `.data/`,
// `__LOOK__` with a key of LOOKS.
//
// The weather reading is a constant: the tracer measures drawing, and a stream
// would only add a subprocess to wait for.
//
// DECISION (Q6, answered): the home is the dotfiles-tauler Package, in pieces;
// this file is the tracer's stand-in for the card there.
import city from "./.data/__CITY__.js";
import CityScene, { LOOKS } from "./CityScene.jsx";

const CARD_W = 248;
const CARD_H = 96;
const reading = { cond: "Partly cloudy", temp: "+17°C", feels: "+16°C", humidity: "64%", uv: "3" };

function WeatherCard({ w }) {
  return (
    <div style={{ position: "relative", width: CARD_W, height: CARD_H }} class="rounded-lg overflow-hidden">
      <CityScene city={city} width={CARD_W} height={CARD_H} look={LOOKS.__LOOK__} />
      {/* Veil: keeps the text readable on any city. Fades the top-left corner,
          where the temperature sits, and leaves the skyline on the right. */}
      <div
        style={{ position: "absolute", top: 0, left: 0, width: CARD_W, height: CARD_H }}
        class="bg-gradient-to-r from-background/80 via-background/30 to-transparent"
      />
      <div style={{ position: "relative" }} class="flex flex-col gap-[2px] p-[10px]">
        <span class="text-[22px] text-foreground font-bold">{w.temp}</span>
        <span class="text-[10px] text-foreground">{w.cond}</span>
        <span class="text-[10px] text-muted-foreground">feels {w.feels} · RH {w.humidity}</span>
      </div>
    </div>
  );
}

export default function render() {
  return (
    <root>
      <I3Layout>
      <Panel id="sidebar" anchor="left" size={272}>
        <div class="flex flex-col w-full h-full bg-background p-[12px]">
          <WeatherCard w={reading} />
        </div>
      </Panel>
      </I3Layout>
    </root>
  );
}
