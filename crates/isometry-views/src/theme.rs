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

/* Mirror about the sprite's own center. serval conjugates transforms
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

/* Tokens: 8x12 pixel sprites at 3x, nearest-neighbor (probe P1). */
.token {
    position: absolute;
    width: 24px;
    height: 36px;
    background-repeat: no-repeat;
    background-size: 100% 100%;
    image-rendering: pixelated;
}
"#
    .to_owned();
    // Voxel-baked pixel tileset: the pixel sprites this sheet was waiting for
    // (design_docs/2026-07-08_campaign_packs_plan.md). Knight is the demo rig;
    // goblin is the same rig recoloured, proving palette-swap on the board.
    css.push_str(&voxel_token_css());
    css.push_str(COMPENDIUM_CSS);
    css
}

/// The compendium overlay + `data_grid` styling. The grid places its cells
/// absolutely (inline geometry), so these rules only paint colour and type.
const COMPENDIUM_CSS: &str = r#"
.compendium { position: absolute; left: 232px; top: 36px; width: 372px; background-color: #1b1e27; border: 1px solid #2c3347; border-radius: 4px; padding: 10px 10px 12px; z-index: 500; box-shadow: 0 8px 28px rgba(0,0,0,0.55); }
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
.monster-sub { font-size: 12px; color: #8a90a0; font-style: italic; margin-bottom: 10px; }
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
