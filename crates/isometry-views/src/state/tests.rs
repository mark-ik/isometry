//! Tests for the runner state.
//!
//! They exercise `UiState` as a whole rather than any one submodule, so they
//! live beside the split rather than inside a single piece of it.
//!
//! Split out of `state.rs` on 2026-07-24; unchanged.

use super::*;
use crate::demo::demo_map;

#[test]
fn paint_undo_redo_round_trip() {
    let mut ui = UiState::new(demo_map());
    let pristine = ui.map.clone();
    ui.mode = EditMode::PaintGround;
    ui.brush = TileKindId(2);
    ui.click_tile((3, 3));
    ui.click_tile((4, 3));
    let painted = ui.map.clone();
    assert_ne!(painted, pristine);
    ui.undo();
    ui.undo();
    assert_eq!(ui.map, pristine);
    ui.redo();
    ui.redo();
    assert_eq!(ui.map, painted);
}

#[test]
fn play_move_respects_turn_gate_and_sets_facing() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    ui.mode = EditMode::Play;
    // Knight at (10, 14): free token, may move.
    ui.select_token(TokenId(1));
    assert!(ui.may_move(TokenId(1)));
    assert!(!ui.reach.is_empty());
    ui.click_tile((12, 14)); // 2 east, within budget 5
    let t = ui.map.token(TokenId(1)).unwrap();
    assert_eq!(t.at, (12, 14));
    assert_eq!(t.facing, isometry_core::Facing::East);
    // Both tokens in the list: only the active one may move.
    ui.toggle_turn(TokenId(1));
    ui.toggle_turn(TokenId(2));
    assert!(ui.may_move(TokenId(1)));
    assert!(!ui.may_move(TokenId(2)));
    ui.select_token(TokenId(2));
    assert!(ui.reach.is_empty(), "waiting token gets no reach");
    let before = ui.map.token(TokenId(2)).unwrap().at;
    ui.click_tile((before.0 + 1, before.1));
    assert_eq!(ui.map.token(TokenId(2)).unwrap().at, before);
    // End turn: token 2 is up and can move now.
    ui.end_turn();
    assert!(ui.may_move(TokenId(2)));
    // The move is undoable (one step: move + facing).
    ui.select_token(TokenId(1));
    assert!(!ui.may_move(TokenId(1)) || ui.turns.active() == Some(TokenId(1)));
}

#[test]
fn ending_a_turn_refreshes_the_incoming_token_per_turn_counters() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    ui.toggle_turn(TokenId(1));
    ui.toggle_turn(TokenId(2));
    assert_eq!(ui.turns.active(), Some(TokenId(1)));
    // The active knight has spent part of its turn: an action economy, a
    // multiple-attack tally -- the view never learns which.
    ui.map.bump_turn_counter(TokenId(1), "actions_spent", 2);

    // The goblin's turn begins. A turn-start wipes the *incoming* token's
    // counters, so the goblin's clear (they were empty) while the knight
    // keeps its spend as it waits.
    ui.end_turn();
    assert_eq!(ui.turns.active(), Some(TokenId(2)));
    assert_eq!(ui.map.turn_counter(TokenId(1), "actions_spent"), 2);

    // Back to the knight: its own turn beginning clears the ledger, so it
    // acts with a whole economy again.
    ui.end_turn();
    assert_eq!(ui.turns.active(), Some(TokenId(1)));
    assert_eq!(ui.map.turn_counter(TokenId(1), "actions_spent"), 0);
}

#[test]
fn drag_move_relocates_a_token_and_is_undoable() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    let start = ui.map.token(TokenId(1)).unwrap().at; // (10, 14)
    ui.drag_move_token(TokenId(1), (5, 5));
    assert_eq!(ui.map.token(TokenId(1)).unwrap().at, (5, 5));
    // Occupied (goblin 2 at (15, 8)) and out-of-bounds are no-ops.
    ui.drag_move_token(TokenId(1), (15, 8));
    ui.drag_move_token(TokenId(1), (999, 999));
    assert_eq!(ui.map.token(TokenId(1)).unwrap().at, (5, 5));
    // The one real move undoes back to the start.
    ui.undo();
    assert_eq!(ui.map.token(TokenId(1)).unwrap().at, start);
}

