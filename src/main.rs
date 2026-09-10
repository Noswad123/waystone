mod app;
mod commands;
mod picker;
mod process;
mod record;
mod time;

use std::env;
use std::error::Error;

use app::App;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn real_main() -> Result<()> {
    let app = App::from_env()?;
    app.init_state()?;
    let args: Vec<String> = env::args().skip(1).collect();
    app.dispatch(&args)
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
