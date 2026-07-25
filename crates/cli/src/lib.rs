//! A generic command-line runner: give it any [`turnbase::Game`] and get a
//! headless CLI, bot self-play, and interactive play, with no per-game
//! plumbing.
//!
//! Two tiers:
//!
//! - [`run`] needs only a `Game` whose `State`/`Action`/`View` are
//!   serde-serializable. It provides `new`/`query`/`act` (headless, file-backed
//!   via [`turnbase_session::FileSession`]), `self-play` (bots), and a text
//!   `play`. Actions on the command line are JSON, so it works for any game.
//! - [`run_tui`] (the `tui` feature, on by default) additionally requires
//!   [`turnbase_simulator::PrintableGame`] and swaps the text `play` for the
//!   retroglyph dashboard. Everything else is identical.
//!
//! A game's `main` is then one line, e.g. `turnbase_cli::run(TicTacToe)` or
//! `turnbase_cli::run_tui(Coup::new(4))`.

use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde::de::DeserializeOwned;
use turnbase::{Game, PlayerId};
use turnbase_bots::Random;
use turnbase_match::{PlayerAgent, Simulator};
use turnbase_protocol::{Request, Response};
use turnbase_session::FileSession;

#[cfg(feature = "tui")]
use turnbase_simulator::{PrintableGame, SessionApp, standard_bots};

/// A guard against a game whose random-play match never terminates (uniform
/// random Risk, say), so `self-play` and `play` always return instead of
/// looping forever. Every reference game that does converge finishes far
/// inside this.
const STEP_LIMIT: usize = 10_000;

#[derive(Parser)]
#[command(name = "turnbase", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new session save file and print its path plus the opening view.
    New {
        /// Seed for the match. Chosen at random if omitted.
        #[arg(long)]
        seed: Option<u64>,
        /// Where to save. Generated under the temp directory if omitted.
        #[arg(long)]
        session: Option<PathBuf>,
    },
    /// Print a seat's current view. No side effects.
    Query {
        /// Path to an existing session.
        #[arg(long)]
        session: PathBuf,
        /// The seat asking.
        #[arg(long)]
        player: u32,
    },
    /// Submit one seat's action (encoded as JSON), then print the result.
    Act {
        /// Path to an existing session.
        #[arg(long)]
        session: PathBuf,
        /// The seat acting.
        #[arg(long)]
        player: u32,
        /// The action as JSON, e.g. `4` for tic-tac-toe or `{"Coup":2}` for Coup.
        #[arg(long)]
        action: String,
    },
    /// Play a full match with every seat driven by a bot; print the outcome.
    SelfPlay {
        /// Seed for the match. Chosen at random if omitted.
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Play interactively: you drive some seats, bots play the rest.
    Play(PlayArgs),
}

/// Arguments for the interactive `play` command, exposed so a game that passes
/// its own `play` handler to [`run_with_play`] can read them.
#[derive(Args)]
pub struct PlayArgs {
    /// Seed for the match. Chosen at random if omitted.
    #[arg(long)]
    seed: Option<u64>,
    /// Seats you control, comma-separated (e.g. `0` or `0,1`). Defaults to seat 0.
    #[arg(long)]
    manual: Option<String>,
}

impl PlayArgs {
    /// The seed requested on the command line, if any (otherwise the caller
    /// picks one, e.g. at random).
    #[must_use]
    pub const fn seed(&self) -> Option<u64> {
        self.seed
    }

    /// The seats the human asked to control, defaulting to seat 0.
    #[must_use]
    pub fn manual_seats(&self) -> Vec<u32> {
        parse_seats(self.manual.as_deref())
    }
}

/// Runs the text/headless CLI for `game`: `new`, `query`, `act`, `self-play`,
/// and a text `play`. Works for any game whose types are serde-serializable.
#[must_use]
pub fn run<G>(game: G) -> ExitCode
where
    G: Game + Serialize + DeserializeOwned,
    G::State: Serialize + DeserializeOwned,
    G::Action: DeserializeOwned + Debug,
    G::View: Serialize,
{
    run_with_play(game, text_play)
}