#[test]
fn drag_move_routes_out_in_remote_mode() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    ui.net_mode = NetMode::Remote;
    let before = ui.map.token(TokenId(1)).unwrap().at;
    ui.drag_move_token(TokenId(1), (5, 5));
    // Session mode emits an intent and leaves the local map untouched.
    assert_eq!(ui.map.token(TokenId(1)).unwrap().at, before);
    assert_eq!(ui.net_outbox.len(), 1);
}

#[test]
fn token_drag_candidate_finds_a_token_in_select_mode_only() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    // Cursor over knight 1's tile (10, 14); default camera is (0, 0).
    let (sx, sy) = ui.geo.tile_to_screen((10, 14), 0);
    let on_token = (sx + PANEL_W + ui.camera.0, sy + ui.camera.1);
    assert_eq!(ui.mode, EditMode::Select);
    assert_eq!(ui.token_drag_candidate(on_token), Some(TokenId(1)));
    // An empty tile, or any non-Select mode, yields nothing.
    let (ex, ey) = ui.geo.tile_to_screen((0, 0), 0);
    assert_eq!(ui.token_drag_candidate((ex + PANEL_W, ey)), None);
    ui.mode = EditMode::Play;
    assert_eq!(ui.token_drag_candidate(on_token), None);
}

#[test]
fn context_menu_opens_selects_and_removes() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    let n = ui.map.tokens.len();
    ui.open_context_menu(TokenId(1), (50.0, 60.0));
    assert_eq!(ui.context_menu, Some((TokenId(1), (50.0, 60.0))));
    assert_eq!(ui.selected_token, Some(TokenId(1)), "right-click selects");
    ui.close_context_menu();
    assert!(ui.context_menu.is_none());
    // Remove drops the token from the map, turn order, and selection.
    ui.turns.add(TokenId(1));
    ui.open_context_menu(TokenId(1), (0.0, 0.0));
    ui.remove_token(TokenId(1));
    assert_eq!(ui.map.tokens.len(), n - 1);
    assert!(ui.map.token(TokenId(1)).is_none());
    assert!(!ui.turns.contains(TokenId(1)));
    assert!(ui.selected_token.is_none());
    assert!(ui.context_menu.is_none());
}

#[test]
fn remote_mode_routes_moves_as_events_not_local_mutation() {
    use isometry_core::TokenId;
    let mut ui = UiState::new(demo_map());
    ui.net_mode = NetMode::Remote;
    ui.mode = EditMode::Play;
    let before = ui.map.token(TokenId(1)).unwrap().at;
    ui.select_token(TokenId(1));
    // A move in a session emits intents and leaves the local map
    // untouched (the host authority echoes the real move back).
    ui.click_tile((before.0 + 1, before.1));
    assert_eq!(ui.map.token(TokenId(1)).unwrap().at, before);
    assert_eq!(ui.net_outbox.len(), 2, "move + facing emitted");
    // End turn and toggle also route out, not local.
    ui.net_outbox.clear();
    ui.end_turn();
    ui.toggle_turn(TokenId(2));
    assert_eq!(ui.net_outbox.len(), 2);
    // Editing is inert in a session.
    ui.net_outbox.clear();
    ui.mode = EditMode::PaintGround;
    ui.click_tile((0, 0));
    assert!(ui.net_outbox.is_empty());
    assert!(ui.can_undo() == false, "no local edit happened");

    // A snapshot mirrors the authoritative state in.
    let mut snap_map = ui.map.clone();
    snap_map.token_mut(TokenId(1)).unwrap().at = (before.0 + 1, before.1);
    let inventories = std::collections::BTreeMap::from([(TokenId(1), Inventory::default())]);
    let snap = GameSnapshot {
        map: snap_map,
        turns: ui.turns.clone(),
        roll_log: Vec::new(),
        journal: Vec::new(),
        inventories: inventories.clone(),
        generations: Vec::new(),
        maps: Default::default(),
        active_map: None,
        world: Default::default(),
        clocks: Default::default(),

        party_cap: isometry_net::default_party_cap(),
        last_beats: Vec::new(),
        beat_seq: 0,
    };
    ui.apply_snapshot(snap);
    assert_eq!(
        ui.map.token(TokenId(1)).unwrap().at,
        (before.0 + 1, before.1)
    );
    assert_eq!(ui.inventories, inventories);
}

