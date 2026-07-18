//! Games shared by this crate's `interactive_*`/`headless_ui` examples.
//!
//! Kept out of the library itself (and out of `examples/*.rs` proper, since
//! `impl PrintableGame for Coup` would violate the orphan rule from an
//! example binary — neither the trait nor `examples::Coup` are local to it)
//! and included via `#[path]` instead.
//!
//! `Coup` reuses the workspace's own tested implementation from the
//! `examples` crate rather than reimplementing its rules here, through a
//! thin newtype (`DemoCoup`) that delegates every [`Game`] method to the
//! wrapped type. That newtype exists only to satisfy the orphan rule locally
//! — putting a `PrintableGame` impl directly on `examples::Coup` would mean
//! giving `examples` (a UI-free reference-games crate depended on by
//! `turnbase-bots`) an optional dependency on this crate's `retroglyph`
//! stack, which is exactly the "UI-free core" boundary `ARCHITECTURE.md`
//! draws for the engine.
#![allow(dead_code)] // Each including example only uses part of this surface.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase::{ActivePlayers, Determinize, Game, PlayerId, Prng};
use turnbase_bots::{Bot, Ismcts, RandomBot};
use turnbase_simulator::PrintableGame;

/// A process-random `u64`, for seeding an interactive session (the initial
/// deal, a bot's decisions) so it differs from run to run rather than
/// dealing the same hand every time.
///
/// Built on `RandomState` rather than pulling in the `rand` crate: a fresh
/// `RandomState` is keyed from OS entropy on construction, and hashing
/// nothing with it still yields a value derived from that random key. Two
/// calls are independent (each constructs its own `RandomState`), so it is
/// fine to call this once for the game seed and again for a bot's seed.
pub fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

// ---------------------------------------------------------------------------
// CountToTen: the simplest demo game, defined here rather than reused (there
// is nothing to reuse it from).
// ---------------------------------------------------------------------------

/// Two seats alternately add 1..=3 to a shared total; whoever reaches 10 wins.
#[derive(Clone, Copy, Default)]
pub struct CountToTen;

/// Add `0` (amount 1) through `2` (amount 3) to the running total.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Add(pub u32);

/// The running total plus how many additions have been made.
///
/// `moves` tracks whose turn it is; `total`'s parity cannot be used for that
/// (as an earlier version of this demo did), since an amount of 2 leaves
/// parity unchanged and would let the same seat move twice in a row.
///
/// Also doubles as [`Game::View`]: `CountToTen` has no hidden information, so
/// there is nothing a view needs to redact.
#[derive(Clone, Copy, Default)]
pub struct Position {
    total: u32,
    moves: u32,
}

impl Game for CountToTen {
    type State = Position;
    type Action = Add;
    type View = Position;

    fn new_initial_state(&self, _seed: u64) -> Self::State {
        Position::default()
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::new(state.moves % 2))
        }
    }

    fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) {
            Vec::new()
        } else {
            vec![Add(0), Add(1), Add(2)]
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        state.total += action.0 + 1;
        state.moves += 1;
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.total >= 10
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        // The seat that just moved (the previous mover) is the one who
        // crossed 10.
        let winner = (state.moves + 1) % 2;
        if player.index() == winner { 1.0 } else { -1.0 }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        *state
    }
}

impl PrintableGame for CountToTen {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        term.print(area.left(), area.top(), &format!("total: {}", view.total));
        term.print(area.left(), area.top() + 2, "first to 10 (or past it) wins");
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            ("total".to_owned(), view.total.to_string()),
            ("seat to move".to_owned(), (view.moves % 2).to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        format!("add {}", action.0 + 1)
    }
}

