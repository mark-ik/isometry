# Optional intelligence: DM-loaded models, agents, and dynamic content

**Date:** 2026-07-07
**Status:** vision record (capture-now, not a build commitment). Horizon: post-keystone. Depends on the widened schema/Lua ABI and the generators lane. Recorded so the shape is not lost; explicitly not sequenced ahead of viewport windowing. Companion to [2026-07-07_next_horizons_landscape.md](2026-07-07_next_horizons_landscape.md).

---

## 0. Framing and the substrate invariants

This document records a design space, not a plan. Everything in it rides seams Isometry already has or already intends to build. Four substrate facts (verified against the code in the survey) decide how any model attaches, and they make most of this cheaper and safer than it looks:

1. **The determinism firewall.** Session convergence is an FNV-1a rolling hash over the postcard bytes of every `(seq, GameEvent)` (`protocol.rs:91-112`). An LLM is never byte-identical across peers, so a model can never sit *inside* replay. It sits *beside* the log: the host runs it once, and only the **result** crosses the wire as ordinary data (a `SheetSet`, a `TokenPlaced`, a text entry). This is exactly the seam `dice.rs` uses and the generators lane already plans (`dice.rs:5-9`).
2. **DM-authority is the trust boundary, already built.** The DM app is the sole authority, the only holder of file I/O and inbound net, and the only place a model or API key would live. Players never invoke inference. The planned `>gen` preview with `[insert] [reroll] [discard]` is a ready-made human-in-the-loop gate.
3. **Optional is literal.** Every feature degrades to a deterministic floor (a dialog tree, fuzzy search, weighted tables, a factual serializer) when no model is loaded. Player browsers never run a model; inference is DM-only.
4. **The log is legible but sub-semantic.** `SessionEvent` is geometry (`TokenMoved { to: (4,5) }`), not story. A board-to-text projection is the shared primitive nearly every feature here consumes.

The flagship is the **DM-in-the-loop dynamic-dialog system**. It is presented first and in depth; the opportunity catalog follows.

---

## 1. The dialog system (flagship)

### 1.1 Framing one: dialog is tools, the model is one driver

The substrate does not expose "an LLM." It exposes a small set of **dialog capabilities** as tools:

- `query_knowledge(topic)`: what does this NPC know
- `check_disposition()`: read trust/attitude toward the speaker
- `reveal_fact(id)` / `withhold_fact(id)`: controlled disclosure
- `offer_quest(id)` / `set_flag(key, val)`: world-state mutations
- `end_conversation()`: close the exchange

A **deterministic dialog tree** and an **LLM** are interchangeable drivers of that same tool surface. The tree walks authored nodes and calls the tools; the model reasons and calls the tools. The substrate depends on neither. This mirrors the isometry-core/isometry-system split (core knows geometry and turns; rules are Lua) and burn's provider seam (inference lives behind a trait, never bound as universal). It is the same discipline the recommended `DialogEngine` seam (section 3.5) embodies and the same posture Hidden Door validates in prior art: the model is an **optional accelerator over an authored substrate**, so the tree is the graceful-degradation floor rather than AI-Dungeon's "generation or nothing."

Each tool maps to an existing surface: an `isometry-system` Lua/piccolo action or an `isometry-core` read. The driver never touches state directly; it *requests* a tool, the DM host executes it authoritatively, and the effect becomes an ordinary event. This inherits every determinism and authority property for free.

### 1.2 Framing two: the conversation economy is a system-plugin value

Conversation is a **spendable resource**, not free chat. The primitive is a **response-token budget**:

```
budget = base ± CHA_mod ± trust ± reputation ± environment   (in-battle = 1)
       [ optional per-question word limit ]
```

This is a **derived stat + action cost computed by the system plugin**, exactly like an attack bonus or a spell slot. The substrate only tracks the counter and decrements it; the rules layer decides the number. This is not novel in tabletop (a strength: the design space is charted). It maps cleanly onto:

| Tabletop precedent | What it contributes |
|---|---|
| 5e reaction rolls / attitude tracks | one costed check, DC set by disposition, cannot retry the same argument |
| 4e skill challenge | bounded N successes before 3 failures; tension via a hard cap |
| PbtA parley / seduce-or-manipulate | each social move is a discrete costed action; a miss hands the GM a hard move |
| Burning Wheel Duel of Wits | a stat-seeded spendable pool; even the winner pays a compromise |