#[test]
fn compendium_item_request_targets_the_open_sheet() {
    let mut ui = UiState::new(demo_map());
    ui.open_sheet = Some(TokenId(1));
    let item = ItemRow {
        key: "longsword".to_owned(),
        name: "Longsword".to_owned(),
        category: "Weapon".to_owned(),
        cost: "15 gp".to_owned(),
        weight: "3 lb.".to_owned(),
        detail: "1d8 slashing".to_owned(),
        desc: String::new(),
    };
    ui.request_compendium_item(&item);
    assert_eq!(
        ui.inventory_request,
        Some(InventoryRequest::AddCompendiumItem {
            token: TokenId(1),
            template: "longsword".to_owned(),
            name: "Longsword".to_owned(),
            category: "Weapon".to_owned(),
        })
    );
}

#[test]
fn generator_controls_keep_locks_visible_and_queue_host_work() {
    let mut ui = UiState::new(demo_map());
    ui.generator_choices.push(GeneratorChoice {
        id: "demo:forge_item".to_owned(),
        name: "Forge item".to_owned(),
        default_args: GenValue::Text {
            value: "river".to_owned(),
        },
        lock_presets: vec![isometry_campaign::GeneratorLockPreset {
            key: "culture".to_owned(),
            label: "River-clan culture".to_owned(),
            value: GenValue::Text {
                value: "river-clans".to_owned(),
            },
        }],
    });
    ui.open_generator();
    ui.toggle_generator_lock();
    assert_eq!(
        ui.generator_locks.get("culture"),
        Some(&GenValue::Text {
            value: "river-clans".to_owned()
        })
    );
    ui.request_generation();
    assert_eq!(ui.generation_request, Some(GenerationRequest::Generate));

    ui.toggle_generator_lock();
    assert!(!ui.generator_locks.contains_key("culture"));
}

#[test]
fn governance_conflict_queues_typed_adopt_and_branch_requests() {
    let mut ui = UiState::new(demo_map());
    ui.governance_conflict = Some(GovernanceConflict {
        candidates: vec![
            GovernanceBindingRow {
                proposal: [1; 32],
                moot: "North table".to_owned(),
                policy: "unanimous".to_owned(),
                endorsements: 2,
                required: 2,
                claims: 1,
            },
            GovernanceBindingRow {
                proposal: [2; 32],
                moot: "North table".to_owned(),
                policy: "threshold 2".to_owned(),
                endorsements: 2,
                required: 2,
                claims: 1,
            },
        ],
        can_adopt: true,
        can_branch: true,
        restriction: None,
    });

    ui.open_governance_conflict();
    ui.select_governance_candidate(1);
    ui.request_governance_adopt();
    assert_eq!(
        ui.governance_resolution_request,
        Some(GovernanceResolutionRequest::Adopt { selected: [2; 32] })
    );
    assert!(!ui.governance_conflict_open);

    ui.open_governance_conflict();
    ui.request_governance_branch();
    assert_eq!(
        ui.governance_resolution_request,
        Some(GovernanceResolutionRequest::Branch {
            candidates: vec![[1; 32], [2; 32]],
        })
    );
}

#[test]
fn governance_conflict_respects_host_restrictions() {
    let mut ui = UiState::new(demo_map());
    ui.governance_conflict = Some(GovernanceConflict {
        candidates: vec![
            GovernanceBindingRow {
                proposal: [1; 32],
                moot: "First table".to_owned(),
                policy: "unanimous".to_owned(),
                endorsements: 1,
                required: 1,
                claims: 1,
            },
            GovernanceBindingRow {
                proposal: [2; 32],
                moot: "Other table".to_owned(),
                policy: "unanimous".to_owned(),
                endorsements: 1,
                required: 1,
                claims: 1,
            },
        ],
        can_adopt: false,
        can_branch: false,
        restriction: Some("no shared founding electorate".to_owned()),
    });
    ui.open_governance_conflict();
    ui.request_governance_adopt();
    assert!(ui.governance_resolution_request.is_none());
    assert_eq!(ui.status, "no shared founding electorate");
}