/// Like [`run`], but the game supplies its own interactive `play` handler (a
/// bespoke UI, say) rather than the built-in text stepper.
///
/// The headless `new`/`query`/`act` and `self-play` commands are handled here
/// exactly as [`run`] does; only the interactive `play` command is delegated
/// to `play`. This is how a game ships its own terminal UI (see
/// `examples/blackjack`) without reimplementing the rest of the CLI.
#[must_use]
pub fn run_with_play<G, F>(game: G, play: F) -> ExitCode
where
    G: Game + Serialize + DeserializeOwned,
    G::State: Serialize + DeserializeOwned,
    G::Action: DeserializeOwned + Debug,
    G::View: Serialize,
    F: FnOnce(G, &PlayArgs) -> ExitCode,
{
    match Cli::parse().command {
        Command::New { seed, session } => handle_new(game, seed, session),
        Command::Query { session, player } => handle_query::<G>(&session, player),
        Command::Act {
            session,
            player,
            action,
        } => handle_act::<G>(&session, player, &action),
        Command::SelfPlay { seed } => handle_self_play(game, seed),
        Command::Play(args) => play(game, &args),
    }
}

/// Like [`run`], but `play` opens the retroglyph dashboard instead of the text
/// stepper. Requires the game to implement [`PrintableGame`].
#[cfg(feature = "tui")]
#[must_use]
pub fn run_tui<G>(game: G) -> ExitCode
where
    G: PrintableGame + Clone + Serialize + DeserializeOwned,
    G::State: Clone + Serialize + DeserializeOwned,
    G::Action: Clone + DeserializeOwned + Debug,
    G::View: Serialize,
{
    run_with_play(game, tui_play)
}

