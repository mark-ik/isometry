# Isometry

**Status:** first cut authored 2026-07-05 from the founding design session.
This file is maintainer-owned per DOC_POLICY §6; edit freely, Mark.

## What it is

Isometry is a pixel-art isometric virtual tabletop. One player (the DM)
prepares maps in the built-in editor and hosts a session; the others join
peer-to-peer, no server, and play through the maps in turns. It is a
substrate for tabletop systems (D&D, Pathfinder, others), not a game with
rules of its own.

The feel target is a GBA-era tactics RPG: Tactics Ogre: The Knight of
Lodis, Final Fantasy Tactics Advance. Fixed isometric camera, 2:1 diamond
tiles, sculpted elevation, chunky sprites at low internal resolution,
integer-scaled. The bet is that easy modding beats production value: if
drawing a tileset is one Aseprite file and a manifest, groups will make
their own campaigns.

## Pillars

1. **The DM is the server.** Sessions are hosted from the DM's app over
   p2p transport. Players join with a ticket string. Turn-based play
   needs an ordered event log, not netcode.
2. **Battle-scale maps, sculpted.** Lodis-scale boards (roughly 15x15 to
   30x30) with per-tile height as a first-class editing brush. Elevation,
   facing, and turn order are substrate features because the reference
   games treat them as terrain, not rules.
3. **Modding is folders and stylesheets.** A tileset is sprites plus a
   manifest; appearance binds through CSS class vocabulary; a campaign
   can reskin the world without touching the app.
4. **Systems are plugins.** Character and item definitions are schemas;
   derived stats and dice behavior are scripts (rhai). The substrate
   tracks geometry and turns, never hit points. 5e SRD (CC-BY-4.0) and
   Pathfinder 2e (ORC) are the first-party system candidates.
5. **Players eventually join from a browser.** The serval-web lane makes
   a no-install player client plausible from the same codebase. Native DM
   app first.

## Feature sketch (unplanned items are aspirational)

- Map editor: tile palette, layers, height brush, props, spawn points,
  fog regions, GM notes.
- Play: token movement with path preview, facing, initiative modes
  (individual speed order or side-based), area templates, dice roller,
  measurement, per-player fog, GM whispers.
- Character sheets: schema-driven, system plugin supplies structure and
  roll formulas.
- Campaign packs: maps, tilesets, sheets, and system choice in one
  distributable bundle.