#[test]
fn transfer_request_keeps_source_and_target_explicit() {
    let mut ui = UiState::new(demo_map());
    ui.open_sheet = Some(TokenId(1));
    ui.request_transfer(TokenId(2), ItemId::new("token-1.item-0"));
    assert_eq!(
        ui.inventory_request,
        Some(InventoryRequest::Transfer {
            from: TokenId(1),
            to: TokenId(2),
            item: ItemId::new("token-1.item-0"),
        })
    );
}

#[test]
fn local_roll_appends_to_log_and_is_reproducible() {
    let mut ui = UiState::new(demo_map());
    ui.reseed(99);
    ui.roll_dice("1d20+3");
    assert_eq!(ui.roll_log.len(), 1);
    let rec = &ui.roll_log[0];
    assert_eq!(rec.by, "dm");
    assert_eq!(rec.dice.len(), 1);
    assert_eq!(rec.total, rec.dice[0] as i32 + 3);
    // A bad expression sets a status and adds nothing.
    ui.roll_dice("nonsense");
    assert_eq!(ui.roll_log.len(), 1);
    assert!(ui.status.starts_with("bad roll"));
}

#[test]
fn roll_initiative_individual_and_side() {
    let mut ui = UiState::new(demo_map());
    ui.reseed(5);
    let ids: Vec<_> = ui.map.tokens.iter().map(|t| t.id).collect();
    for id in &ids {
        ui.turns.add(*id);
    }
    // Individual: one roll per token, order preserved as a set.
    ui.roll_initiative();
    assert_eq!(ui.turns.entries().len(), ids.len());
    assert_eq!(ui.roll_log.len(), ids.len());
    let mut sorted = ui.turns.entries().to_vec();
    sorted.sort();
    let mut expect = ids.clone();
    expect.sort();
    assert_eq!(sorted, expect, "same tokens, reordered");

    // Side-based: tokens grouped by owner, so exactly one boundary
    // between the two sides (A knights, B goblins).
    ui.initiative_mode = InitiativeMode::SideBased;
    ui.roll_initiative();
    let owners: Vec<String> = ui
        .turns
        .entries()
        .iter()
        .map(|id| ui.map.token(*id).unwrap().owner.clone().unwrap())
        .collect();
    let boundaries = owners.windows(2).filter(|w| w[0] != w[1]).count();
    assert_eq!(boundaries, 1, "two sides act in blocks");
}

#[test]
fn compose_whisper_builds_draft_and_logs() {
    let mut ui = UiState::new(demo_map());
    ui.whisper_target = Some("alice".to_owned());
    ui.start_compose();
    assert!(ui.composing);
    for c in "hi".chars() {
        ui.compose_char(c);
    }
    ui.compose_backspace();
    ui.compose_char('e');
    ui.compose_char('y');
    assert_eq!(ui.whisper_draft, "hey");
    ui.compose_send();
    assert!(!ui.composing);
    assert_eq!(ui.messages, vec!["to alice: hey".to_string()]);
    assert_eq!(
        ui.whisper_outbox,
        vec![("alice".to_string(), "hey".to_string())]
    );
    ui.receive_whisper("dm", "watch out");
    assert_eq!(ui.messages[1], "from dm: watch out");
}

/// The pane shows the last five, so unbounded growth was invisible from the
/// screen: only the cap keeps a long session's log from growing forever.
/// Oldest goes first, so the newest (the ones actually rendered) survive.
#[test]
fn message_log_is_capped_and_drops_oldest() {
    let mut ui = UiState::new(demo_map());
    for i in 0..(MESSAGES_CAP + 20) {
        ui.receive_whisper("dm", &format!("line {i}"));
    }
    assert_eq!(ui.messages.len(), MESSAGES_CAP);
    assert_eq!(
        ui.messages.last().unwrap(),
        &format!("from dm: line {}", MESSAGES_CAP + 19),
        "the newest whisper must survive; the pane renders from the tail"
    );
    assert_eq!(ui.messages.first().unwrap(), "from dm: line 20");
}