**This framing is what ties the flagship to the schema/ABI-widening lane.** The current Lua boundary is int-only (`call_int`). Trust, reputation, and environment are precisely the context the int-only ABI cannot see or return. The conversation economy is therefore a *forcing function* for the widened ABI: it needs the plugin to read richer context and compute a derived budget. Until the ABI widens, the economy runs on the thin inputs available (CHA mod, an in-battle flag) and grows as the ABI does.

**Two prior-art cautions to carry into the design (not resolve here):**
- *Stat-gating lockout* (the 4e critique): if CHA raises both *how many* questions and *how well each lands*, low-CHA builds may feel excluded from the whole social pillar. Consider decoupling volume from effect, or giving low-CHA players a different lever (evidence, leverage, environmental pressure).
- *Hard word-limits read as a glitch*: truncating an NPC mid-thought looks like a bug, not characterization. Duel of Wits' *compromise* (you got a costly partial) is a cleaner representation of "you didn't fully get what you wanted" than a word-count cut.

### 1.3 Framing three: DM-in-the-loop, thinking as a latency mask

The NPC's reasoning renders **privately to the DM** before the reply commits. The DM can interrupt and inject guidance; to the player, the pause is indistinguishable from a model "thinking." This rides a shipped, accepted UX pattern (Grok/o3/DeepSeek/Claude all stream a "thinking" beat to manage perceived wait; the "elevator-mirror effect"). Every GameEvent already flows through host authority; the dialog reply is just another event passing through the same gate. This is the Wizard-of-Oz paradigm applied to game mastering, and the hybrid AI-DM literature gives it empirical support: an LLM-proposes / DM-selects loop measured **41.8% fewer hallucinations** than autonomous NPCs.

**State plainly what the DM-gate does and does not cover** (per the prior-art findings):

**Mitigates well:**
- *Secret-leak intent* the DM can see forming in the reasoning and redirect ("deflect").
- *Hallucinated facts* the DM corrects against world/entity state (the 41.8% result).
- *Character drift within an answer* re-anchored before it ships.
- *Obvious jailbreak compliance* (the grandma/role-play frame) spotted and refused.