// ---------------------------------------------------------------------------
// Coup: reused from `examples::coup`. Hidden information (each seat's
// influence cards), so this is the demo that actually exercises the switch
// from `&Self::State` to `&Self::View`: `CoupView::own_hand` is only
// populated for the seat `SimulationRunner` renders from (see
// `Simulator::primary_human`), so the AI's hand never reaches the screen.
//
// `CoupView::pending` (added specifically for this dashboard) is what makes
// the UI legible: Coup's real complexity is its response windows ("p0
// claims Tax, do you challenge or block?"), and without exposing what is
// actually being claimed, a UI can only show a bare Pass/Challenge/Block
// menu with no context for the decision.
// ---------------------------------------------------------------------------

/// Wraps [`examples::Coup`] with a local [`PrintableGame`] impl.
#[derive(Clone, Copy)]
pub struct DemoCoup(pub examples::Coup);

impl Default for DemoCoup {
    fn default() -> Self {
        Self(examples::Coup::new(2))
    }
}

impl Game for DemoCoup {
    type State = <examples::Coup as Game>::State;
    type Action = <examples::Coup as Game>::Action;
    type View = <examples::Coup as Game>::View;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        self.0.new_initial_state(seed)
    }
    fn num_players(&self) -> usize {
        self.0.num_players()
    }
    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        self.0.active_players(state)
    }
    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        self.0.legal_actions(state, player)
    }
    fn is_legal(&self, state: &Self::State, player: PlayerId, action: &Self::Action) -> bool {
        self.0.is_legal(state, player, action)
    }
    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
        self.0.apply(state, player, action);
    }
    fn is_terminal(&self, state: &Self::State) -> bool {
        self.0.is_terminal(state)
    }
    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        self.0.reward(state, player)
    }
    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        self.0.view(state, viewer)
    }
}

impl Determinize for DemoCoup {
    fn determinize(&self, state: &Self::State, observer: PlayerId, rng: &mut Prng) -> Self::State {
        self.0.determinize(state, observer, rng)
    }
}

/// How strong an AI seat plays, from a menu a demo's `main` can offer on the
/// command line.
///
/// [`RandomBot`] and [`Ismcts`] are the only bots in `turnbase-bots` fit for
/// this: `Minimax`/`Mcts` are documented as two-player-zero-sum and search
/// the *true* state, which would mean an AI opponent that cheats by seeing
/// through Coup's hidden hands. [`Ismcts`] never does that -- every
/// simulation resamples a fresh world consistent with what the bot's seat
/// can actually see (`Determinize`, implemented above) -- and its `max^n`
/// backup handles the 3-4 player case, not just two-player zero-sum. Higher
/// iteration counts stay well under a frame's budget even in a debug build
/// (~25ms/move at Hard with 4 seats, measured), so difficulty scales with no
/// visible UI lag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    /// Uniform-random legal moves: no bluffing sense, no read on challenges.
    Easy,
    /// ISMCTS, 150 simulations/move: makes sane bets, still exploitable.
    Medium,
    /// ISMCTS, 800 simulations/move: a genuinely careful opponent.
    Hard,
}

impl Difficulty {
    /// Parses a `--difficulty` value (case-insensitive), or `None` for an
    /// unrecognized one.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "easy" => Some(Self::Easy),
            "medium" => Some(Self::Medium),
            "hard" => Some(Self::Hard),
            _ => None,
        }
    }

    /// Builds a fresh bot at this difficulty, seeded from `seed`.
    #[must_use]
    pub fn bot(self, seed: u64) -> Box<dyn Bot<DemoCoup>> {
        match self {
            Self::Easy => Box::new(RandomBot::new(seed)),
            Self::Medium => Box::new(Ismcts::new(150, seed)),
            Self::Hard => Box::new(Ismcts::new(800, seed)),
        }
    }
}

impl PrintableGame for DemoCoup {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Coup ==");
        y = y.saturating_add(2);

        // The pending description can run well past a narrow terminal's
        // viewport width ("p0 declares Foreign Aid -- p1 may pass,
        // challenge, or block" is 58 columns), so wrap it to `area`'s actual
        // width rather than letting it bleed into the stats panel.
        term.fg(Color::Ansi(AnsiColor::BrightYellow));
        for line in wrap(&describe_pending(&view.pending), area.width()) {
            term.print(area.left(), y, &line);
            y = y.saturating_add(1);
        }
        term.reset_style();
        y = y.saturating_add(1);

