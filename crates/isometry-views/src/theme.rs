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
.tile:hover { background-color: #ffe9a0; }
.tile-selected { background-color: #ffd766; }

.tile-grass { background-color: #4f8f3b; }
.tile-grass.alt { background-color: #478536; }
.tile-water { background-color: #2f629e; }
.tile-water.alt { background-color: #2a5a93; }
.tile-stone { background-color: #8d9098; }
.tile-stone.alt { background-color: #84878f; }
.tile-under { background-color: #3b5c2d; }

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
