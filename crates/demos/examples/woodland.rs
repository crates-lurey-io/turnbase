//! Browser demo entry for Woodland: an all-bot match on the shared dashboard.

use turnbase_demos::{Demo, dashboard, demo_entry};
use turnbase_simulator::{SessionApp, standard_bots};
use woodland::Woodland;

#[derive(Default)]
struct WoodlandDemo;

impl Demo for WoodlandDemo {
    type App = SessionApp<Woodland>;

    fn build(seed: u64) -> Self::App {
        dashboard(Woodland, standard_bots(), seed)
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(WoodlandDemo);