        let deciding = active_seat(&view.pending);
        for (seat, &coins) in view.coins.iter().enumerate() {
            let marker = if seat == deciding { '>' } else { ' ' };
            if seat == deciding {
                term.fg(Color::Ansi(AnsiColor::BrightGreen));
            }
            let lost = describe_cards(&view.lost[seat]);
            term.print(
                area.left(),
                y,
                &format!(
                    "{marker} p{seat}: {coins} coins, {} influence (lost: {lost})",
                    view.influence[seat]
                ),
            );
            term.reset_style();
            y = y.saturating_add(1);
        }

        y = y.saturating_add(1);
        term.print(
            area.left(),
            y,
            &format!("deck: {} cards left", view.deck_size),
        );
        y = y.saturating_add(1);

        // During your own exchange your hand is briefly empty (its cards
        // moved into the pool), so show the pool -- with the indices
        // `Action::Return` needs -- instead of an empty "your hand" line.
        if let examples::coup::PendingView::ExchangeReturn { pool, .. } = &view.pending
            && !pool.is_empty()
        {
            term.print(
                area.left(),
                y,
                &format!("your exchange pool: {}", describe_indexed(pool)),
            );
        } else {
            term.print(
                area.left(),
                y,
                &format!("your hand: {}", describe_indexed(&view.own_hand)),
            );
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        // Keep every value short: this panel is narrow enough that a longer
        // line (a full pending description, a multi-card hand) wraps into
        // the row below and corrupts whatever's printed there. The full
        // narrative lives in the (wider) viewport instead.
        let mut stats: Vec<(String, String)> = view
            .coins
            .iter()
            .enumerate()
            .map(|(seat, &coins)| (format!("p{seat} coins"), coins.to_string()))
            .collect();
        stats.push(("deck".to_owned(), view.deck_size.to_string()));
        stats.push(("phase".to_owned(), phase_tag(&view.pending).to_owned()));
        stats
    }

    fn format_action(&self, action: &Self::Action) -> String {
        // Kept short (the actions panel is a narrow column, typically
        // 20-something columns wide) rather than fully spelled out; the
        // wider viewport's pending description carries the longer
        // narrative, and `print_rows` clips anything that still overflows.
        use examples::coup::Action as CoupAction;
        match action {
            CoupAction::Income => "income (+1)".to_owned(),
            CoupAction::ForeignAid => "foreign aid (+2)".to_owned(),
            CoupAction::Coup(target) => format!("coup p{target}"),
            CoupAction::Tax => "tax (Duke, +3)".to_owned(),
            CoupAction::Assassinate(target) => format!("assassinate p{target} (Assassin)"),
            CoupAction::Steal(target) => format!("steal p{target} (Captain)"),
            CoupAction::Exchange => "exchange (Ambassador)".to_owned(),
            CoupAction::Return(index) => format!("return card {index}"),
            CoupAction::Pass => "pass".to_owned(),
            CoupAction::Challenge => "challenge".to_owned(),
            CoupAction::Block(character) => format!("block ({character:?})"),
            CoupAction::Lose(index) => format!("discard card {index}"),
        }
    }
}

/// The seat [`examples::coup::PendingView`] says is currently owed a
/// decision, as a `usize` ready to compare against an `enumerate()` index.
fn active_seat(pending: &examples::coup::PendingView) -> usize {
    use examples::coup::PendingView as P;
    let seat = match pending {
        P::ChooseAction { actor } => *actor,
        P::Respond { responder, .. } | P::RespondToBlock { responder, .. } => *responder,
        P::Lose { who } => *who,
        P::ExchangeReturn { player, .. } => *player,
        P::GameOver => return usize::MAX, // no seat is deciding anything
    };
    usize::from(seat)
}