#[test]
fn remote_roll_routes_out_not_local() {
    let mut ui = UiState::new(demo_map());
    ui.net_mode = NetMode::Remote;
    ui.viewer = Some("A".to_owned());
    ui.roll_dice("2d6");
    assert!(
        ui.roll_log.is_empty(),
        "remote rolls come back via snapshot"
    );
    assert_eq!(ui.net_outbox.len(), 1);
}

#[test]
fn fog_hides_out_of_sight_and_remembers_explored() {
    let mut ui = UiState::new(demo_map());
    // Knights are owner "A" near (10,14)/(9,15); goblins "B" near the
    // northeast hill. As player A, the goblins are out of sight.
    ui.viewer = Some("A".to_owned());
    ui.recompute_fog();
    let knight = ui.map.token(TokenId(1)).unwrap().at;
    let goblin = ui.map.token(TokenId(2)).unwrap().at;
    assert_eq!(ui.fog_level(knight), FogLevel::Clear);
    assert_eq!(ui.fog_level(goblin), FogLevel::Hidden);
    assert!(ui.token_visible(ui.map.token(TokenId(1)).unwrap()));
    assert!(!ui.token_visible(ui.map.token(TokenId(2)).unwrap()));

    // Explored memory: a tile seen once stays remembered (Dim) after
    // the token that saw it moves away.
    let seen_far = ui
        .visible
        .iter()
        .copied()
        .find(|&t| t != knight && (t.0 - knight.0).abs() + (t.1 - knight.1).abs() >= 3)
        .expect("some far-but-visible tile");
    // Move the knight to the opposite side so seen_far leaves sight.
    ui.mode = EditMode::Play;
    ui.apply_step(vec![SessionEvent::TokenMoved {
        id: TokenId(1),
        to: (0, 0),
    }]);
    ui.apply_step(vec![SessionEvent::TokenMoved {
        id: TokenId(3),
        to: (1, 0),
    }]);
    assert_eq!(
        ui.fog_level(seen_far),
        FogLevel::Dim,
        "a tile seen earlier is remembered, not black"
    );

    // Omniscient clears fog entirely.
    ui.viewer = None;
    ui.recompute_fog();
    assert_eq!(ui.fog_level(goblin), FogLevel::Clear);
    assert!(!ui.fog_active());
}

#[test]
fn token_mode_places_and_removes() {
    let mut ui = UiState::new(demo_map());
    ui.mode = EditMode::Token;
    let n = ui.map.tokens.len();
    ui.click_tile((2, 2));
    assert_eq!(ui.map.tokens.len(), n + 1);
    let placed = ui.token_at((2, 2)).unwrap();
    assert!(placed.0 > 2, "fresh id past the demo tokens");
    ui.click_tile((2, 2));
    assert_eq!(ui.map.tokens.len(), n);
    ui.undo(); // undo the removal: token back
    assert_eq!(ui.map.tokens.len(), n + 1);
    assert_eq!(ui.token_at((2, 2)), Some(placed));
}

#[test]
fn fill_is_one_undo_step() {
    let mut ui = UiState::new(demo_map());
    let pristine = ui.map.clone();
    ui.mode = EditMode::Fill;
    ui.brush = TileKindId(3);
    ui.click_tile((0, 0));
    assert_ne!(ui.map, pristine);
    ui.undo();
    assert_eq!(ui.map, pristine);
}

