#!/usr/bin/env bash
# City fetcher: the subprocess that hands a layout the City geometry for a
# weather location. Stub — design only, nothing here runs yet.
#
#   const raw = useStringStream("/usr/bin/bash", cityFetcher(location));
#
# PROVISIONAL (Q1–Q7): the City scene this feeds is still being designed; see
# CityScene.jsx for the paths (P1-P5) these SPECs serve.
#
# DECISION: coordinates come from the same `location` string <Weather> already
# uses, asked of wttr.in again as `?format=j1`: its `nearest_area[0]` carries
# `latitude`/`longitude` to three decimals (about 100 m), for a named place and
# for the empty, IP-geolocated location alike. Measured 225–318 ms per request.
# Rejected: changing <Weather> to emit coordinates — it would switch its own
# request to the much larger j1 format and change its output shape for every
# existing <WeatherSummary> user. Rejected: a separate geocoder (Nominatim) — a
# second service that could disagree with wttr.in about where "here" is.
#
# DECISION: the geometry travels as one stream line. The stream reader has no
# line limit (BufReader::lines, src/data/mod.rs), and passing a 0.2–0.6 MB line
# into QuickJS costs 0.05–0.17 ms per render (tracer, layout-stream.jsx) when
# the layout memoizes on the raw string, as layout-stream.jsx does.
# Rejected: `useJSONStream`, which re-parses the whole string on every render
# (tauler-core/src/globals.rs).
#
# SPEC (P1): on start, resolve coordinates, then print the cached City geometry
# for them as one JSON line if the cache has it, before any network request.
# SPEC (P1): cache at ${XDG_CACHE_HOME:-~/.cache}/tauler/city/<lat>,<lon>,<radius>.json,
# written to a temp file and renamed, so a killed fetch never leaves half a file
# (same rule as the Package cache, ADR 0041).
# SPEC (P2): if Overpass fails or times out, print nothing and retry with
# backoff (60 s, doubling, capped at 1 h). Never print a partial or empty
# geometry: no line at all is what makes the card fall back to no scene.
# SPEC (P3): re-resolve coordinates every 6 h; a move of more than RADIUS/4
# fetches and prints the new geometry. Smaller moves keep the old scene, so an
# IP geolocation that wobbles between requests does not refetch (whether it
# wobbles at all was not measured; the threshold costs nothing if it does not).
#
# RISK: Overpass's public instance has a fair-use policy (limits not checked
# here); a Package shared by many users still makes one request per user per
# location, cached forever. Kill: Overpass blocking a shared User-Agent or
# returning 429 for first-time fetches → per-user User-Agent, or a mirror.
# Cost: small (read their usage policy; watch for 429 in the fetcher's log).
#
# ASK (Q7): OpenStreetMap data is ODbL; publicly shown maps credit
# "© OpenStreetMap contributors". A scene on your own screen is not a public
# display, but a Package others install puts it on theirs.
# Recommend: credit OSM in the Package's README and in this file, not on the card.
#
# Pseudocode:
#   coords = wttr.in/<location>?format=j1 | jq nearest_area[0] lat,lon
#   cache  = city/<coords>,<radius>.json
#   [ -f cache ] && cat cache
#   loop: fetch-city (Overpass, compact) → tmp → mv cache → cat cache; break
#         on failure: sleep backoff
#   every 6 h: re-resolve coords; moved > radius/4 → fetch again
