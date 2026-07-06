# Turnbase Architecture

Turnbase is a headless, deterministic game engine framework for Rust. It provides
an event-sourced-flavored, side-effect-free core that lets you define any turn-based
game once and get simulation, AI training (MCTS/RL), and headless playtesting for free.

Core promise: **async-free, UI-free, pure computation**. `turnbase-core` has no
networking, no rendering, no I/O, and no tokio/async-std in its dependency tree.
Everything is a synchronous function from state + action to new state.

This document records the architecture decisions made for v1 and the reasoning
behind them. Full research backing each decision lives in `.matan/research*.md`.

## Non-goals for v1

- Networking / multiplayer transport (turnbase-core is a library; wire it up yourself)
- Rendering / UI of any kind
- A scripting/DSL layer for effects (post-0.1 stretch — see "Future: scripting")
- Full MTG-style priority stack (Tier 3 triggers — targeted for v0.2/v0.3)

## Core trait shape

No `G` + `Ctx` split like boardgame.io. Instead, one `State` type per game, and the
engine asks questions of it through a trait — closer to OpenSpiel's `State` object
than boardgame.io's reducer. Turn/phase bookkeeping is just fields on your own
state; the engine never owns a parallel struct you have to keep in sync.

```rust
pub trait Game {
    type State;
    type Action;
    /// What a player or spectator is allowed to observe, produced by `view`.
    type View;

    /// The initial position for a match. Per-match configuration (player count,
    /// board size, variant rules, scoring options) lives on the `Game` value
    /// itself (`&self`), not baked into every `State`. This mirrors OpenSpiel's
    /// `Game`/`State` split: config is stored once, so `State` stays lean and
    /// cheap to clone (the search primitive — see "Move application"), and
    /// there's a single home for `num_players`, variant flags, etc. The seed
    /// initializes the state's own serializable generator (see "Determinism").
    fn new_initial_state(&self, seed: u64) -> Self::State;

    fn num_players(&self) -> usize;

    /// Players who owe a decision right now, as an ordered, deterministic set
    /// (never a `HashSet` — iteration order is observable behavior, see
    /// "Determinism"). Empty during engine-only resolution steps (e.g.
    /// adjudicating a Diplomacy turn). More than one player during
    /// simultaneous/secret phases (Diplomacy orders, Civ-style planning).
    /// Exactly one player is just the common case, not a privileged concept.
    fn active_players(&self, state: &Self::State) -> ActivePlayers;

    /// Choices available right now, for one of the active players. A "turn" can
    /// span many `apply()` calls before `active_players()` changes — Diplomacy
    /// asks "which unit, then which order" as two decision points; tic-tac-toe
    /// just happens to have turns of length 1.
    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action>;

    /// Cheap validity check for one action, usable even when the full legal
    /// set is too large or unwieldy to materialize eagerly (see "Decision
    /// points with large branching factors" below). Defaults to checking
    /// membership in `legal_actions`; games with huge-but-structured decision
    /// points (arbitrary map targeting, mana-payment combinatorics) should
    /// override this with a direct check instead of enumerating.
    fn is_legal(&self, state: &Self::State, player: PlayerId, action: &Self::Action) -> bool {
        self.legal_actions(state, player).contains(action)
    }

    /// Advances state in place. The one required mutator, and a pure function
    /// of state — the RNG lives *in* `state` (see "Determinism and RNG"), so
    /// there's no `&mut dyn RngCore` to thread or forget. Backtracking search
    /// uses `apply_cloned` (copy-make) by default; games that need make/unmake
    /// speed implement the optional `Reversible` trait instead of paying an
    /// undo tax on every game.
    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action);

    fn is_terminal(&self, state: &Self::State) -> bool;

    /// A player's outcome in a terminal state — "did you win," as a single
    /// scalar for search/RL. Deliberately not richer than this: games with
    /// multi-dimensional scoring (Civilization's five victory tracks, several
    /// currencies) expose that richness as ordinary public fields on `State`
    /// instead. Reward is the engine's minimal terminal signal, not a
    /// general-purpose scoring API.
    fn reward(&self, state: &Self::State, player: PlayerId) -> f64;

    /// Optional per-step reward for RL trainers that need a dense signal rather
    /// than one terminal scalar. Defaults to 0 (all signal at the end), keeping
    /// simple games trivial while letting shaped-reward and general-sum games
    /// emit per-player, per-step returns — mirrors OpenSpiel exposing both
    /// `Rewards()` and `Returns()`. Sparse terminal-only reward is exactly RL's
    /// hard case, so "training for free" needs this hook to be honest.
    fn step_reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let _ = (state, player);
        0.0
    }

    /// What `viewer` is allowed to observe. `None` is a seatless spectator and
    /// sees the public projection only — encoding the observer case in the type
    /// avoids boardgame.io's null-`playerID` class of crashes (issue #989). For
    /// a seated player, defaults to the engine's standard rule (public zone plus
    /// the viewer's own private zone, see `view_for`), which covers the common
    /// case (your own hand, hidden from everyone else). Override when a game's
    /// visibility rule is inverted — e.g. Hanabi, where every player sees every
    /// *other* hand but not their own, is exactly the opposite of the default.
    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View;
}

/// Ordered, deterministic set of players who owe a decision. Wraps a
/// `BTreeSet<PlayerId>` rather than exposing a raw collection as trait surface,
/// so callers get stable iteration without the engine committing to a concrete
/// set type in its public API.
pub struct ActivePlayers(/* BTreeSet<PlayerId>, private */);

/// Opt-in make/unmake for games where cloning the whole state per search node
/// is too slow (chess-scale branching). OpenSpiel required undo of every game
/// and then had to *remove* it from several because correct reversal needs
/// "extra nontrivial book-keeping" (its own `UndoAction` doc notes undo is
/// "only necessary for algorithms that need a fast undo, e.g. minimax");
/// keeping it optional means a wrong `undo` (which silently corrupts search
/// rather than crashing) is only ever a bug you opted into.
///
/// RNG invariant: `UndoRecord` MUST capture the generator's position *as it was
/// before* the move — a small `Copy` value (see "Determinism and RNG") — and
/// `undo` MUST write it back. This is O(1) and allocation-free, but it is an
/// explicit obligation, not automatic. A single `apply` may consume a variable
/// number of draws (rejection sampling, loops that depend on state), so the
/// pre-move position CANNOT be recovered by subtracting a draw count; it must
/// be snapshotted up front. The clone path (`apply_cloned`) gets this for free
/// — cloning `State` clones the generator with it — which is exactly why undo
/// is the opt-in fast path and clone is the default.
///
/// Simultaneous games need the same care OpenSpiel calls out: the record must
/// also restore whatever buffered-submission bookkeeping the move advanced, not
/// just the board.
pub trait Reversible: Game {
    type UndoRecord;
    fn apply_undoable(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) -> Self::UndoRecord;
    fn undo(&self, state: &mut Self::State, record: Self::UndoRecord);
}
```

