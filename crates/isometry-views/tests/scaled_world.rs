//! Scaled-world receipts for the 2026-07-20 perf fixes.
//!
//! Those fixes removed per-frame and per-dispatch work whose cost scales with
//! *world* size: the overmap swatch build behind the `overmap_open` guard, the
//! leaf re-registration behind the swatch-changed gate, and four `CampaignWorld`
//! clones behind their request flags. The demo campaign is five places, so the
//! before/after delta sat below the measurement noise floor and the fixes landed
//! on call-path reading alone. This file supplies the world that makes them
//! measurable, and asserts the invariants each gate depends on.
//!
//! Two tiers, deliberately:
//!
//! - The `gate_*` tests always run. They assert *soundness* -- that the
//!   swatch-changed gate neither repaints a world that did not change nor
//!   withholds paint from one that did. A gate that is unsound is worse than no
//!   gate, and timing will never tell you which you have.
//! - [`receipt`] is `#[ignore]`d and prints the scaled numbers:
//!   `cargo test -p isometry-views --release --test scaled_world -- --ignored --nocapture`
//!   Release matters: debug timings are dominated by unoptimized BTreeMap walks
//!   and misreport the ratios.
//!
//! `ISOMETRY_SYNTH_WORLD=<places>` overrides the receipt's largest size.

use std::time::{Duration, Instant};

use isometry_core::MapDocument;
use isometry_views::{overmap_swatch, synth_world, UiState, SYNTH_PARTY};

/// A state carrying a synthetic world of `places` sites and `storylets`
/// opportunities. The map stays tiny: these receipts measure world scale, and a
/// large board would fold board cost into the numbers.
fn state(places: usize, storylets: usize) -> UiState {
    let mut ui = UiState::new(MapDocument::new("receipt", 4, 4));
    ui.world = synth_world(places, storylets);
    ui
}

// --- Gate soundness (always run) ---------------------------------------------

/// The retention gate compares the freshly built swatch against the last one and
/// re-registers the paint leaf only on a difference. That is sound only if an
/// unchanged world builds an *equal* swatch. It did not always: a fresh
/// `GraphCanvas` is born dirty, and before the fix every frame inserted a new
/// one and repainted. If layout ever becomes nondeterministic (hash-ordered
/// iteration, a relaxation seeded from the clock), the gate silently stops
/// gating and the per-frame repaint returns with nothing to signal it. This is
/// the test that would notice.
#[test]
fn gate_holds_for_an_unchanged_world() {
    let ui = state(256, 0);
    let first = overmap_swatch(&ui).expect("a discovered world draws");
    let second = overmap_swatch(&ui).expect("a discovered world draws");
    assert_eq!(
        first, second,
        "an unchanged world must rebuild an equal swatch, or the retention gate \
         re-registers the leaf every frame"
    );
}

/// The mirror failure: a gate that never fires paints a stale map. Moving the
/// party changes which node is `Here` and which node reads as selected, so the
/// swatch must differ.
#[test]
fn gate_releases_when_the_party_moves() {
    let mut ui = state(256, 0);
    let before = overmap_swatch(&ui).expect("a discovered world draws");
    ui.world
        .party_node
        .insert(SYNTH_PARTY.to_owned(), "p9".to_owned());
    let after = overmap_swatch(&ui).expect("a discovered world draws");
    assert_ne!(
        before, after,
        "a moved party must change the swatch, or the overmap paints a stale position"
    );
}

/// Discovery grows the drawn graph, so a reveal must reach the paint leaf too.
#[test]
fn gate_releases_when_a_place_is_revealed() {
    let mut ui = state(256, 0);
    ui.world
        .party_known
        .get_mut(SYNTH_PARTY)
        .expect("synth world reveals the map")
        .remove("p200");
    let before = overmap_swatch(&ui).expect("a discovered world draws");
    ui.world.reveal(SYNTH_PARTY, "p200");
    let after = overmap_swatch(&ui).expect("a discovered world draws");
    assert_ne!(
        before, after,
        "a revealed place must change the swatch, or discovery never reaches the paint leaf"
    );
    assert_eq!(
        after.graph.nodes.len(),
        before.graph.nodes.len() + 1,
        "the revealed place should be the one added node"
    );
}

/// Hover is swatch state, so crossing a node also releases the gate. Worth
/// pinning: it is the one gate release that fires on pointer movement rather
/// than on a world edit, and it bounds how cheap hover can ever be.
#[test]
fn gate_releases_on_hover() {
    let mut ui = state(64, 0);
    let before = overmap_swatch(&ui).expect("a discovered world draws");
    ui.overmap_hover = Some("p7".to_owned());
    let after = overmap_swatch(&ui).expect("a discovered world draws");
    assert_ne!(before, after, "hover must reach the painted leaf");
}

/// The projection draws exactly what the party has discovered, at any scale.
/// The filter is what makes the swatch cost track *discovered* size rather than
/// world size, so the receipt below is honest only while this holds.
#[test]
fn swatch_draws_only_discovered_places() {
    let mut ui = state(128, 0);
    ui.world.party_known.insert(
        SYNTH_PARTY.to_owned(),
        ["p0", "p1", "p2"].iter().map(|s| s.to_string()).collect(),
    );
    let swatch = overmap_swatch(&ui).expect("a discovered world draws");
    assert_eq!(swatch.graph.nodes.len(), 3);
    assert!(
        swatch
            .graph
            .edges
            .iter()
            .all(|e| ["p0", "p1", "p2"].contains(&e.from.as_str())
                && ["p0", "p1", "p2"].contains(&e.to.as_str())),
        "an edge to an undiscovered place must not be drawn"
    );
}

