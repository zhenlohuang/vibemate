use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::config::{DEFAULT_FULL_CONFIG, expand_tilde};
use crate::error::Result;

pub fn run(init: bool, config_path: &Path) -> Result<()> {
    let resolved_path = expand_tilde(config_path);

    if init {
        if let Some(parent) = resolved_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if resolved_path.exists() {
            print!(
                "Config already exists at {}. Overwrite? [y/N] ",
                resolved_path.display()
            );
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let confirmed = matches!(input.trim(), "y" | "Y");
            if !confirmed {
                println!("Keeping existing config.");
                return display_config(&resolved_path);
            }
        }

        fs::write(&resolved_path, DEFAULT_FULL_CONFIG)?;
        println!("Config initialized at {}", resolved_path.display());
    }

    display_config(&resolved_path)
}

fn display_config(path: &Path) -> Result<()> {
    if !path.exists() {
        println!(
            "No config file found at {}. Run `vibemate config --init` to create one.",
            path.display()
        );
        return Ok(());
    }

    let contents = fs::read_to_string(path)?;
    println!("{contents}");
    Ok(())
}
