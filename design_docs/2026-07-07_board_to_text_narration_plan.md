# Board-to-Text Narration Plan

**Date:** 2026-07-07
**Status:** active plan. Near-term buildable, un-gated. Scoped out of the
optional-intelligence vision ([2026-07-07_optional_intelligence_vision.md](2026-07-07_optional_intelligence_vision.md),
section 8): the factual layer needs no model and no ABI widening, so it
escapes the post-keystone gating and lands early. Done-conditions, not time
estimates.

## Why

A deterministic serializer that turns board state into prose facts. It is
the shared perception primitive: accessibility and text-only play, session
recap, and a future model's grounding context all read the same facts.
(2026-07-09: plus the "meanwhile" faction-turn interstitial, worldbuilding
plan rung 7, which renders committed faction moves between local maps.) Pure
substrate geometry, alongside `template` and `visibility`, so it lives in
`isometry-core` and stays free of UI, I/O, and models. The world compass
matches `Facing`: north is decreasing row, south increasing row, east
increasing col, west decreasing col.

## Phase N1: scene serializer (factual, omniscient)

A `narrate` module in `isometry-core`.

- `facing_word(Facing) -> &str` and `bearing(from, to) -> &str` (eight-way
  world bearing on the `Facing` axes, "here" when tiles coincide).
- `describe_token(map, id) -> Option<String>`: label ("knight 1"), owner,
  facing, position, and a terrain/elevation note from the ground kind and
  height field.
- `describe_scene(map) -> String`: board name and size, then one line per
  token.

**Done when:** a known board renders to accurate text (labels, owners,
facings, positions, terrain, elevation) verified by tests over a fixture
board.

## Phase N2: viewer-relative and fog-aware

- `describe_from(map, viewer, sight) -> Option<String>`: narrate from one
  token's eyes. Compute the viewer's visible set with `visible_from`, then
  describe only the tokens standing on visible tiles (fog: unseen tokens are
  omitted), each as distance + bearing + facing relative to the viewer.

**Done when:** a viewer's narration lists an in-sight token with the correct
distance and bearing, and omits a token that is out of range or behind a
wall, verified by tests.

## Phase N3: model fluency pass (deferred, gated)

An optional pass behind the host's provider seam (the promoted `vates`
crate) that rephrases the N1/N2 facts into prose. Post-keystone; out of
scope here. The factual layers above stand alone without it.

## Findings

- 2026-07-07: core exposes exactly the primitives this needs, so N1/N2 are
  pure additions with no substrate change. `TileCoord = (col, row)`;
  `distance` is Chebyshev (template.rs); `visible_from(map, origin, &SightRules)`
  gives the fog set (visibility.rs); `Facing` fixes the world compass
  (north = decreasing row, map.rs).

## Progress

- 2026-07-07 (N1 + N2 landed): `isometry-core::narrate` module. `facing_word`
  and `bearing` (eight-way on the Facing axes), `describe_token` and
  `describe_scene` (omniscient factual), `describe_from` (viewer-relative,
  fog-aware via `visible_from`, omits tokens on unseen tiles). Pure additive,
  no substrate change. 9 tests (45 core total). Verified standalone (the
  workspace build was transiently blocked by an unrelated serval/stylo pin
  conflict during a concurrent edit). N3 (model fluency) stays deferred until
  the vates seam lands.
