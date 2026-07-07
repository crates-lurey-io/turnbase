//! A tiny generic CLI stepper for any Turnbase game.
//!
//! One line per step. Seats you control are played from stdin (from your own
//! view — you never see an opponent's hidden cards); the rest are played by a
//! random bot. Chance nodes are auto-sampled.
//!
//! Usage:
//!   cargo run -p turnbase-bots --example play -- <game> [--seed N] [--manual [seats]] [--step]
//!   games:   coup | ttt | rps | highcard | minions
//!   --manual with no value puts you in seat 1; --manual 0 or --manual 0,1 picks seats.
//!
//! This is a dev tool, so pedantic/nursery ergonomics (unwrap, casts, I/O) are
//! not worth fighting here.
#![allow(clippy::pedantic, clippy::nursery)]

use std::fmt::Debug;
use std::io::{self, Write};

use examples::coup::CoupState;
use examples::{Coup, HighCard, MinionBattle, RockPaperScissors, TicTacToe};
use turnbase::{Game, PlayerId, Prng, sample_chance};
use turnbase_bots::{Bot, RandomBot};

struct Options {
    seed: u64,
    manual: Vec<u32>,
    step: bool,
    players: u8,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let game = args.get(1).map_or("coup", String::as_str);
    let opts = parse_options(&args);

    // A single human seat renders from that seat's view; otherwise a god view.
    let viewer = (opts.manual.len() == 1).then(|| PlayerId::new(opts.manual[0]));

    match game {
        "coup" => run(&Coup::new(opts.players), &opts, viewer, render_coup),
        "ttt" => run(&TicTacToe, &opts, viewer, |s, _| one_line(&format!("{s}"))),
        "rps" => run(&RockPaperScissors, &opts, viewer, |s, _| format!("{s:?}")),
        "highcard" => run(&HighCard::default(), &opts, viewer, |s, _| format!("{s:?}")),
        "minions" => run(&MinionBattle, &opts, viewer, |s, _| format!("{s:?}")),
        other => eprintln!("unknown game: {other} (try: coup | ttt | rps | highcard | minions)"),
    }
}

fn parse_options(args: &[String]) -> Options {
    let mut opts = Options {
        seed: 1,
        manual: Vec::new(),
        step: false,
        players: 2,
    };
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                i += 1;
                opts.seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
            "--manual" => match args.get(i + 1) {
                Some(value) if !value.starts_with("--") => {
                    opts.manual = value.split(',').filter_map(|x| x.parse().ok()).collect();
                    if opts.manual.is_empty() {
                        opts.manual = vec![1];
                    }
                    i += 1;
                }
                _ => opts.manual = vec![1],
            },
            "--players" => {
                i += 1;
                opts.players = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2)
                    .clamp(2, 4);
            }
            "--step" => opts.step = true,
            _ => {}
        }
        i += 1;
    }
    opts
}

fn run<G, R>(game: &G, opts: &Options, viewer: Option<PlayerId>, mut summary: R)
where
    G: Game,
    G::Action: Debug + Clone,
    R: FnMut(&G::State, Option<PlayerId>) -> String,
{
    let mut state = game.new_initial_state(opts.seed);
    let mut chance_rng = Prng::new(opts.seed ^ 0x00C0_FFEE);
    let mut bot = RandomBot::new(opts.seed ^ 0x00BE_EF00);

    let you = opts
        .manual
        .iter()
        .map(|s| format!("P{s}"))
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "# seed {}  you={}",
        opts.seed,
        if you.is_empty() { "(none)" } else { &you }
    );
    println!("    {}", summary(&state, viewer));

    let mut step = 0;
    while !game.is_terminal(&state) {
        let Some(player) = game.active_players(&state).iter().next() else {
            break;
        };
        step += 1;

        let (label, action) = if player.is_chance() {
            let sampled = sample_chance(game, &state, &mut chance_rng).expect("a chance outcome");
            ("~chance".to_string(), sampled)
        } else {
            let actions = game.legal_actions(&state, player);
            if actions.is_empty() {
                break;
            }
            if opts.manual.contains(&player.index()) {
                (format!("P{}", player.index()), prompt(player, &actions))
            } else {
                let chosen = bot.choose(game, &state, player).expect("a legal action");
                if opts.step {
                    wait_for_enter();
                }
                (format!("P{}", player.index()), chosen)
            }
        };

        game.apply(&mut state, player, action.clone());
        println!(
            "{step:>3} {label:>7} {:<16} {}",
            format!("{action:?}"),
            summary(&state, viewer),
        );

        if step > 5_000 {
            println!("(step limit reached)");
            break;
        }
    }

    let rewards = (0..game.num_players())
        .map(|s| format!("P{s} {:+}", game.reward(&state, PlayerId::new(s as u32))))
        .collect::<Vec<_>>()
        .join("  ");
    println!("=== terminal ===  {rewards}");
}

fn prompt<A: Debug + Clone>(player: PlayerId, actions: &[A]) -> A {
    if actions.len() == 1 {
        return actions[0].clone();
    }
    loop {
        print!("P{} > choose ", player.index());
        for (i, action) in actions.iter().enumerate() {
            print!("[{i}]{action:?} ");
        }
        print!(": ");
        io::stdout().flush().ok();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            continue;
        }
        match line.trim().parse::<usize>() {
            Ok(index) if index < actions.len() => return actions[index].clone(),
            _ => println!("  invalid choice"),
        }
    }
}

fn wait_for_enter() {
    print!("  [enter] ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line).ok();
}

fn one_line(text: &str) -> String {
    text.replace('\n', " / ")
}

/// Compact Coup line from `viewer`'s perspective: your own cards are shown; a
/// hidden opponent's influence is shown as dots, plus any revealed cards.
fn render_coup(state: &CoupState, viewer: Option<PlayerId>) -> String {
    let mut seats = Vec::new();
    for seat in 0..state.seats() as usize {
        let known = viewer.is_none() || viewer.map(|v| v.index() as usize) == Some(seat);
        let hand = if known {
            format!("{:?}", state.hand(seat))
        } else {
            "●".repeat(state.influence(seat))
        };
        let lost = state.lost(seat);
        let revealed = if lost.is_empty() {
            String::new()
        } else {
            format!(" x{lost:?}")
        };
        seats.push(format!("P{seat} {}c {hand}{revealed}", state.coins(seat)));
    }
    let turn = if state.is_over() {
        "OVER".to_string()
    } else {
        format!("P{}", state.current())
    };
    format!("{}  | {turn}", seats.join("  "))
}
