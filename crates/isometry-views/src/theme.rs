//! The placeholder tileset, expressed the way real ones will be: a
//! stylesheet over the tile-kind class vocabulary. Colored clip-path
//! diamonds stand in for sprites until a pixel tileset lands (and the
//! `image-rendering: pixelated` engine seam opens, probe P1).

/// The whole app sheet: chrome plus the placeholder tileset.
pub fn board_css() -> String {
    r#"
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
    padding: 14px;
    background-color: #1b1e27;
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
.token-knight { background-image: url("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAMCAYAAABfnvydAAAAAXNSR0IArs4c6QAAAARnQU1BAACxjwv8YQUAAAAJcEhZcwAADsMAAA7DAcdvqGQAAABOSURBVChTY2BAAgpKOv9BGFkMDkASj5+9B2OsivAqAAn8v54GVwBioyhC1o1hCoiRV9ICFkCWhIkRVoDNGgxHElSAbCRO4wkqQBZA5gMAHtiM9x/Mi9QAAAAASUVORK5CYII="); }
.token-goblin { background-image: url("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAMCAYAAABfnvydAAAAAXNSR0IArs4c6QAAAARnQU1BAACxjwv8YQUAAAAJcEhZcwAADsMAAA7DAcdvqGQAAABASURBVChTY2AgBigo6fzHKwbiIAug88EC/d47wILIbBRJbBisCKbg//U0uASMjaIAG8ZrDfGOJFoBsgA6Hy8AAJSac/fEyk0mAAAAAElFTkSuQmCC"); }
"#
    .to_owned()
}