`apply_cloned()` (pure, `Clone`-based) is the default backtracking primitive: a
provided helper that clones the state and calls `apply` on the copy, so search
algorithms and property tests get copy-on-write semantics without any game
author hand-writing reversal logic. Copy-make is simple, always correct, and —
per chess-engine practice — fast enough for the overwhelming majority of games.
`Reversible` (make/unmake) is the opt-in fast path for the few games where
per-node cloning actually dominates the search budget.

### Why not event-sourcing or ECS for the core?

Both were evaluated. Event-sourcing gives the best auditability/replay story but
is heavyweight (external event store, aggregate design) for a library meant to
scale down to tic-tac-toe. ECS (bevy_ecs) is excellent for effect-scheduling
extensibility but its correctness depends on system ordering, which fights the
determinism guarantee. The trait-based `apply` model (clone to backtrack, opt
into `Reversible` only when search perf demands it) is simplest to author
against, cheapest for AI search, and doesn't force simple games to pay for
machinery they don't need. Event-sourcing-style logging (move log + snapshots)
is still available as an *optional* layer on top, for replay/debugging, without
being the foundational state representation.

## Turn structure: `active_players`, not `current_player`

Research across Diplomacy (simultaneous secret orders), Star Wars Rebellion
(asymmetric per-faction phases), Twilight Struggle (alternating with event
interrupts), and Civ-style 4X (plan-then-resolve) found five distinct turn-order
archetypes. `current_player: PlayerId` is not a universal concept — it's the
1-element special case of a broader "who owes an action right now" set.

