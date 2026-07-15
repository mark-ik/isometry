//! The placeholder tileset, expressed the way real ones will be: a
//! stylesheet over the tile-kind class vocabulary. Colored clip-path
//! diamonds stand in for sprites until a pixel tileset lands (and the
//! `image-rendering: pixelated` engine seam opens, probe P1).

/// The whole app sheet: chrome plus the placeholder tileset.
pub fn board_css() -> String {
    let mut css = r#"
.app {
    width: 100%;
    height: 100%;
    display: flex;
    background-color: #14161d;
    color: #cfd3dd;
    font-family: sans-serif;
    font-size: 13px;
}
.side {
    width: 200px;
    height: 100%;
    padding: 14px;
    background-color: #1b1e27;
    overflow-y: auto;
}
.side-title {
    font-size: 18px;
    font-weight: bold;
    margin-bottom: 10px;
    color: #e8ebf2;
}
.side-line { margin-bottom: 4px; }
.side-strong { color: #ffd766; margin-top: 8px; }
.side-hint { color: #8b91a0; font-size: 12px; margin-top: 6px; }
.side-heading {
    color: #8b91a0;
    font-size: 11px;
    margin-top: 12px;
    margin-bottom: 4px;
}
.side-status { color: #9fd48a; font-size: 12px; margin-top: 10px; min-height: 14px; }

.btn-row { display: flex; flex-wrap: wrap; }
.btn {
    padding: 3px 8px;
    margin-right: 4px;
    margin-bottom: 4px;
    background-color: #262b38;
    color: #cfd3dd;
    font-size: 12px;
}
.btn:hover { background-color: #323949; }
.btn-active { background-color: #3d4666; color: #ffffff; }
.btn-dim { color: #5b6070; }

.swatch-row { display: flex; flex-wrap: wrap; }
.swatch {
    width: 22px;
    height: 22px;
    margin-right: 4px;
    margin-bottom: 4px;
    background-color: #10131a;
}
.swatch-active { border: 2px solid #ffd766; }
.sprite-swatch {
    width: 24px;
    height: 36px;
    background-repeat: no-repeat;
    background-size: 100% 100%;
    image-rendering: pixelated;
    background-color: #10131a;
}

.turn-list { margin-bottom: 4px; }
.turn-row {
    display: flex;
    padding: 2px 4px;
    font-size: 12px;
}
.turn-row-active { background-color: #2c3347; }
.turn-row-selected { color: #9fd48a; }
.turn-label { flex: 1; }
.btn-mini { padding: 1px 6px; font-size: 11px; margin-bottom: 4px; }

.roll-log { margin-top: 4px; margin-bottom: 2px; }
.roll-line { color: #cfd3dd; font-size: 12px; margin-bottom: 2px; }

/* Ground markers under tokens: gold = whose turn, green = selected. */
.marker {
    position: absolute;
    width: 28px;
    height: 14px;
    clip-path: polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%);
}
.marker-turn { background-color: #ffd766; }
.marker-select { background-color: #7fd8a3; }

/* Mirror about the sprite's own center. genet conjugates transforms
   at the box origin (spec default is 50% 50%, engine gap on file), so
   pre-translate by the width to land the reflection back in the box. */
.token-flip { transform: translateX(24px) scaleX(-1); }

/* Character-sheet overlay: a panel in the board pane. */
.sheet {
    position: absolute;
    left: 16px;
    top: 16px;
    width: 240px;
    padding: 12px;
    background-color: #1b1e27;
    border: 1px solid #3d4666;
    color: #cfd3dd;
    font-size: 13px;
}
.sheet-header { display: flex; margin-bottom: 8px; }
.sheet-title { flex: 1; font-size: 16px; font-weight: bold; color: #e8ebf2; }
.sheet-row { display: flex; margin-bottom: 3px; }
.sheet-field { flex: 1; }
.sheet-heading { color: #8b91a0; font-size: 11px; margin-top: 8px; margin-bottom: 4px; }
.sheet-mods { display: flex; flex-wrap: wrap; }
.sheet-mods .stat-row { min-width: 72px; font-size: 12px; margin-bottom: 2px; }
.sheet-actions { display: flex; flex-wrap: wrap; }

/* Right-click token context menu: a small card at the click position. */
.context-menu {
    position: absolute;
    min-width: 100px;
    padding: 4px;
    background-color: #1b1e27;
    border: 1px solid #3d4666;
    z-index: 100000;
}
.menu-title { color: #8b91a0; font-size: 11px; padding: 2px 6px 4px; }
.menu-item { padding: 4px 8px; color: #cfd3dd; font-size: 13px; }
.menu-item:hover { background-color: #323949; }

.pane {
    position: relative;
    flex: 1;
    overflow: hidden;
    background-color: #101218;
}
.board { position: absolute; }

.tile {
    position: absolute;
    width: 32px;
    height: 16px;
    clip-path: polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%);
}

.tile-grass { background-color: #4f8f3b; }
.tile-grass.alt { background-color: #478536; }
.tile-water { background-color: #2f629e; }
.tile-water.alt { background-color: #2a5a93; }
.tile-stone { background-color: #8d9098; }
.tile-stone.alt { background-color: #84878f; }
.tile-under { background-color: #3b5c2d; }

/* State tints come after the kind classes: equal specificity, source
   order decides, and these must win over any tile kind. */
.tile:hover { background-color: #ffe9a0; }
.tile-selected { background-color: #ffd766; }
/* Play mode: the selected token's reach, and the hovered path in it. */
.tile-reach { background-color: #4a6ea8; }
.tile-reach.alt { background-color: #44669c; }
.tile-path { background-color: #7fa3d8; }
.tile-path.alt { background-color: #7fa3d8; }
/* Area template preview (measure mode). */
.tile-template { background-color: #d98a4a; }
.tile-template.alt { background-color: #cf8040; }
/* A transition point: the door to another map. Walk onto it to cross. */
.tile-door { background-color: #9a7bd8; }
.tile-door.alt { background-color: #8f70cc; }

/* Fog of war: a dim shroud over explored-but-unseen tiles. Unexplored
   tiles are simply not drawn, so the dark pane behind the board shows. */
.fog-shroud {
    position: absolute;
    width: 32px;
    height: 16px;
    clip-path: polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%);
    background-color: rgba(8, 10, 16, 0.6);
}

.prop {
    position: absolute;
    width: 20px;
    height: 24px;
}
.prop-tree {
    background-color: #2d5b27;
    clip-path: polygon(50% 0%, 100% 78%, 62% 78%, 62% 100%, 38% 100%, 38% 78%, 0% 78%);
}

/* The beat box: what a token *does*. It carries the board position and plays
   the beat. The sprite inside carries appearance and facing. They are two boxes
   because `.token-flip` already owns the sprite's `transform`, and a CSS
   animation on `transform` outranks a normal declaration: one box would mean a
   west-facing knight loses his mirror the instant he swings. */
.beat {
    position: absolute;
    width: 24px;
    height: 36px;
}
/* Tokens: 8x12 pixel sprites at 3x, nearest-neighbor (probe P1). */
.token {
    position: absolute;
    left: 0;
    top: 0;
    width: 24px;
    height: 36px;
    background-repeat: no-repeat;
    background-size: 100% 100%;
    image-rendering: pixelated;
}
/* Pack-layer CSS: `effect:flame` on an equipped public item becomes
   `.token-layer-effect-flame`. A full rig may supply its own layer rule; this
   starter rule makes the W1 equipment projection visible on the live board. */
.token-layer-effect-flame { box-shadow: 0 0 0 2px #e38a34, 0 -3px 0 #ffd766; }

/* Beat vocabulary lives in the campaign pack, not here: `strike`, `recoil`,
   `dodge`, `fall`, `cheer` and the rest are supplied as `@keyframes` plus a
   `.beat-<name>` rule by whatever pack the table loaded (see the `core` pack's
   `beats/*.css`). A campaign that wants a different swing draws a different
   swing, and the app is not the thing that decides a table may cheer but not
   spit. What stays here is only what is *structural* rather than art direction:
   the box a beat plays on, the pose a fallen token holds, and the directional
   force beats, which are generated from the board's own projection so a shove
   travels exactly one tile (see `force_css`). */

/* Out of play: dimmed and slumped, held after the fall beat is cleared. A
   corpse is not clickable as a target (the view withholds `.beat-targetable`). */
.beat-down {
    transform: translate(6px, 10px);
    opacity: 0.55;
}

/* Conditions render as `cond-<name>` classes on the beat wrapper, so a pack can
   style them like it styles beats. This prone pose is the structural default a
   pack may override: half-slumped, still alive. */
.cond-prone {
    transform: translate(4px, 6px);
    opacity: 0.8;
}

/* Target-pick mode: everything clickable reads as a victim. */
.beat-targetable { cursor: crosshair; }

/* An adjudicated action is a verb aimed at someone, not another passive check. */
.btn-attack { background-color: #5a2b2b; color: #ffd9d9; }
.btn-attack:hover { background-color: #7a3a3a; }
"#
    .to_owned();
    // Voxel-baked pixel tileset: the pixel sprites this sheet was waiting for
    // (design_docs/2026-07-08_campaign_packs_plan.md). Knight is the demo rig;
    // goblin is the same rig recoloured, proving palette-swap on the board.
    css.push_str(&voxel_token_css());
    css.push_str(&force_css());
    css.push_str(COMPENDIUM_CSS);
    css
}

/// The directional force beats, generated from the projection so a shove lands
/// exactly one tile away rather than a guessed number of pixels.
///
/// Two families, one geometry, and the whole design in eight lines of output:
///
/// - `staggered-<dir>`: **a flourish.** Out one tile, hold, walk back. The token
///   never leaves its square, so nothing here is state and peers are free to
///   disagree about the sprite's position mid-stagger.
/// - `shoved-<dir>`: **truth.** The board has already put the token on its new
///   tile, so this runs the other way: start where it *used* to be and slide in.
///   Same keyframes, reversed, and the only difference that matters is that one
///   of them changed the game and the other did not.
fn force_css() -> String {
    let geo = isometry_core::IsoGeometry::default();
    let mut css = String::from("\n/* Force beats: see force_css(). */\n");
    for (dcol, drow) in [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ] {
        let dir = isometry_core::compass((dcol, drow)).expect("unit step has a compass name");
        // The same projection the board uses: one tile step in screen pixels.
        let dx = (dcol - drow) as f32 * (geo.tile_w / 2.0);
        let dy = (dcol + drow) as f32 * (geo.tile_h / 2.0);
        css.push_str(&format!(
            "\
@keyframes iso-stagger-{dir} {{
    0%   {{ transform: translate(0px, 0px); }}
    18%  {{ transform: translate({dx}px, {dy}px); }}
    55%  {{ transform: translate({dx}px, {dy}px); }}
    100% {{ transform: translate(0px, 0px); }}
}}
@keyframes iso-shoved-{dir} {{
    0%   {{ transform: translate({nx}px, {ny}px); }}
    100% {{ transform: translate(0px, 0px); }}
}}
.beat-staggered-{dir} {{ animation: iso-stagger-{dir} 1600ms ease-out; }}
.beat-shoved-{dir}    {{ animation: iso-shoved-{dir} 420ms ease-out; }}
",
            // The shove beat starts at the tile it came *from*, which is the
            // step negated: the board already moved it.
            nx = -dx,
            ny = -dy,
        ));
    }
    css
}

/// The compendium overlay + `data_grid` styling. The grid places its cells
/// absolutely (inline geometry), so these rules only paint colour and type.
const COMPENDIUM_CSS: &str = r#"
.compendium { position: absolute; left: 232px; top: 36px; width: 372px; background-color: #1b1e27; border: 1px solid #2c3347; border-radius: 4px; padding: 10px 10px 12px; z-index: 500; box-shadow: 0 8px 28px rgba(0,0,0,0.55); }
.generator { position: absolute; left: 232px; top: 36px; width: 372px; background-color: #1b1e27; border: 1px solid #2c3347; border-radius: 4px; padding: 10px 10px 12px; z-index: 501; box-shadow: 0 8px 28px rgba(0,0,0,0.55); }
.governance { position: absolute; left: 232px; top: 36px; width: 396px; background-color: #1b1e27; border: 1px solid #2c3347; border-radius: 4px; padding: 10px 10px 12px; z-index: 502; box-shadow: 0 8px 28px rgba(0,0,0,0.55); }
.generator-proposal { color: #e8ebf2; font-size: 14px; font-weight: bold; margin: 10px 0; }
.governance-row { padding: 8px; border-top: 1px solid #2c3347; cursor: pointer; }
.governance-row-selected { background-color: #293243; box-shadow: inset 3px 0 0 #9fd48a; }
.governance-moot { color: #e8ebf2; font-size: 13px; font-weight: bold; }
.governance-policy { color: #cfd3dd; font-size: 12px; margin-top: 2px; }
.governance-counts { color: #8a90a0; font-size: 11px; margin-top: 3px; }
.governance-restriction { color: #d6b36a; font-size: 11px; margin-top: 9px; }
.governance-actions { margin-top: 10px; }
.compendium-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; }
.compendium-title { font-size: 15px; font-weight: bold; color: #e8ebf2; }
.compendium-cell { font-size: 12px; color: #cfd3dd; white-space: nowrap; }
.grid { display: block; }
.grid-header-cell { display: flex; align-items: center; font-size: 11px; color: #8a90a0; font-weight: bold; cursor: pointer; padding-left: 6px; box-sizing: border-box; }
.grid-row-even { background-color: #1e222c; }
.grid-row-odd { background-color: #232734; }
.grid-cell { display: flex; align-items: center; padding-left: 6px; box-sizing: border-box; overflow: hidden; }
.compendium-link { color: #9fd48a; cursor: pointer; white-space: nowrap; }
.compendium-actions { display: flex; gap: 6px; }
.overlay-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; }
.overlay-title { font-size: 16px; font-weight: bold; color: #e8ebf2; }
.overlay-actions { display: flex; gap: 6px; }
.entry-sub { font-size: 12px; color: #8a90a0; font-style: italic; margin-bottom: 10px; }
.monster-stats { display: flex; flex-wrap: wrap; gap: 4px 18px; margin-bottom: 12px; }
.stat-row { font-size: 13px; }
.stat-label { color: #8a90a0; margin-right: 5px; }
.stat-val { color: #e8ebf2; font-weight: bold; }
.monster-abilities { display: flex; gap: 6px; margin-bottom: 12px; }
.abil { flex: 1; background-color: #232734; border-radius: 3px; padding: 5px 0; text-align: center; }
.abil-name { font-size: 10px; color: #8a90a0; }
.abil-score { font-size: 15px; color: #e8ebf2; font-weight: bold; }
.abil-mod { font-size: 11px; color: #9fd48a; }
.monster-actions { display: block; margin-bottom: 12px; }
.monster-action { margin-bottom: 7px; }
.action-name { font-size: 13px; color: #e8ebf2; font-weight: bold; }
.action-desc { font-size: 11px; color: #cfd3dd; }
.spawn-btn { display: inline-block; background-color: #2c6e49; color: #eaffea; padding: 7px 14px; border-radius: 4px; cursor: pointer; font-weight: bold; font-size: 13px; }
.tab-strip { display: flex; gap: 4px; margin-bottom: 8px; }
.tab { font-size: 12px; color: #8a90a0; background-color: #232734; padding: 4px 11px; border-radius: 3px; cursor: pointer; }
.tab-active { color: #eef1f7; background-color: #31527a; }
.entry-name { font-size: 16px; font-weight: bold; color: #e8ebf2; margin-bottom: 2px; }
.compendium-desc { font-size: 12px; color: #cfd3dd; line-height: 1.45; }
.search-field { display: flex; align-items: center; justify-content: space-between; background-color: #232734; border: 1px solid #2c3347; border-radius: 3px; padding: 5px 9px; margin-bottom: 8px; font-size: 12px; }
.search-hint { color: #6a7080; font-style: italic; }
.search-text { color: #e8ebf2; }
.search-clear { color: #8a90a0; cursor: pointer; }
"#;

/// Bake the demo voxel rig to `.token-*` sprite rules (data-URI PNGs), called
/// once from [`board_css`]. `background-size: contain` plus a bottom anchor
/// stands the sprite in the 24x36 token box with its feet at the tile.
fn voxel_token_css() -> String {
    use isometry_voxel::{BakeParams, Palette, bake_facing, demo};
    let p = BakeParams { half_w: 2, cube_h: 2, facings: 4, margin: 2 };
    let (rig, base) = demo::hero();
    // Palette-swap the one rig into per-monster recolours (skin index 0, shirt
    // index 1). Proves recolour across the starter bestiary; per-monster voxel
    // models arrive with parts packs (P3).
    let recolor = |skin: [u8; 3], shirt: [u8; 3]| -> Palette {
        Palette::new(
            base.0
                .iter()
                .enumerate()
                .map(|(i, c)| match i {
                    0 => skin,
                    1 => shirt,
                    _ => *c,
                })
                .collect(),
        )
    };
    let variants: [(&str, Palette); 5] = [
        ("knight", base.clone()),
        ("goblin", recolor([140, 165, 110], [72, 110, 60])),
        ("orc", recolor([120, 140, 95], [92, 70, 55])),
        ("skeleton", recolor([226, 223, 211], [198, 194, 180])),
        ("wolf", recolor([150, 150, 158], [92, 92, 100])),
    ];
    let mut css = String::new();
    for (class, pal) in &variants {
        let uri = bake_facing(&rig, pal, 0, &p).to_png_data_uri();
        css.push_str(&format!(
            ".token-{class} {{ background-image: url(\"{uri}\"); \
             background-size: contain; background-position: bottom center; }}\n"
        ));
    }
    css
}