/// An undiscovered world has no leaf to paint, and the host relies on that
/// `None` to drop the leaf rather than paint an empty canvas.
#[test]
fn swatch_is_none_before_discovery() {
    let mut ui = state(64, 0);
    ui.world.party_known.clear();
    assert!(overmap_swatch(&ui).is_none());
}

// --- The scaled receipt (ignored; run explicitly) -----------------------------

/// Median per-op time over nine batches. A single mean is not stable enough to
/// publish: the same 400-place build measured 2937 us and 1355 us on
/// consecutive runs of an earlier draft, which is the machine's scheduling
/// noise, not the code. The median of batches discards those excursions; the
/// column is still an order of magnitude, not a benchmark.
fn time(iterations: u32, mut op: impl FnMut()) -> Duration {
    const BATCHES: usize = 9;
    // One untimed pass so first-touch allocation does not land on sample one.
    op();
    let mut samples: Vec<Duration> = (0..BATCHES)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iterations {
                op();
            }
            start.elapsed() / iterations
        })
        .collect();
    samples.sort();
    samples[BATCHES / 2]
}

fn micros(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

#[test]
#[ignore = "scaled perf receipt; run with --release -- --ignored --nocapture"]
fn receipt() {
    let largest = std::env::var("ISOMETRY_SYNTH_WORLD")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(400);
    // The demo's five places anchor the low end: it is the scale every previous
    // measurement was taken at, and the reason they showed nothing.
    let sizes = [5, 50, 150, largest];

    println!();
    println!("Scaled-world receipt ({} build)", if cfg!(debug_assertions) { "debug" } else { "release" });
    println!();
    println!(
        "| places | storylets | world clone | swatch build | swatch compare | storylet resolve | world compare |"
    );
    println!("| ---: | ---: | ---: | ---: | ---: | ---: | ---: |");

    for places in sizes {
        let storylets = places / 2;
        let ui = state(places, storylets);
        let swatch = overmap_swatch(&ui).expect("a discovered world draws");
        // Deliberately an *equal* swatch. The steady state is a world that did
        // not change, and that is the compare with no early exit: every node and
        // edge walked before the gate can answer "unchanged". Comparing against a
        // differing swatch would measure a first-node mismatch and flatter the
        // gate by orders of magnitude.
        let unchanged = overmap_swatch(&ui).expect("a discovered world draws");
        assert_eq!(swatch, unchanged, "the compare below must walk the whole model");

        // Paid four times per dispatch before the request-flag gates landed.
        let clone = time(50, || {
            std::hint::black_box(ui.world.clone());
        });
        // Paid every frame before the `overmap_open` guard landed, and again
        // every frame before the swatch-changed gate stopped the re-register.
        let build = time(50, || {
            std::hint::black_box(overmap_swatch(&ui));
        });
        // What the gate costs to ask, in its worst and most common case. It only
        // pays if it stays far under `build`.
        let compare = time(500, || {
            std::hint::black_box(swatch == unchanged);
        });
        // What a storylet refresh used to pay every dispatch while the surface
        // was open: this, plus a full world clone, to almost always reproduce the
        // rows already on screen.
        let resolve = time(20, || {
            for storylet in ui.world.storylets.values() {
                std::hint::black_box(
                    ui.world.resolve_storylet(storylet, Vec::<&str>::new()).is_ok(),
                );
            }
        });
        // What it pays now: one equality check against the cached inputs, again
        // in the unchanged case that has no early exit.
        let cached = ui.world.clone();
        let world_compare = time(200, || {
            std::hint::black_box(ui.world == cached);
        });

        println!(
            "| {places} | {storylets} | {:.1} us | {:.1} us | {:.2} us | {:.1} us | {:.1} us |",
            micros(clone),
            micros(build),
            micros(compare),
            micros(resolve),
            micros(world_compare),
        );

        // The storylet gate replaces a clone plus a full resolve with a compare.
        // If that ever stops being a saving the gate is just a second copy of the
        // world for nothing.
        if places >= 50 {
            assert!(
                world_compare < clone + resolve,
                "at {places} places the storylet gate compare ({:.1} us) costs more than the \
                 clone + resolve it replaces ({:.1} us)",
                micros(world_compare),
                micros(clone + resolve),
            );
        }

        // The gate has to be cheaper than what it avoids, or it is pure overhead.
        // A loose margin: the point is the order of magnitude, not a threshold.
        if places >= 50 {
            assert!(
                compare * 4 < build,
                "at {places} places the gate compare ({:.1} us) is not meaningfully \
                 cheaper than the rebuild it avoids ({:.1} us)",
                micros(compare),
                micros(build),
            );
        }
    }

    println!();
    println!("Per-dispatch cost the request-flag gates avoid: 4x the world-clone column.");
    println!("Per-frame cost the open-guard and swatch-gate avoid: the swatch-build column.");
    println!(
        "Per-dispatch cost the storylet gate avoids while the surface is open: \
         world clone + storylet resolve, for the world-compare column."
    );
    println!();
}