**Does NOT reliably stop:**
- *Unfaithful chain-of-thought* (the sharpest risk). Models "don't always say what they think"; a leak can appear in the final answer that the visible reasoning never telegraphed. **Consequence: the DM-gate is defense-in-depth, not the primary secret boundary. Secrets a player must not extract should be partitioned at the system level so the model is never given them** (Inworld's Personal-Knowledge / 4th-Wall posture). Do not rely on "the model knows but chooses not to tell."
- *Latency vs reaction-window tension.* The mask only buys DM time if the thinking is genuinely slow or deliberately held. A small model chosen *for* low latency gives almost no window, and once the visible reply streams, a retraction is visible. This forces a **buffered checkpoint architecture**: generate hidden reasoning → hard-pause for DM approve/inject → *then* stream the visible reply. That pause is real added latency the mask must cover. **Needs a spike: the buffered-checkpoint pipeline (prototype the real pause, do not assume the mask is free).**
- *DM as bottleneck / vigilance decrement.* One DM cannot faithfully watch N NPCs' reasoning in real time; automation complacency guarantees rubber-stamping. Decide explicitly: the gate is a **per-critical-NPC spotlight** (works), not a **blanket real-time filter on all dialog** (fails at scale). Consider an automated pre-filter that surfaces only flagged reasoning (secret-adjacent, out-of-lore, refusal-worthy) to the DM.
- *Over-steering paradox.* Heavy DM injection pushes toward homogenized, puppeted-sounding NPCs (the same "same voice" complaint that dogs character.ai), the very failure the gate is meant to prevent. Budget DM interventions like a resource too.

### 1.4 Framing four: grounding and the provider seam

The driver reads a **context projection** of world/entity state (relevant tokens + their `SheetData` + the recent `GameEvent` tail + `roll_log`), all already serde-serializable, so no new substrate is required to *feed* a model. Inference is **optional, DM-loaded, and lives behind a provider seam**: in-process burn **or** an external local endpoint, chosen by capability, never bound as universal. Player browsers do not run the model. This is the burn provider-seam discipline applied verbatim: burn stays behind the seam, and the seam admits a non-model driver (the tree) as a first-class peer.

### 1.5 Data flow for one dialog turn

```
player types a question
        │
        ▼
[1] budget check                     plugin-computed cost vs remaining
    (isometry-system)                counter; in-battle = 1; reject if 0
        │  (ok)
        ▼
[2] assemble context projection      tokens + SheetData + GameEvent tail
    (host, reads isometry-core)       + roll_log + this NPC's known facts
        │
        ▼
[3] driver runs the dialog tools     tree walks nodes  OR  model reasons;
    (model OR deterministic tree)     either emits reveal/withhold/offer/…
        │
        ▼
[4] DM reviews / interrupts          reasoning shown privately; DM may
    (buffered checkpoint)             inject guidance before commit
        │  (approve)
        ▼
[5] commit reply as a GameEvent      ordinary data on the ordered log;
    (host authority)                  decrement budget counter
        │
        ▼
[6] replicate to players             iroh, players are pure receivers
    (isometry-net)
```

Steps 1, 5, and 6 are pure substrate and exist today. Steps 2 and 3 are the context projection and the driver seam. Step 4 is the buffered-checkpoint gate (the spike). Secrets referenced in step 3 must be system-partitioned per 1.3, not merely withheld by a cooperative model.

### 1.6 Deterministic tree vs LLM: same tools, different driver

| Dimension | Deterministic dialog tree | LLM driver |
|---|---|---|
| Tool surface | identical (query/reveal/withhold/offer/flag/end) | identical |
| Substrate dependency | none | none (behind the seam) |
| Determinism / replay | trivially byte-identical | result-only crosses the wire |
| Author cost | high (hand-authored nodes) | low (reasons over projection) |
| Runtime cost | ~0 | inference latency + DM-gate pause |
| Failure modes | rigid, exhaustible, predictable | drift, hallucination, leak, sameness |
| Secret safety | absolute (never encoded) | needs system-level partition + DM-gate |
| Role | the always-available floor | the optional accelerator |

The tree is the fallback that ships when no model is loaded. The two are not competitors; they are drivers behind one seam, and shipping the tool surface first makes the model a later opt-in swap rather than a rewrite.

---

## 2. World and entity graphs: what exists vs greenfield

### 2.1 Current state (from the ecosystem audit)

Entity/graph infrastructure in Isometry is **essentially greenfield**. `isometry-core` has `dice, event, grid, iso, map, path, sheet, template, turn, visibility` and **no `node`, `relation`, `edge`, `world`, or `graph` module**.

- Entity = `Token { id, at, facing, sprite, owner }` (`map.rs:40-49`) carrying `SheetData { system, fields: BTreeMap<String, FieldValue> }`, `FieldValue = Int | Text | Bool` (`sheet.rs:14-44`).
- **No inter-entity relationships exist as types.** `SessionEvent` is tile/token/elevation mutations; `GameEvent` adds turn-order + roll-log + `SheetSet`. No event asserts a relationship between two entities.
- The only "graph" is a 4-connected uniform-cost grid BFS (`path.rs:30-99`).
- The WORLD pointcrawl (nodes = sites, edges = weighted routes) is **explicitly deferred**; the 2026-07-07 recommendation is to defer the travel sim and build the transition-point primitive instead.
- Replicated state is small and clean: `GameSnapshot { map, turns, roll_log }`, fully serde/postcard, host-authority ordered log.

The upshot: the substrate is already a clean, flat, serializable **projection source**. A model does not need graph infrastructure to be *fed*.

### 2.2 The fork: light context-projection vs first-class knowledge graph

**Recommendation: build the light context-projection first; do not borrow Mere's graph-kernel wholesale.** Confidence: HIGH.

- Mere's `graph-kernel::Node` is a **webpage object** (`cached_host`, `favicon_rgba`, `thumbnail_png`, `viewer_override`, `NodeLifecycle` webview management). Importing it to represent an NPC or a room drags a browser's entire surface in. This matches the standing ruling: graphshell/mere is donor and grab-bag, cite for ideas, never copy wholesale; mere-kernel is canonical *for Mere*.
- What **is** worth borrowing is the **design** of Mere's edge taxonomy, not its code: `RelationKind = Semantic | Traversal | Containment | Arrangement | Imported | Provenance` with a read-side discriminant split from the write-side assertion, plus `RelationDurability::{Durable, Session}`. That typed-family + subkind + durability shape is a proven reference *if and when* a real graph is funded.
- For the DM model now: **project** relevant tokens + `SheetData` + the recent event tail into a prompt, **run** it through the seam on an off-thread actor, **keep it out of `isometry-core`** (core stays pure: no wgpu, no iroh, no serval). The projection reads core types; it never pollutes them. It belongs in the native host or a new `isometry-intel` / `isometry-brain` crate.

**When a first-class entity graph would be justified:** if the product grows to NPC relationship webs, faction politics, quest-dependency graphs, or the WORLD pointcrawl. The right move then is a **new pure-`isometry-core` graph module** over Isometry's *own* entities (reusing the weighted-BFS traversal), borrowing the `RelationKind` design as reference, **not** a Mere import. That graph then becomes a richer projection source for the same seam. It is a separable, product-gated feature; the context-projection ships first and works against today's flat substrate.

---

## 3. Local-model feasibility and recommended inference architecture

### 3.1 Bottom line (from the feasibility agent)

A typical DM laptop can run a small tool-calling LLM for interactive NPC dialog, and it should be **DM-only**. Confidence: HIGH.

- **Run inference on the DM host only.** HIGH. Falls out of DM-authority: the host owns state and adjudication; NPC tool calls mutate authoritative state; players receive streamed dialog over iroh. Nobody but the DM needs a GPU.
- **Keep inference out of player browsers.** HIGH. Browser WebGPU inference (WebLLM/wllama) is real but its tool-calling is immature and latency/quality worse. Reserve as a spike only if a DM-less/solo `isometry-web` mode is ever greenlit.
- **Model floor for reliable multi-turn tool use is ~3-4B, not 1B.** MED-HIGH. NPC dialog is multi-turn by nature, and multi-turn tool accuracy collapses below ~3B.

### 3.2 Latency

The governing metric is **time-to-first-token**, because dialog streams word by word. Text (unlike voice) is forgiving: players read at ~5 words/sec, so even 20-30 tok/s keeps up. A 3-4B int4 model on a modern laptop GPU or Apple Silicon streams a 40-token line in ~1-1.5s with sub-second TTFT. **Hidden cost: a tool-using line is two passes** (emit `tool_call` → DM executes → speak the result), so budget two TTFTs. Tool executions are local `isometry-system`/`isometry-core` calls (microseconds), not network round-trips.

### 3.3 Candidate models

| Model | Params | int4 RAM | Tool calling | Fit |
|---|---|---|---|---|
| **Qwen2.5-3B-Instruct** | 3.1B | ~2 GB | strong, BFCL-tuned | **best tools-per-byte; default pick** |
| **Llama-3.2-3B-Instruct** | 3.2B | ~2 GB | native multi-turn | most ubiquitous templates; safe baseline |
| **Phi-4-mini** | 3.8B | ~2.5 GB | native `<|tool|>` | solid, slightly larger |
| Gemma-3-4B-it / Gemma-4-E4B | ~4B | ~2.5-3 GB | via `<tool_call>`, over-triggers | workable; needs an anti-over-trigger system prompt |
| Llama-3.2-1B / Qwen2.5-1.5B | 1-1.5B | ~1 GB | mis-formats multi-turn | too weak for tool-first dialog |

Quantization: **Q4_K_M (int4) is the sweet spot**; the usual int4 caution is for heavy math/reasoning, not NPC banter.

### 3.4 The tool-calling crux

Two findings shape the design more than model choice:
- **Constrain decoding to the tool schema.** Small models "frequently fail to emit tool calls in the correct format" unless output is grammar/strict-schema constrained. llama.cpp and mistral.rs both ship this. **This single choice turns a flaky 3B into a reliable tool caller.**
- **Keep the tool set small, well-named, disjoint.** Keyword-triggered false calls (calling `get_weather` on the word "weather") argue for few curated tools mapped onto existing Lua actions.

### 3.5 Recommended architecture: one seam, DM-only, external-first, burn-eventual

A single trait seam (`DialogEngine` / `NpcBrain`, new crate e.g. `isometry-brain`) shaped on the **OpenAI chat+tools** contract, which all three candidate runtimes speak. Tools bridge to `isometry-system` Lua actions and `isometry-core` reads; results and streamed text propagate over `isometry-net`.

| Engine | Cross-platform GPU (4 targets) | Tool calling | In-process | Verdict |
|---|---|---|---|---|
| **External local endpoint (Ollama / llama.cpp server)** | excellent (Vulkan/Metal/CUDA/CPU) | mature + grammar | no | **first impl / default** |
| **Embedded `llama-cpp-2`** | excellent | mature + grammar | yes | in-process alt (C build dep) |
| **Embedded `mistral.rs`** | CUDA/Metal strong, Vulkan unclear | mature + strict-schema | yes | in-process alt where CUDA/Metal guaranteed |
| **Burn-LM (burn/wgpu)** | excellent (wgpu) | none documented yet | yes | **strategic target, later swap** |

**Why external-first despite the burn endorsement.** The burn doctrine is about keeping the in-process ML *lane* first-class behind a seam, and this design does exactly that. But Burn-LM today ports only Llama-family models (Llama 3/3.1/3.2 + TinyLlama), with **no documented quantization, chat templating, or tool-calling**. Making it the default would mean porting a model and building the tool/template layer yourself: a multi-week spike, not a weekend. The llama.cpp lane meets the requirement (reliable multi-turn tool calls on all four desktop GPUs) now. The seam honors burn strategically without blocking the feature on it. This is the low-regret "support both" resolution.

Note: Mere already has a validated in-process burn-wgpu inference lane (`intel/infer`: `InferenceProvider` seam, real llama decoder, TinyLlama at ~10 tok/s on GPU, streaming actor, canned no-GPU stub). Isometry consumes **none** of it today (zero burn/intel references in any Isometry `Cargo.toml`); it lives in Mere, not the serval/netrender stack Isometry depends on. Reuse would mean **promoting `intel/infer` to a standalone crate** (the wgpu-graft/weld/scry precedent; its seam is a clean ~215-LOC, `Send+Sync`, wasm-safe core) or mirroring its ~3-type seam. Either way the burn decoder, actor, and canned-stub patterns transfer directly when the `BurnLmEngine` swap comes.

### 3.6 Spikes (with confidence)

1. **Seam + local endpoint end-to-end**: one NPC calls one real Isometry tool and speaks the streamed result on the DM host. *Works: HIGH.*
2. **Model bake-off on Mark's hardware**: Qwen2.5-3B vs Llama-3.2-3B vs Phi-4-mini vs Gemma-3-4B on Windows (Vulkan) and iMac (Metal); measure tool-call correctness, TTFT, tok/s. Output: default model + per-target numbers. *Usable default: HIGH.*
3. **Grammar-constrained tool output**: confirm schema-constrained decoding makes a 3B emit valid tool JSON reliably. *Reliability unlock: HIGH.*
4. **Cross-platform GPU matrix**: verify acceleration on all four targets (Win/Fedora/Mint Vulkan, iMac Metal). *llama.cpp Vulkan/Metal: HIGH.*
5. **Buffered-checkpoint pipeline**: the hidden-reasoning → DM-pause → stream architecture from 1.3; prototype the real added latency. *Needs a spike; the flagship's load-bearing UX risk.*
6. **Burn-LM viability**: stand up Burn-LM with Llama-3.2-3B on wgpu/Vulkan; measure tok/s vs llama.cpp; scope the tool/template + second-architecture effort. *Runs Llama on wgpu: MED-HIGH. Has tool ergonomics today: LOW.*
7. **(Optional) Browser spike**: only if DM-less/solo mode is greenlit. *Production-ready: LOW.*

---

## 4. Prior-art lessons and the failure modes to design against

The design's originality is the **pairing**: a tabletop-proven conversation-budget (a *volume* control) with a Wizard-of-Oz editorial gate (a *content* control) over an authored substrate the LLM merely optionally accelerates. No shipped system combines a per-encounter budget with a live human editorial gate. That pairing addresses more of the documented failure surface than any shipped system (Inworld, NVIDIA ACE/Covert Protocol, AI Dungeon, Suck Up!, 1001 Nights, Hidden Door, Vaudeville, character.ai/Replika).

**Failure-mode taxonomy and coverage:**

| Failure mode | Evidence | Covered by | Residual risk |
|---|---|---|---|
| Secret leak | Gandalf/grandma: blocklists + a 2nd guarding LLM still bypassed (acrostic, reversed, story-embedded) | DM-gate sees forming intent | high vs a determined player; **must partition secret at system level** |
| Hallucinated facts | AI Dungeon off-lore improvisation | DM correction + grounding (41.8% fewer) | hallucination *within* an allowed answer persists |
| Character drift | character.ai (~300 msgs), Replika persona-swap | DM re-anchor within an answer | long-horizon identity still fragile |
| Jailbreak compliance | role-play jailbreaks "most persistent" | DM spots the frame | one crafted prompt can suffice; budget caps *volume* not *content* |
| Latency | NVIDIA SLM push; ~7s indie generations | thinking-as-mask | mask fights the small-model choice; checkpoint adds real latency |
| Sameness / mode collapse | RLHF homogenization | distinct authored personas | **DM over-steering can cause the very sameness it fights** |
| Stat-gating lockout | 4e skill-challenge critique | (design choice) | decouple volume from effect, or give low-CHA a different lever |

**The two load-bearing risks are technical and human, not conceptual:**
- **Unfaithful chain-of-thought.** The DM's window into the NPC's mind is partial; a leak can bypass the visible reasoning. So secrets must be **withheld at the system level**, not merely guarded. The DM-gate and the budget both leak against a determined player; entity-graph grounding + hard secret-partition carry that weight.
- **DM vigilance + latency limits.** The gate must be a **selective, flagged spotlight** with a genuine buffered checkpoint, not a blanket real-time filter on all dialog.

Neither the DM-gate nor the budget stops hallucination-within-an-allowed-answer, homogenized voice, or a single well-crafted extraction on their own. Design accordingly.

---

## 5. The broader opportunity catalog, ranked

Leverage = value × substrate-fit ÷ (effort + risk). Each tagged with the lane it rides. The determinism firewall + DM-gate apply to all: the host runs the model once, only the result crosses the wire, and everything degrades to a deterministic floor.

| # | Opportunity | Rides | Model | Feasibility | Note |
|---|---|---|---|---|---|
| 1 | **Session recap / continuity digest** | no new lane (reads log + roll_log) | small local; cloud for polish | HIGH | lowest effort/risk; input is already the authoritative log; ceiling is the geometry-vs-story gap |
| 2 | **Semantic lore search + rules RAG** | generators (`>find`/`>q` already specced) | small local embedder (MiniLM ~46MB) | HIGH | offline, <100ms; cited SRD answers; cleanest fit-to-planned-lane |
| 3 | **NL → command intent for `>` composer** | generators | small local, JSON/grammar-constrained | HIGH | one host-side pre-parse; unparseable → literal parser; value scales with generator content |
| 4 | **Generative content over the tables** (quests/encounters/loot/NPCs/stat blocks) | generators + schema/ABI | cloud/mid for prose; small local for schema | MED | highest ceiling, fork-gated; **killer sub-move: model authors the deterministic Lua tables at prep time, runtime stays offline** |
| 5 | **Board-to-text narration** (accessibility + shared perception primitive) | new, tiny, high-reuse | none for facts; small local for fluency | HIGH | factual layer is a deterministic serializer (`path_to`, `visible_from`); feeds #1 and #6; **build early** |
| 6 | **Tactical co-pilot / monster autopilot** | schema/ABI + existing geometry | mid/cloud for tactics; small local for legal moves | MED | agent over `reachable`/`visible_from`; advises DM or drives monster `Intent`s host-validated; authority-clean |
| 7 | **NL stat-block / sheet import → `SheetData`** | schema/ABI | small local, grammar-constrained | MED | onboarding cold-start tax; gated on `List`/`Map` field widening; forcing function for it |
| 8 | **NPC memory + evolving relationships** | schema/ABI + persistence open Q | small local store; mid/cloud reasoning | LOW-now / HIGH-later | needs persistent NPC records + relationship edges; no cross-session persistence today; multiplier for the flagship |

**Below the cut (evaluated, ranked out):**
- **Token/sprite generation**: real pain but quality-risky for fixed-palette iso pixel-art; integrate an existing prep-time tool (PixelLab, Retro Diffusion's local Aseprite extension), don't build in-app.
- **Map/battlemap from text**: tractable but the editor and viewport windowing aren't ready for big boards; defer, pairs with world-graph/REGION.
- **Adaptive difficulty**: the valuable 80% (CR/XP-budget advice) is deterministic math, no model, belongs in the co-pilot; true auto-scaling fights DM authority.
- **Living-world simulation ticks**: most premature; needs the world-graph + faction model + cross-session persistence (#8's blocker).
- **Translation/localization**: premature (no content corpus yet); rides #4's output stage later.
- **Voice (TTS/STT)**: lowest leverage as its own lane; cross-platform audio tax across four targets; an output stage to bolt onto dialog and #5, not a foundational bet.

**The one substrate change that unlocks the cluster:** a **semantic annotation channel on the log** (a `GameEvent::Narration`/note variant, or promoting GM-notes into replicated events) carrying optional human- or model-authored story text beside the geometry. Stays pure (just more replicated data, `#[serde(default)]`-migratable like the roll_log widening was), and raises the ceiling of recap (#1), search (#2), and living-world work at once by closing the geometry-vs-story gap. Pair with one infra decision: an **optional local-model sidecar** (Ollama-compatible HTTP so one seam serves local and cloud) **plus a MiniLM-class embedder**, enough to power #1, #2, #3, #5, #7 without a frontier dependency. Do #5's board-to-text serializer early regardless.

---

## 6. Roadmap placement

**This is a post-keystone horizon, recorded now, NOT sequenced ahead of viewport windowing.** Viewport windowing remains the top-priority performance fix; nothing in this document displaces it.

**Hard dependencies (why this cannot lead):**
- **Widened schema/Lua ABI.** The conversation economy (§1.2) needs the plugin to read trust/reputation/environment and return a derived budget; the current int-only `call_int` boundary cannot see or express that. Rich content and sheet import (§5 #4, #7) need `FieldValue::{List, Map, Float}`.
- **Generators / command-grammar lane.** The flagship's dialog output and catalog #2/#3/#4 ride the `>gen`/`>q`/`>find` verbs and the `call_gen(func, args) -> GenValue` entry point. A DM-loaded model is the higher-order sibling of the Lua/Tracery generators there: same host-authoritative, single-sided, result-crosses-the-wire, `[insert][reroll][discard]`-previewed flow.
- **Optionally the world/entity graph.** Not required for the flagship (the light context-projection works against today's flat substrate). Becomes a dependency only for the relationship/faction/living-world tier (#8 and below).

**Sequencing sketch once the keystone lands (illustrative, not a commitment):**
1. Board-to-text serializer (§5 #5): pure, un-gated, pays three features. Buildable early.
2. The `DialogEngine` seam + deterministic tree floor (the tool surface, no model).
3. Recap (#1) and RAG search (#2): ride existing/planned lanes, HIGH feasibility, small local models.
4. External-endpoint `DialogEngine` impl + model bake-off spikes (§3.6).
5. The buffered-checkpoint DM-gate (§1.3 spike) and the conversation economy against the widened ABI.
6. `BurnLmEngine` swap when Burn-LM grows tool/template support.
7. Graph-gated tier (#8, living world) only if funded as a product goal.

---

## 7. Open questions for Mark

1. **Secret boundary posture.** Accept that secrets must be system-partitioned (the model is never given them) rather than "known but withheld"? This changes how NPC knowledge is authored (Personal vs Common knowledge partitions) and is the honest answer to CoT unfaithfulness.
2. **DM-gate scope.** Per-critical-NPC spotlight, or blanket real-time filter? The prior art says the blanket version fails at scale. If spotlight, what flags an NPC's reasoning for DM attention (secret-adjacent, out-of-lore, refusal-worthy)?
3. **Conversation economy stat-gating.** Should CHA raise *how many* questions, *how well each lands*, or both? Coupling both risks locking low-CHA players out of the social pillar. What alternative lever do low-CHA players get (evidence, leverage, environment)?
4. **Word-limit vs compromise.** Hard per-question word caps (risk: reads as a glitch) or a Duel-of-Wits-style partial/compromise outcome?
5. **`intel/infer` promotion.** Promote Mere's inference seam to a standalone crates.io crate (wgpu-graft precedent) so Isometry can consume it, or mirror the ~215-LOC seam independently?
6. **Local-only vs cloud-at-prep.** Hold to local-only for on-ethos offline play, or allow frontier cloud models for latency-tolerant prep-time authoring (recap polish, table compilation in §5 #4's killer sub-move)?
7. **The annotation channel.** Add `GameEvent::Narration` (or promote GM-notes to replicated events) now as the shared story layer, given it is `#[serde(default)]`-migratable and unlocks recap/search/living-world at once?
8. **World/entity graph trigger.** What product goal (NPC relationship webs, factions, quest graphs, the WORLD pointcrawl) would justify building the first-class pure-core graph module (borrowing Mere's `RelationKind` *design*, not its web-graph code)?

---

## 8. Decisions and refinements (2026-07-07, Mark)

Resolutions to the open questions and design additions from the review. These supersede the corresponding open questions above.

- **Buffered checkpoint: adopted (Q1.3 spike stands).** Rationale added: dialog is already turn-chunked, and each party needs a beat to evaluate the other's response, so the hidden-reasoning to DM-pause to visible-reply rhythm matches the natural cadence of conversation rather than intruding on it. The pause is intuitive, not a tax.

- **Secrets: sealed-secret plus revelation-condition (resolves Q7.1).** System-partition is the floor, but the design goal is *revealable* secrets earned through in-character play, not hard-withheld and not prompt-hackable. Mechanic: a secret's text stays out of the model's context by default (so it cannot be extracted by prompting), and the `reveal_fact(id)` tool for that secret is **locked** until a DM-set **revelation condition** is met. The condition is a soft in-fiction trigger, not a scripted reveal node: a trust threshold crossed, a topic broached with sufficient standing, a password or clue produced, a persuasion success. When play satisfies it, the secret enters context and the tool unlocks, and the NPC reveals it in its own words. The DM authors the secret and its lock without pre-deciding how the reveal happens. This keeps the "finagle a secret through real interaction" charm while blocking out-of-character prompt-hacking (the honest answer to CoT unfaithfulness: the model is never given what it must not reveal until the fiction earns it).

- **Charisma and trust are separate axes (resolves Q7.3).** Charisma is the cold-open lever; **trust is its own earned axis and overrides it.** A known, trusted low-Charisma character still gets a real conversation, possibly a long one, because trust was earned over prior play. Low Charisma means you must earn standing first, not that you are locked out of the pillar. Other stats get their own non-social levers (evidence, leverage, intimidation, environmental pressure). Decouple volume from effect.

- **Verbosity pricing, not a hard word cap (resolves Q7.4).** Drop the word limit. Instead, price verbosity: a longer or wordier question costs more of the response-token budget, scaled by the Charisma you lack. A silver tongue says more for less; a clumsy one burns the exchange fast. Soft economic pressure that never reads as a truncation glitch.

- **Duel of Wits as the high-stakes escalation (extends Q7.4).** The lightweight response-token budget covers casual info-gathering. For marquee negotiations (talk down the warlord, broker a treaty, bluff the gate) the exchange escalates into a bounded, scored contest modeled on Burning Wheel's Duel of Wits: each side has a Body-of-Argument pool seeded by trust/reputation plus a social skill, and the **compromise mechanic** grades the outcome by margin of victory, so even a win concedes something proportional. This is the "boss-fight" form of the conversation economy; the verbosity pricing rides underneath the routine tier.

- **DM-gate is a per-critical-NPC spotlight (leaning, from the overall posture, Q7.2).** Not a blanket real-time filter on all dialog. An auto-pre-filter surfaces only flagged reasoning (secret-adjacent, out-of-lore, refusal-worthy) to the DM. Left open: the exact flagging heuristic.

- **Promote `intel/infer` to a standalone crate (resolves Q7.5).** Confirmed. Promote Mere's burn-wgpu inference seam to a standalone crate (the wgpu-graft/weld/scry precedent) so Isometry and others can consume it. Name: **vates** (Latin, the poet-prophet), chosen 2026-07-07. It is the one word that fuses speech (the bard who voices) with prophecy (the seer who infers), which is exactly a crate that both voices NPCs and runs inference. Free on crates.io. The field was run first: oracle (Oracle DB/Corp), cassandra (Apache Cassandra, plus the cursed never-believed connotation), pythia (a well-known open LLM family), and sibyl (an Oracle DB driver crate) were ruled out on collision; sybil was ruled out on connotation (the P2P Sybil-attack, wrong in a P2P codebase). sibylla and vates were the collision-clean finalists; vates won on precision and for carrying no prior baggage.

- **Board-to-text narration: near-term buildable, NOT post-keystone gated.** The factual layer needs no model and no ABI widening (pure isometry-core reads plus templated text), so it escapes this document's post-keystone gating and can be built early, even beside viewport windowing. Scope: **N1** a `narrate` module in isometry-core serializing scene facts (positions, facing, distance via `path_to`, LOS/fog via `visible_from`, elevation, turn) with a done-condition of accurate text for a known board; **N2** viewer-relative and fog-aware (omits what the viewer's tokens cannot see); **N3** (later, gated) an optional model fluency pass behind the provider seam. It is the shared perception primitive, feeding accessibility and text-only play, session recap, and the model's own grounding projection. Candidate for its own short plan.

Still open: Q7.6 (local-only vs cloud-at-prep) and Q7.7 (add `GameEvent::Narration` now).