Building on an ordered `active_players` set (`ActivePlayers`, not a `HashSet` —
see "Determinism and RNG") from day one avoids bolting simultaneous-turn support
on later:

- Strict alternating (chess, tic-tac-toe): `active_players` is always a singleton.
- Simultaneous-secret (Diplomacy): `active_players` is everyone during the orders
  phase, empty during adjudication.
- Asymmetric (Rebellion): `active_players` is faction-specific and phase-dependent.
- Plan-then-resolve (Civ): `active_players` is everyone during planning, empty
  during the deterministic resolution pass.

### Built-in simultaneous-action buffering

When more than one player is active, the engine buffers each player's submitted
action per phase and only invokes a `resolve()` hook once all active players
have submitted. Game authors don't reimplement "wait for everyone" bookkeeping
per game. `resolve()` must consume the buffered actions in a deterministic
order (players sorted by `PlayerId`, never hash order) — otherwise two replays
of the same seed + inputs can diverge. This is why `active_players` is an
ordered set and `private` is a `BTreeMap`, not their hashed equivalents.

PettingZoo's design paper is the cautionary tale here: it argues true
"everyone acts at once" (POSG) APIs are "not conceptually clear for games
implemented in code" and make race conditions "a very easy mistake," and so
sequentializes everything into an agent-environment cycle. This buffer-then-
`resolve()` model is exactly that sequentialization — each player's submission
is recorded independently and the environment reconciles conflicts once, in a
fixed order — so simultaneity never becomes a source of order-dependent bugs.

## Hidden information: enforced public/private split

boardgame.io's `playerView(state) -> redacted` is hand-written per game and easy
to get wrong by omission. Turnbase requires games to shape their state as:

```rust
pub struct State<P, Q> {
    pub public: P,
    private: BTreeMap<PlayerId, Q>,   // ordered + behind accessors, not raw pub
}
```

