//! The placeholder tileset, expressed the way real ones will be: a
//! stylesheet over the tile-kind class vocabulary. Colored clip-path
//! diamonds stand in for sprites until a pixel tileset lands (and the
//! `image-rendering: pixelated` engine seam opens, probe P1).

/// The whole app sheet: chrome plus the placeholder tileset.
pub fn board_css() -> String {
    r#"
.app {
    position: absolute;
    left: 0; top: 0; right: 0; bottom: 0;
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

.token {
    position: absolute;
    width: 14px;
    height: 20px;
}
.token-knight { background-color: #e3e6ef; }
.token-goblin { background-color: #8f4bb8; }
"#
    .to_owned()
}