fn handle_new<G>(game: G, seed: Option<u64>, session: Option<PathBuf>) -> ExitCode
where
    G: Game + Serialize + DeserializeOwned,
    G::State: Serialize + DeserializeOwned,
    G::View: Serialize,
{
    let seed = seed.unwrap_or_else(random_seed);
    let state = game.new_initial_state(seed);
    let path = match FileSession::create(game, session, state, seed) {
        Ok(path) => path,
        Err(err) => return fail(err),
    };
    // Echo the opening position from seat 0's view so `new` is self-explaining.
    match FileSession::handle::<G>(&path, PlayerId::new(0), Request::Query) {
        Ok(response) => {
            print_response(&path, &response);
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn handle_query<G>(session: &Path, player: u32) -> ExitCode
where
    G: Game + Serialize + DeserializeOwned,
    G::State: Serialize + DeserializeOwned,
    G::View: Serialize,
{
    match FileSession::handle::<G>(session, PlayerId::new(player), Request::Query) {
        Ok(response) => {
            print_response(session, &response);
            ExitCode::SUCCESS
        }
        Err(err) => fail(err),
    }
}

fn handle_act<G>(session: &Path, player: u32, action_json: &str) -> ExitCode
where
    G: Game + Serialize + DeserializeOwned,
    G::State: Serialize + DeserializeOwned,
    G::Action: DeserializeOwned,
    G::View: Serialize,
{
    let action: G::Action = match serde_json::from_str(action_json) {
        Ok(action) => action,
        Err(err) => return fail(format!("could not parse --action as JSON: {err}")),
    };
    let player = PlayerId::new(player);
    match FileSession::handle::<G>(session, player, Request::Act(action)) {
        // An accepted action returns no state; follow up with a query so the
        // caller sees the result of their move in one command.
        Ok(Response::Ack) => handle_query::<G>(session, player.index()),
        Ok(response @ Response::State { .. }) => {
            print_response(session, &response);
            ExitCode::SUCCESS
        }
        Ok(Response::Error(message)) => fail(format!("rejected: {message}")),
        Err(err) => fail(err),
    }
}

fn handle_self_play<G>(game: G, seed: Option<u64>) -> ExitCode
where
    G: Game,
    G::Action: Debug,
{
    let seed = seed.unwrap_or_else(random_seed);
    let agents = build_agents(&game, seed, &[]);
    let mut sim = Simulator::new(game, seed, agents);
    if let Err(err) = drive_to_end(&mut sim) {
        return fail(err);
    }
    print_outcome(&sim);
    ExitCode::SUCCESS
}

fn text_play<G>(game: G, args: &PlayArgs) -> ExitCode
where
    G: Game,
    G::Action: Debug,
    G::View: Serialize,
{
    let seed = args.seed.unwrap_or_else(random_seed);
    let manual = parse_seats(args.manual.as_deref());
    let agents = build_agents(&game, seed, &manual);
    let mut sim = Simulator::new(game, seed, agents);

    // Render from a single controlled seat's view (so hidden info stays hidden);
    // with zero or several controlled seats, render the public spectator view.
    // Multi-seat pass-and-play therefore does not reveal each acting seat's own
    // private data at its prompt; a hidden-info game wanting that needs the
    // dashboard (`run_tui`), which fixes one viewing seat per match.
    let viewer = match manual.as_slice() {
        [only] => Some(PlayerId::new(*only)),
        _ => None,
    };

    println!("# seed {seed}, you control {manual:?}");
    render_view(&sim, viewer);
    let mut steps = 0;
    while !sim.is_terminal() && steps < STEP_LIMIT {
        steps += 1;
        match sim.awaiting_human() {
            Some(player) => {
                let mut actions = sim.game().legal_actions(sim.state(), player);
                if actions.is_empty() {
                    break;
                }
                let choice = prompt_choice(player, &actions);
                let action = actions.swap_remove(choice);
                if let Err(err) = sim.select_human_action(player, action) {
                    return fail(err);
                }
            }
            None => match sim.step() {
                Ok(true) => {}
                Ok(false) => break,
                Err(err) => return fail(err),
            },
        }
        render_view(&sim, viewer);
    }
    print_outcome(&sim);
    ExitCode::SUCCESS
}

#[cfg(feature = "tui")]
fn tui_play<G>(game: G, args: &PlayArgs) -> ExitCode
where
    G: PrintableGame + Clone,
    G::State: Clone,
    G::Action: Clone + Debug,
{
    let seed = args.seed.unwrap_or_else(random_seed);
    // Open the setup modal so the player can pick Human/AI per seat, pre-seeding
    // the seats they named with --manual as human.
    let mut app = SessionApp::new(game, standard_bots(), seed).with_setup_open(true);
    for seat in parse_seats(args.manual.as_deref()) {
        if let Ok(index) = usize::try_from(seat) {
            app = app.with_human_seat(index);
        }
    }
    match turnbase_simulator::run_session(app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => fail(err),
    }
}

/// Builds one agent per seat: [`PlayerAgent::Human`] for a seat in `manual`,
/// otherwise a per-seat-seeded [`Random`]. `manual` empty means all bots.
fn build_agents<G: Game>(game: &G, seed: u64, manual: &[u32]) -> HashMap<PlayerId, PlayerAgent<G>> {
    let mut agents = HashMap::new();
    for seat in 0..game.num_players() {
        let id = PlayerId::new(u32::try_from(seat).expect("seat index fits in u32"));
        let agent = if manual.contains(&id.index()) {
            PlayerAgent::Human
        } else {
            PlayerAgent::Ai(Box::new(Random::new(seat_seed(seed, id.index()))))
        };
        agents.insert(id, agent);
    }
    agents
}

/// Steps an all-bot (plus chance) match to its end, or the step guard.
fn drive_to_end<G>(sim: &mut Simulator<G>) -> Result<(), turnbase::Error>
where
    G: Game,
    G::Action: Debug,
{
    let mut steps = 0;
    while !sim.is_terminal() && steps < STEP_LIMIT {
        // Ok(false) at a non-terminal state means a seat is waiting on a human,
        // or a bot declined (no legal action). Neither should happen in an
        // all-bot match, so log and stop rather than spin.
        if !sim.step()? {
            log::debug!("self-play stalled at a non-terminal state; stopping");
            break;
        }
        steps += 1;
    }
    Ok(())
}

/// Prints the JSON view for `viewer` on one line.
fn render_view<G>(sim: &Simulator<G>, viewer: Option<PlayerId>)
where
    G: Game,
    G::View: Serialize,
{
    let view = sim.game().view(sim.state(), viewer);
    match serde_json::to_string(&view) {
        Ok(json) => println!("  {json}"),
        Err(err) => eprintln!("could not encode view: {err}"),
    }
}

/// Prints each seat's terminal reward and the move count, or an honest note if
/// the match hit the step guard without finishing (rewards are only meaningful
/// at a terminal state).
fn print_outcome<G: Game>(sim: &Simulator<G>) {
    if !sim.is_terminal() {
        println!(
            "=== unfinished after {} move(s) (hit the step limit) ===",
            sim.log_history().len()
        );
        return;
    }
    let game = sim.game();
    let rewards: Vec<String> = (0..game.num_players())
        .map(|seat| {
            let id = PlayerId::new(u32::try_from(seat).expect("seat index fits in u32"));
            format!("P{seat}={:+}", game.reward(sim.state(), id))
        })
        .collect();
    println!(
        "=== terminal after {} move(s) === {}",
        sim.log_history().len(),
        rewards.join("  ")
    );
}

/// Prompts a human to pick one of `actions` by index, returning that index.
/// A forced single choice is taken automatically.
fn prompt_choice<A: Debug>(player: PlayerId, actions: &[A]) -> usize {
    if actions.len() == 1 {
        return 0;
    }
    loop {
        print!("P{} choose ", player.index());
        for (index, action) in actions.iter().enumerate() {
            print!("[{index}] {action:?}  ");
        }
        print!("> ");
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        let Ok(bytes) = std::io::stdin().read_line(&mut line) else {
            continue;
        };
        // A zero-byte read is end-of-input (piped or closed stdin); take the
        // first choice rather than looping forever on an empty line.
        if bytes == 0 {
            return 0;
        }
        match line.trim().parse::<usize>() {
            Ok(index) if index < actions.len() => return index,
            _ => println!("  not a valid choice"),
        }
    }
}

/// Prints a `{ session, response }` JSON envelope.
fn print_response<V: Serialize>(session: &Path, response: &Response<V>) {
    #[derive(Serialize)]
    struct Envelope<'a, V> {
        session: &'a Path,
        response: &'a Response<V>,
    }
    let envelope = Envelope { session, response };
    match serde_json::to_string_pretty(&envelope) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("could not encode output: {err}"),
    }
}

/// Parses `--manual` ("0" or "0,1"), defaulting to seat 0 when absent or empty.
fn parse_seats(manual: Option<&str>) -> Vec<u32> {
    let Some(raw) = manual else {
        return vec![0];
    };
    let seats: Vec<u32> = raw
        .split(',')
        .filter_map(|token| token.trim().parse().ok())
        .collect();
    if seats.is_empty() { vec![0] } else { seats }
}

/// Mixes a per-seat bot seed off the match seed, so two bot seats do not share
/// a random stream.
fn seat_seed(seed: u64, seat: u32) -> u64 {
    seed ^ u64::from(seat)
        .wrapping_add(1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// A process-random seed, so an unseeded match differs from run to run.
fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

fn fail(err: impl Display) -> ExitCode {
    eprintln!("error: {err}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{parse_seats, seat_seed};

    #[test]
    fn parse_seats_defaults_to_seat_zero() {
        assert_eq!(parse_seats(None), vec![0]);
        assert_eq!(parse_seats(Some("")), vec![0]);
        assert_eq!(parse_seats(Some("nonsense")), vec![0]);
    }

    #[test]
    fn parse_seats_reads_a_comma_list() {
        assert_eq!(parse_seats(Some("0")), vec![0]);
        assert_eq!(parse_seats(Some("0,2")), vec![0, 2]);
        assert_eq!(parse_seats(Some(" 1 , 3 ")), vec![1, 3]);
    }

    #[test]
    fn seat_seed_is_stable_and_distinct_per_seat() {
        assert_eq!(seat_seed(42, 0), seat_seed(42, 0), "same inputs, same seed");
        assert_ne!(
            seat_seed(42, 0),
            seat_seed(42, 1),
            "different seats get different streams"
        );
    }
}