The private map is a `BTreeMap` (ordered, for determinism) and is not `pub`:
games reach it through accessors (`private(player)`, `private_mut(player)`)
rather than the engine committing a concrete collection type to its public
surface. Redaction becomes mechanical: a player's view is `(public,
private.get(player))` — there's no field to forget to strip, because the view
only *exposes* your own private data through `private(you)`, not the whole map.
This mirrors OpenSpiel's "information set" formalism: two states differing only
in *other* players' private data are the same information set to you. A `None`
(spectator) viewer simply gets `public` with no private entry at all.

Stratego: `public` = board positions + captured piece history, `private[player]`
= piece rank assignments for that player's pieces.

Rebellion: `public` = revealed fleet positions + Empire bases, `private[Rebel]`
= true fleet positions + planned orders.

### When the default redaction rule is backwards: Hanabi

The engine's `view_for(player)` default — public zone plus the viewer's own
`private[player]` entry — covers the overwhelmingly common case (your own
hand, hidden from everyone else). Hanabi inverts it: every player sees every
*other* player's hand, but not their own. That's not a data-shape problem
(`State<P, Q>` still works fine), it's a redaction-*rule* problem — the fixed
engine default can't express "public plus everyone's private zone except
yours."

`Game::view(state, Some(player))` is a required, game-defined method precisely
so this isn't a special case bolted onto the engine: the engine still provides
`view_for` as a convenience default that most games can just delegate to, but
Hanabi (and similar inverted-visibility games, e.g. blind-auction variants where
you see others' bids but not your own) implements `view` directly against
`private` however its rules actually work. The `None` spectator case stays
mechanical regardless — public projection only.

## Response windows: bounded objections, no stack required

Pressure-testing against Coup (any player may challenge or block a declared
action before it resolves) and XCOM-style overwatch (a move can trigger a
reaction from a different, non-active player mid-resolution) initially looked
like it needed Tier 3 machinery. It doesn't. "Declare an action, give every
other player one chance to object, then resolve based on the outcome" is
exactly the decision-point pattern already in this document — nothing new:

```rust
// Resolving one player's declared action inserts a decision point for others:
active_players(state) -> {everyone but the actor}   // "does anyone object?"
legal_actions(state, other_player) -> [Pass, Challenge, Block]
// apply() branches on the responses, then active_players() returns to
// whoever's turn it actually is
```

The distinction that matters: a **bounded** response window (ask once, resolve)
is just more decision points using `active_players`/`legal_actions` as they
already exist. What actually requires Tier 3 is MTG's *re-openable* LIFO stack
— a response can itself be responded to, arbitrarily deep, with state-based
action rechecking after every resolution. Coup and overwatch never re-open;
they ask once and move on. Don't defer "can another player object" to Tier 3
by default — only the unbounded, re-openable version needs it.

## Move application

`apply(&self, &mut State, PlayerId, Action)` is the one required mutator —
in-place, and a pure function of state (the RNG lives *in* state, see below).
The default backtracking primitive is `apply_cloned()`: clone the state, `apply`
to the copy, hand back a `Result<State, Error>`. Copy-make is always correct
and, per chess-engine practice, fast enough for the vast majority of turn-based
games; no game author hand-rolls reversal just to get a pure API or drive search.

Games at chess-scale branching, where per-node cloning genuinely dominates the
search budget, opt into the `Reversible` trait for make/unmake
(`apply_undoable`/`undo`, mirroring OpenSpiel's `ApplyAction`/`UndoAction` and
chess engines' make/unmake). This is deliberately *not* the default: OpenSpiel
required `UndoAction` of every game and then had to remove it from several
because correct reversal needed "extra nontrivial book-keeping," and a wrong
`undo` corrupts search silently instead of crashing. Making it opt-in means that
footgun is only ever one you reached for on purpose.

## Determinism and RNG

The RNG lives *inside* `State` as a serializable, counter-based generator (a
concrete `Prng` newtype the engine provides — think PCG/philox: a seed plus a
64-bit counter, not an opaque `Box<dyn RngCore>`). Three properties fall out of
this one decision:

- **Snapshot + resume is O(1).** Serializing `State` serializes the RNG
  position with it, so a match resumes from a single saved state — no replaying
  the entire action log from turn 1 (which grows without bound over a long 4X
  game). boardgame.io keeps its PRNG counter in state for exactly this reason;
  a "never serialize the RNG" plan can't resume from a snapshot at all.
- **`undo` can rewind the random stream.** Because the generator's position is
  part of state, a `Reversible` game's `UndoRecord` snapshots that position
  before a move and `undo` writes it back, so re-exploring an undone branch
  resamples *identically* — closing the gap a side-channel `&mut dyn RngCore`
  would leave open. Automatic on the clone path; a one-word copy the record
  must carry on the make/unmake path (see the `Reversible` invariant).
- **Algorithm-swap stays cheap.** Keeping `apply` free of an `&mut dyn RngCore`
  parameter (games pull randomness from `state`'s `Prng`) lets the concrete
  generator change under the hood without rippling through every call site —
  the same benefit the trait object gave, via a concrete wrapper type instead.

Concretely, `Prng` must be a fixed-algorithm, integer-only generator whose
*entire* reproducible position is a small `Copy` value the engine can read and
restore — a counter-based RNG (Philox/Threefry are literally stateless functions
of `(key, counter)`), a seekable LCG-family generator (PCG exposes jump-ahead,
and jumping backwards is just jumping forward around the cycle), or ChaCha
(whose `get_word_pos`/`set_word_pos` expose a 128-bit position over its internal
block buffer). It is deliberately *not* a `Box<dyn RngCore>`: a trait object
erases the concrete state, so it can be neither snapshotted into an `UndoRecord`
nor serialized portably. No float-based generator either, for the same
cross-platform reason std's randomized `HashMap` hasher is banned here. This is
standard rollback-netcode practice — GGPO-style engines snapshot the RNG
position as part of frame state and restore it on rewind — and OpenSpiel
likewise serializes RNG state for its implicitly-stochastic games.

Determinism also depends on *iteration order*: `active_players` is an ordered
`ActivePlayers` set and `private` is a `BTreeMap`, never `HashSet`/`HashMap`.
Rust's std hashers randomize iteration order per process; Bevy had to abandon
std `HashMap` for a fixed hasher for exactly this reason. Any game logic that
consumes RNG or resolves buffered actions while walking a hashed collection
would desync two replays of the same seed. Ordered collections make that class
of bug unrepresentable rather than merely discouraged.

## Randomness, part 2: explicit chance nodes for committed outcomes

Pressure-testing against Battle Line, Star Wars Rebellion, Root, and Slay the
Spire found a remaining gap: even with the RNG counter living in state, there's
nowhere natural to put an outcome that's secret from *everyone* (a shuffled
deck's order isn't `public`, and it isn't any one player's `private[PlayerId]`
either), and while `undo` now rewinds the RNG stream cleanly, a value
*resampled* on a fresh re-exploration still isn't the same committed fact that
players may have already observed and reasoned about.

The fix is a hybrid, not a wholesale replacement:

- **Cheap, uncommitted rolls stay implicit.** Combat dice, random enemy
  intent, anything no player commits a decision against keeps pulling from
  `state`'s `Prng` inside `apply()`. Resampling such a roll on a fresh
  (non-`undo`) re-exploration is acceptable — but it is *not* free of
  consequence, and calling it "harmless" earlier was an overstatement. Naive
  resampling across determinizations is the source of the classic
  strategy-fusion and non-locality biases in imperfect-information MCTS (Frank
  & Basin; Cowling et al.): the search fools itself by assuming it will "find
  out" hidden values it actually cannot distinguish. Rule of thumb: if a
  search's *decisions* depend on the outcome, model it as a chance node
  (below); reserve implicit rolls for outcomes no strategy branches on.
- **Outcomes that become visible/committed state go through explicit chance
  nodes instead.** A reserved `Chance` pseudo-player appears in
  `active_players()` when the game needs to reveal a card, deal a hand, or
  otherwise fix an outcome that players will observe and reason about later.
  `legal_actions(state, Chance)` returns the possible outcomes (e.g. "draw the
  3\u2665"); `apply`/`undo` treat a chance move exactly like a player move \u2014
  it becomes real, undo-able state instead of a side channel.

```rust
// A deck draw becomes a real, undo-able action instead of an RNG side effect:
active_players(state) -> {Chance}                 // a draw is needed
legal_actions(state, Chance) -> [Card::Ace, Card::King, ...] // remaining deck
apply(state, Chance, Card::Ace)                    // consumes the top card
undo(state, record)                                // Reversible games: puts Card::Ace back
```

No third zone is needed on `State<P, Q>` for this: deck order is simply
`Chance`-owned data living wherever is convenient in the game's own state
(often `public`, since its *contents* aren't secret once you accept "nobody
has looked yet" ≠ "hidden from a specific viewer") — a convention, not a new
type. The `Chance` pseudo-player is the only thing new here, and it reuses
every existing mechanism (`active_players`, `legal_actions`, `apply`, `undo`).

## Triggered effects: Tier 2 now, Tier 3 later

Trigger/stack research found a clean three-tier ladder:

1. **Tier 1** — observer pattern, immediate dispatch (Slay the Spire relics).
2. **Tier 2** — event queue, enum-based events/triggers, ordered by
   controller/summon-order, no player responses (Hearthstone).
3. **Tier 3** — LIFO priority stack, priority-passing loop, state-based-action
   rechecking, response windows (MTG).

The jump from Tier 2 to Tier 3 is "surgical, not architectural" *if* Tier 2 is
built with triggers as queued enum values rather than immediately-invoked
callbacks. Turnbase ships Tier 2 in v1:

```rust
pub enum Effect { /* game-defined */ }