/// A one-line narrative of the current decision point, for the viewport.
fn describe_pending(pending: &examples::coup::PendingView) -> String {
    use examples::coup::PendingView as P;
    match pending {
        P::ChooseAction { actor } => format!("p{actor}'s turn: choose an action"),
        P::Respond {
            actor,
            action,
            claim,
            responder,
        } => {
            let claim = claim.map(|c| format!(", claims {c:?}")).unwrap_or_default();
            let can_block = matches!(
                action,
                examples::coup::Action::ForeignAid
                    | examples::coup::Action::Assassinate(_)
                    | examples::coup::Action::Steal(_)
            );
            let options = if can_block {
                "pass, challenge, or block"
            } else {
                "pass or challenge"
            };
            format!(
                "p{actor} declares {}{claim} -- p{responder} may {options}",
                declared_action_name(*action)
            )
        }
        P::RespondToBlock {
            actor,
            blocker,
            block_as,
            responder,
            ..
        } => format!(
            "p{blocker} blocks p{actor}, claims {block_as:?} -- p{responder} may pass or challenge"
        ),
        P::Lose { who } => format!("p{who} must reveal and discard an influence card"),
        P::ExchangeReturn {
            player,
            pool,
            returns_left,
        } => {
            if pool.is_empty() {
                // Redacted: it's not the viewer's own exchange, so the pool
                // never got populated (see `PendingView::ExchangeReturn`).
                format!("p{player} is exchanging: choosing which cards to keep")
            } else {
                format!(
                    "p{player} is exchanging: return {returns_left} more of {}",
                    describe_indexed(pool)
                )
            }
        }
        P::GameOver => "match over".to_owned(),
    }
}

/// Short, fixed-width tag for the stats panel's "phase" row.
const fn phase_tag(pending: &examples::coup::PendingView) -> &'static str {
    use examples::coup::PendingView as P;
    match pending {
        P::ChooseAction { .. } => "choose",
        P::Respond { .. } => "respond",
        P::RespondToBlock { .. } => "block?",
        P::Lose { .. } => "lose",
        P::ExchangeReturn { .. } => "exchange",
        P::GameOver => "over",
    }
}

/// The claim-worthy name of a declared action (the ones a response window
/// can open on), for [`describe_pending`].
const fn declared_action_name(action: examples::coup::Action) -> &'static str {
    match action {
        examples::coup::Action::ForeignAid => "Foreign Aid",
        examples::coup::Action::Tax => "Tax",
        examples::coup::Action::Assassinate(_) => "Assassinate",
        examples::coup::Action::Steal(_) => "Steal",
        examples::coup::Action::Exchange => "Exchange",
        _ => "an action",
    }
}

/// Greedily word-wraps `text` to at most `width` columns per line.
///
/// [`PrintableGame::draw_viewport`] implementations are expected to stay
/// within the `area` they are handed; `Terminal::print` itself only wraps at
/// the *grid's* width, not an arbitrary rect's, so a status line built from
/// runtime data (a claimed action, a seat number) needs its own wrap rather
/// than risking a bleed into whatever panel sits to the right.
fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width);
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let extra = usize::from(!line.is_empty());
        if !line.is_empty() && line.len() + extra + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Renders a hand/pool as `"0=Duke, 1=Captain"`, or `"none"` when empty (a
/// redacted opponent hand, or a seat with no influence left). Indexed to
/// match what `Action::Lose`/`Action::Return` expect.
fn describe_indexed(cards: &[examples::coup::Character]) -> String {
    if cards.is_empty() {
        return "none".to_owned();
    }
    cards
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{i}={c:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders a revealed-card list as `"Duke, Captain"` (no indices: revealed
/// cards are not addressable by any action), or `"none"` when empty.
fn describe_cards(cards: &[examples::coup::Character]) -> String {
    if cards.is_empty() {
        return "none".to_owned();
    }
    cards
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}
