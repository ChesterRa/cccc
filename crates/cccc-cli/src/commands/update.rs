use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::args::UpdateArgs;

const CARGO: &str = "cargo";
const PACKAGE: &str = "cccc";
const UPDATE_ARGS: &[&str] = &["install", PACKAGE, "--force", "--locked"];

#[derive(Debug, PartialEq, Eq)]
struct UpdatePlan {
    program: &'static str,
    args: &'static [&'static str],
}

impl UpdatePlan {
    fn crates_io() -> Self {
        Self {
            program: CARGO,
            args: UPDATE_ARGS,
        }
    }

    fn display(&self) -> String {
        std::iter::once(self.program)
            .chain(self.args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Update the public Rust crate. Returns `true` when Cargo completed an install.
pub fn run(args: UpdateArgs, product_version: &str, crate_version: &str) -> Result<bool> {
    let plan = UpdatePlan::crates_io();
    println!("Current CCCC product version: {product_version}");
    println!("Current crates.io package version: {crate_version}");
    println!("Update source: crates.io");
    println!("Command: {}", plan.display());

    if args.check {
        return Ok(false);
    }

    let status = Command::new(plan.program)
        .args(plan.args)
        .status()
        .with_context(|| "could not run Cargo; install Rust/Cargo and retry `cccc update`")?;
    if !status.success() {
        bail!("Cargo update failed with {status}");
    }

    println!("CCCC Rust update installed successfully.");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crates_io_plan_reinstalls_the_public_crate_from_its_lockfile() {
        let plan = UpdatePlan::crates_io();
        assert_eq!(plan.program, "cargo");
        assert_eq!(plan.args, ["install", "cccc", "--force", "--locked"]);
        assert_eq!(plan.display(), "cargo install cccc --force --locked");
    }

    #[test]
    fn check_mode_never_runs_cargo() {
        let installed = run(UpdateArgs { check: true }, "0.4.33", "0.0.5").expect("check");
        assert!(!installed);
    }
}