#[test]
fn spawn_in_a_session_routes_through_the_authority_not_the_local_map() {
    // The bug the adversarial review caught: a hosted DM's `>spawn` mutated
    // the local map directly, so the token never replicated and was wiped by
    // the next snapshot mirror (leaving an orphan sheet). It must emit an
    // authoritative TokenPlaced instead.
    let mut ui = UiState::new(demo_map());
    ui.net_mode = NetMode::Remote;
    ui.bestiary = vec![MonsterRow {
        key: "goblin".to_owned(),
        name: "Goblin".to_owned(),
        cr: 0.25,
        cr_label: "1/4".to_owned(),
        kind: "humanoid".to_owned(),
        size: "small".to_owned(),
        alignment: "ne".to_owned(),
        hp: 7,
        hit_dice: "2d6".to_owned(),
        ac: 15,
        speed_ft: 30,
        xp: 50,
        abilities: [8, 14, 10, 10, 8, 8],
        actions: Vec::new(),
        sprite: "goblin".to_owned(),
    }];
    let before = ui.map.tokens.len();

    ui.spawn_query("goblin");

    // The local map is untouched; the placement is queued for the authority.
    assert_eq!(
        ui.map.tokens.len(),
        before,
        "no local mutation in a session"
    );
    let placed = ui
        .net_outbox
        .iter()
        .any(|e| matches!(e, GameEvent::Map(SessionEvent::TokenPlaced(_))));
    assert!(placed, "the spawn must replicate as an authoritative event");
    // And the stat-block bind is queued for the same id.
    assert!(ui.spawn_sheet_request.is_some());
}

#[test]
fn the_storylet_surface_is_dm_only_and_plays_only_the_ready() {
    let mut ui = UiState::new(demo_map());
    // A joined player cannot open or play storylets (matching reads secrets).
    ui.can_edit_inventory = false;
    ui.open_storylets();
    assert!(
        !ui.storylet_open,
        "a client must not open the storylet surface"
    );

    // The DM can. A locked storylet cannot be played; a ready one arms a
    // request the host will commit.
    ui.can_edit_inventory = true;
    ui.storylets = vec![
        StoryletRow {
            key: "locked".to_owned(),
            entry: "The cult stirs.".to_owned(),
            available: false,
            status: "needs a faction tagged 'cult'".to_owned(),
            cast: Vec::new(),
        },
        StoryletRow {
            key: "ready".to_owned(),
            entry: "A stranger greets you.".to_owned(),
            available: true,
            status: "ready".to_owned(),
            cast: Vec::new(),
        },
    ];
    ui.open_storylets();
    assert!(ui.storylet_open);

    ui.storylet_selected = 0; // the locked one
    ui.play_storylet();
    assert_eq!(
        ui.storylet_request, None,
        "a locked storylet cannot be played"
    );

    ui.storylet_selected = 1; // the ready one
    ui.play_storylet();
    assert_eq!(ui.storylet_request.as_deref(), Some("ready"));
}

#[test]
fn the_downtime_surface_is_dm_only_and_commits_only_the_kept() {
    let mut ui = UiState::new(demo_map());
    // A joined player cannot open downtime (the roll reads the world and
    // spends host entropy), so nothing is armed.
    ui.can_edit_inventory = false;
    ui.open_downtime();
    assert!(
        !ui.downtime_open,
        "a client must not open the downtime surface"
    );
    assert!(!ui.downtime_roll_request);

    // The DM can: opening arms a roll request the host fills with rows.
    ui.can_edit_inventory = true;
    ui.open_downtime();
    assert!(ui.downtime_open && ui.downtime_roll_request);
    ui.downtime_roll_request = false; // the host consumed it and filled rows
    ui.faction_moves = vec![
        FactionMoveRow {
            faction: "tide".to_owned(),
            verb: "court".to_owned(),
            text: "Bran swore to the Tide Court.".to_owned(),
            has_change: true,
            struck: false,
        },
        FactionMoveRow {
            faction: "ash".to_owned(),
            verb: "raid".to_owned(),
            text: "The Ash Company raided a rival.".to_owned(),
            has_change: false,
            struck: false,
        },
    ];

    // Strike the raid; it will not commit.
    ui.downtime_selected = 1;
    ui.toggle_strike_downtime();
    assert!(ui.faction_moves[1].struck);
    ui.commit_downtime();
    assert!(ui.downtime_commit_request, "one kept move arms the commit");

    // Strike everything and commit refuses: an empty tick is no tick.
    ui.downtime_commit_request = false;
    ui.faction_moves.iter_mut().for_each(|m| m.struck = true);
    ui.commit_downtime();
    assert!(
        !ui.downtime_commit_request,
        "nothing kept, nothing to commit"
    );
}