pub trait Trigger {
    fn condition(&self, event: &Effect) -> bool;
    fn resolve(&self, state: &mut State) -> Vec<Effect>;
}
```

Effects are queued, never invoked synchronously inline, so a future Tier 3 adds
a priority-passing loop and an SBA-recheck pass around the same queue rather
than replacing it.

## Action spaces: decision points, not flat turn enumeration

Diplomacy-scale games (34 units, ~5 orders each) don't enumerate the full turn
as a single flat action — that's combinatorially absurd. `legal_actions()` and
`apply()` are decision-point primitives: they answer "what can I do right now,"
and a full turn is a sequence of `apply()` calls until `active_players()`
changes. Tic-tac-toe's turns just happen to be length 1. This means the same
trait scales from tic-tac-toe to Star Wars Rebellion without a second, more
complex trait for "big" games.

The decision-point sequence within a phase is not required to be a fixed,
known-in-advance count either. Poker's betting rounds and Bridge's auction
re-visit the same player repeatedly and end only when a state-dependent
condition holds (all bets matched or folded; three consecutive passes) — not
after N steps. This falls out for free from `active_players`/`legal_actions`
being recomputed fresh after every `apply()` call rather than following a
pre-planned decomposition; it's called out here only because every example
above (Diplomacy: unit, then order) happens to have a fixed step count and
could otherwise be mistaken for a requirement.

### Decision points with large branching factors

Decomposition kills the *cross-product* explosion across a whole turn; it does
not guarantee every individual decision point is small. A decision point
returning a few thousand concrete actions is fine — `Vec<Action>` allocation
at that size is noise. Two specific shapes still need explicit handling:

1. **Combinatorial within one decision point** ("discard any subset of your
   15-card hand" is `2^15` subsets if modeled as one choice). Fix: decompose
   further, the same way a full turn gets decomposed — one binary
   include/exclude decision per card, 15 steps instead of one `2^15`-sized
   step.

2. **Large-but-structured spaces that resist decomposition** (targeting any
   tile on a 500-tile map). Two escape hatches, either or both:
   - *Hierarchical decomposition*: add a level — choose region (~20), then
     tile within region (~25) — turning one 500-way choice into two ~20-25-way
     choices.
   - *Decouple enumerate from validate*: `Game::is_legal` (see trait above)
     lets a game skip materializing the full legal set entirely. Exhaustive
     search (minimax/MCTS) needs `legal_actions()` to be complete; an RL
     policy network or a human UI only needs to validate the one action it
     already picked, via `is_legal()`. A game can provide a cheap `is_legal`
     without a faithful `legal_actions()` and simply document that
     exhaustive-search bots aren't supported at that specific decision point
     — the same way no one minimaxes over "which of 500 tiles to bombard" in
     practice either.

## Multi-phase "run" structures (composition, not a new trait)

Slay the Spire's map → combat → shop → rest loop (and similarly, tie-fighter-rl's
title → boon → map → briefing → combat → debrief) isn't one flat turn loop —
it's a phase machine where one phase (combat) is itself a full mini-game with
its own turns. This is exactly the friction pattern found scouting
tie-fighter-rl: a hardcoded `Phase` enum touched in `sync()`, `render()`, and
`handle_event()` every time a phase is added.

For v1, this is **documented convention, not a formal trait**: give the
top-level `State` a phase tag plus an inner state field per complex phase
(e.g. `combat: Option<CombatState>`), and have the top-level `Game` impl's
`active_players`/`legal_actions`/`apply`/`undo` dispatch into the inner phase's
logic (which can itself be a complete, separate `Game` implementation) when
the phase tag matches. This keeps combat-specific code from bloating run-level
code without inventing sub-game-hosting machinery before we've built a real
game that needs it. Revisit formalizing a `SubGame`-style helper once a
Slay-the-Spire-shaped reference game is actually attempted.

## Faction asymmetry: enum-of-enums for Action

Root-style games give every seat a genuinely different rule system (different
action sets, win conditions, sometimes different turn structure entirely).
The recommended convention is a flat `Action` enum with one variant per
faction, wrapping that faction's own action enum:

```rust
enum Action {
    Marquise(MarquiseAction),
    Eyrie(EyrieAction),
    WoodlandAlliance(AllianceAction),
    Vagabond(VagabondAction),
}
```

`legal_actions` for a given player dispatches to that faction's own
enumeration logic and wraps the result. This is ordinary Rust enum
composition, not new trait machinery — called out here only because deep
asymmetry makes it tempting to reach for something fancier than necessary.

## Scripted / automated participants

Blackjack's dealer, Slay the Spire's enemies, and Root's optional automa
factions all validated the same pattern with no changes needed: they're just
an entry in `active_players()` whose `legal_actions()` happens to always
return exactly one algorithmically-computed action (or a small fixed set,
for a scripted enemy choosing among a few intents). No bot, no special
"NPC" concept in the trait — "this seat isn't really a decision" falls out
for free from `legal_actions` returning a singleton.

## Workspace layout

```
turnbase/
  Cargo.toml                 # workspace root
  crates/
    core/                     # crate `turnbase`: Game trait, ActivePlayers, RNG, Tier-2 triggers
    bots/                     # crate `turnbase-bots`: RandomBot, minimax/alpha-beta, MCTS helpers
  examples/                   # crate `examples` (publish = false): reference games
    src/tic_tac_toe.rs        # reference game validating the trait design
