#[macro_use]
extern crate log;

mod event_dispatcher;
mod event_handler;
mod types;

use event_dispatcher::EventDispatcher;
use event_handler::EventHandler;

use clap::Parser;
use serde::{Deserialize, Serialize};

use std::fs::File;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

#[derive(Parser, Debug)]
#[clap(version = "0.1.1", author = "Joe K. <joe.kaushal@gmail.com>")]
struct Opts {
    #[clap(short, long, default_value = "config.ron")]
    config: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Config {
    event_dispatcher: event_dispatcher::Config,
    event_handler: event_handler::Config,
}

fn load_config<P: AsRef<Path>>(path: P) -> Config {
    let path_str = path.as_ref().to_string_lossy();

    match File::open(&path).map(ron::de::from_reader) {
        Ok(Ok(config)) => {
            info!("loaded config from \"{}\"", path_str);
            return config;
        }

        Err(error) => error!("could open config \"{}\": {}", path_str, error),
        Ok(Err(error)) => error!("could not process config \"{}\": {}", path_str, error),
    }

    error!("using default config");
    Config::default()
}

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    info!("rlm2c 0.1.1 - github.com/bozbez/rlm2c");

    let opts: Opts = Opts::parse();

    let Config {
        event_dispatcher: event_dispatcher_config,
        event_handler: event_handler_config,
    } = load_config(opts.config);

    let (tx, rx) = mpsc::channel();

    let event_handler_thread = thread::spawn(|| {
        match EventHandler::new(rx, event_handler_config) {
            Ok(mut event_handler) => match event_handler.run() {
                Ok(()) => {}
                Err(error) => error!("could not run event handler: {}", error),
            },

            Err(error) => error!("could not create event handler: {}", error),
        };
    });

    match EventDispatcher::new(tx, event_dispatcher_config) {
        Some(mut event_dispatcher) => event_dispatcher.run(),
        None => error!("could not create event dispatcher"),
    };

    event_handler_thread.join().unwrap();
}
