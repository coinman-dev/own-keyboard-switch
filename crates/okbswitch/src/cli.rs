//! Command-line interface.

use clap::Parser;
use std::path::PathBuf;

/// Own Keyboard Switch: automatic RU/EN keyboard layout switcher.
#[derive(Debug, Parser)]
#[command(name = "okbswitch", version, about, long_about = None)]
pub struct Cli {
    /// Use another configuration file inside the program directory.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Log at debug level and mirror the log to the terminal.
    #[arg(short, long)]
    pub debug: bool,

    /// Run without the tray icon (stop with Ctrl+C).
    #[arg(long)]
    pub no_tray: bool,

    /// Open the settings window right after start.
    #[arg(long)]
    pub settings: bool,

    /// Wait for the previous instance to exit (used when restarting with
    /// administrator rights).
    #[arg(long, hide = true)]
    pub restarting: bool,

    /// Check the environment (session, layouts, device access) and exit.
    #[arg(long, conflicts_with_all = ["print_default_config", "paths"])]
    pub diagnose: bool,

    /// Print the default configuration file and exit.
    #[arg(long, conflicts_with = "paths")]
    pub print_default_config: bool,

    /// Print the locations of configuration, log and lock files and exit.
    #[arg(long)]
    pub paths: bool,

    /// Print the program license and embedded language-data notices and exit.
    #[arg(long)]
    pub licenses: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_flags() {
        let cli = Cli::parse_from(["okbswitch", "-d", "--config", "/tmp/c.toml"]);
        assert!(cli.debug);
        assert!(!cli.restarting);
        assert!(Cli::parse_from(["okbswitch", "--restarting"]).restarting);
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/c.toml")));
        assert!(Cli::try_parse_from(["okbswitch", "--diagnose", "--paths"]).is_err());
    }
}