#[test]
fn the_overmap_surface_opens_and_arms_a_travel_request() {
    let mut ui = UiState::new(demo_map());
    assert!(!ui.overmap_open);
    // Anyone may look at the overmap (unlike the DM-only downtime surface).
    ui.open_overmap();
    assert!(ui.overmap_open);
    // Clicking a place arms a one-shot the host resolves; the view decides
    // nothing about the trip.
    ui.request_travel("forest".to_owned());
    assert_eq!(ui.overmap_travel_request.as_deref(), Some("forest"));
    // Choosing a pace and a stance arm their own one-shots for the host.
    ui.request_pace(50);
    assert_eq!(ui.overmap_pace_request, Some(50));
    ui.request_stance("scout");
    assert_eq!(ui.overmap_stance_request.as_deref(), Some("scout"));
    ui.request_map_read();
    assert!(ui.overmap_read_request, "studying the map arms a read");
    ui.close_overmap();
    assert!(!ui.overmap_open);
}

#[test]
fn a_viewer_commands_a_faction_only_once_granted_its_channel() {
    let mut ui = UiState::new(demo_map());
    ui.viewer = Some("B".to_owned());

    // Ungranted, a faction's token is not B's to command.
    assert!(!ui.commands(Some("tide")));
    // Grant B the Tide Court's channel (as the replicated world would carry).
    ui.world
        .faction_control
        .insert("tide".to_owned(), "B".to_owned());
    assert!(
        ui.commands(Some("tide")),
        "the grant extends command to the faction"
    );

    // Direct ownership is unchanged, and a stranger's token stays off-limits.
    assert!(ui.commands(Some("B")));
    assert!(!ui.commands(Some("A")));
    assert!(!ui.commands(Some("ash")), "an unrelated faction is not B's");
    assert!(!ui.commands(None), "a DM token is nobody's to a player");
}

#[test]
fn apply_snapshot_mirrors_the_clock_and_the_cap() {
    // A joined client mirrors the host snapshot into its UiState. Dropping
    // clocks (a C3 omission the C5 review caught) shows the wrong split-party
    // time on clients; dropping party_cap desyncs the limit.
    let mut ui = UiState::new(demo_map());
    assert_eq!(ui.party_cap, 4);
    let mut snap = GameSnapshot {
        map: demo_map(),
        turns: TurnList::new(),
        roll_log: Vec::new(),
        journal: Vec::new(),
        inventories: Default::default(),
        generations: Vec::new(),
        maps: Default::default(),
        active_map: None,
        world: Default::default(),
        clocks: Default::default(),
        party_cap: 2,
        last_beats: Vec::new(),
        beat_seq: 0,
    };
    snap.clocks.insert("field".to_owned(), 7);
    ui.apply_snapshot(snap);
    assert_eq!(ui.party_cap, 2, "the cap must mirror");
    assert_eq!(ui.clocks.get("field"), Some(&7), "the clock must mirror");
}

#[test]
fn a_spawn_tile_stays_on_the_board_on_a_narrow_map() {
    // free_spawn_tile's outward scan could walk off a map narrower than its
    // stride, yielding an off-board tile that fails placement. It must clamp.
    let mut ui = UiState::new(MapDocument::new("slot", 3, 3));
    // Pack the whole 3x3 but one cell, forcing the scan to the survivor.
    for row in 0..3 {
        for col in 0..3 {
            if (col, row) != (2, 2) {
                ui.map.tokens.push(Token {
                    id: TokenId(100 + (row * 3 + col) as u32),
                    at: (col, row),
                    facing: Facing::South,
                    sprite: "goblin".to_owned(),
                    owner: None,
                });
            }
        }
    }
    let at = ui.free_spawn_tile();
    assert!(
        ui.map.ground.in_bounds(at.0, at.1),
        "spawn tile {at:?} is off the 3x3 board"
    );
    assert_eq!(at, (2, 2), "the one free in-bounds cell");
}