```

Matches the `rg` house style: workspace-level lints (`clippy::all`/`pedantic`/
`nursery` deny), `resolver = "2"`, shared `[workspace.package]` metadata,
custom error enums instead of thiserror/anyhow, `mod tests` inline per file,
no unsafe, no async.

## Future: scripting / effect-authoring DSL

Deferred until 2-3 real games exist on the trait-based core and gaps are
concrete rather than speculated. Leading candidates, kept viable by design
(nothing in the core precludes them):

- **Data-driven effect vocabulary**: small set of composable verb enums
  (`DealDamage`, `Draw`, `ApplyStatus`) deserialized from YAML/JSON — the
  Hearthstone/Slay-the-Spire modding pattern. No embedded language needed.
- **Rhai**: pure-Rust, deterministic-friendly (no external float/RNG surprises),
  sandboxable, easiest to embed without FFI risk.
- **Lua (mlua)**: bigger ecosystem, but floating-point non-determinism across
  platforms is a real risk for replay-safety and needs mitigation.

The Tier-2 `Effect`/`Trigger` enums are the natural boundary where any of these
would plug in later — effects are already data, so a script's job is just to
*produce* `Effect` values rather than mutate state directly.

## Reference game: Tic-Tac-Toe

Chosen as the first implementation to validate the trait design before
committing to it further: simplest possible `active_players` (always a
singleton), simplest `legal_actions` (9 cells), no hidden information, no RNG,
no triggers. See `examples/tic_tac_toe.rs`.
