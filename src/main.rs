//! `facturacion`: opens the desktop window. All logic lives in the `tessia`
//! library.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use tessia::config::Settings;
use tessia::context::AppContext;
use tessia::shutdown;
use tracing_subscriber::EnvFilter;

/// Exit status of a usage error (an argument the window does not take).
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    init_tracing();
    let config = match config_file(std::env::args_os().skip(1)) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    shutdown::install();
    match run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// The only thing a shortcut can pass: `--config <file>` (also `TESSIA_CONFIG`).
/// A leading `desktop` is accepted so older shortcuts keep working.
fn config_file(args: impl Iterator<Item = OsString>) -> Result<Option<PathBuf>, String> {
    let mut args = args.peekable();
    if args.peek().is_some_and(|first| first == "desktop") {
        args.next();
    }
    let mut config = None;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            let file = args
                .next()
                .ok_or("falta el archivo después de --config <ARCHIVO>")?;
            config = Some(PathBuf::from(file));
        } else {
            return Err(format!(
                "argumento no admitido: {}. Uso: facturacion [--config <ARCHIVO>]",
                arg.to_string_lossy()
            ));
        }
    }
    Ok(config)
}

fn run(config: Option<PathBuf>) -> Result<()> {
    let settings = Settings::load(config.as_deref())?;
    tessia::desktop::run(AppContext::from_settings(settings)?)
}

/// Diagnostics go to stderr; the level comes from `RUST_LOG` (`warn` by
/// default).
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(list: &[&str]) -> Result<Option<PathBuf>, String> {
        config_file(list.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_open_the_window_with_the_default_settings() {
        assert_eq!(parse(&[]), Ok(None));
    }

    #[test]
    fn a_shortcut_can_point_the_window_at_a_settings_file() {
        assert_eq!(
            parse(&["--config", "tessia.env"]),
            Ok(Some(PathBuf::from("tessia.env")))
        );
        assert_eq!(
            parse(&["desktop", "--config", "tessia.env"]),
            Ok(Some(PathBuf::from("tessia.env")))
        );
    }

    #[test]
    fn anything_else_is_a_usage_error_not_a_silent_window() {
        assert!(parse(&["db", "migrate"]).is_err());
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["--help"]).is_err());
    }
}
