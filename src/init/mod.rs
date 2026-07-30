//! Claude Code–parity `/init`: scan the cwd and scaffold `AGENTS.md`.

mod detect;
mod probe;

pub use probe::ProjectProbe;

use anyhow::Result;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct InitOpts {
    /// Overwrite an existing AGENTS.md.
    pub force: bool,
    /// Print generated markdown; do not write.
    pub print_only: bool,
    /// Also emit a prompt suitable for agent refinement (`abbey init --agent`).
    pub agent: bool,
}

/// Run local `/init`. Returns a human status string; when `opts.agent`, also returns
/// an optional agent prompt as the second value.
pub fn run_init(cwd: &Path, opts: InitOpts) -> Result<(String, Option<String>)> {
    let path = cwd.join("AGENTS.md");
    if path.exists() && !opts.force && !opts.print_only {
        let mut msg = format!("already exists: {}", path.display());
        let _ = write!(
            msg,
            "\nre-run with --force to overwrite, or --print to preview a fresh scan"
        );
        if opts.agent {
            let probe = ProjectProbe::scan(cwd)?;
            let draft = if path.exists() {
                fs::read_to_string(&path).unwrap_or_else(|_| probe.render_agents_md())
            } else {
                probe.render_agents_md()
            };
            return Ok((msg, Some(probe.agent_refine_prompt(&draft))));
        }
        return Ok((msg, None));
    }

    let probe = ProjectProbe::scan(cwd)?;
    let content = probe.render_agents_md();
    let agent_prompt = opts.agent.then(|| probe.agent_refine_prompt(&content));

    if opts.print_only {
        return Ok((content, agent_prompt));
    }

    fs::write(&path, &content)?;
    let mut status = format!(
        "wrote {} ({} stack)",
        path.display(),
        probe.languages.join("/")
    );
    if opts.force {
        status.push_str(" [forced]");
    }
    Ok((status, agent_prompt))
}

pub fn parse_init_args(rest: &str) -> InitOpts {
    let mut opts = InitOpts::default();
    for tok in rest.split_whitespace() {
        match tok {
            "--force" | "-f" | "force" => opts.force = true,
            "--print" | "-p" | "print" | "preview" => opts.print_only = true,
            "--agent" | "agent" | "refine" => opts.agent = true,
            _ => {}
        }
    }
    opts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_rust_project() {
        let dir = std::env::temp_dir().join(format!("abbey-init-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "demo-crate"
version = "0.1.0"
edition = "2024"
description = "A demo crate for init"
"#,
        )
        .unwrap();
        fs::write(
            dir.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"nightly\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("README.md"),
            "# Demo\n\nUseful demo project for agents.\n",
        )
        .unwrap();

        let probe = ProjectProbe::scan(&dir).unwrap();
        assert_eq!(probe.name, "demo-crate");
        assert!(probe.languages.contains(&"Rust"));
        assert!(probe.build.iter().any(|b| b.contains("cargo")));
        let md = probe.render_agents_md();
        assert!(md.contains("demo-crate"));
        assert!(md.contains("cargo test"));

        let (status, _) = run_init(&dir, InitOpts::default()).unwrap();
        assert!(status.contains("wrote"));
        assert!(dir.join("AGENTS.md").is_file());

        let err = run_init(&dir, InitOpts::default()).unwrap();
        assert!(err.0.contains("already exists"));

        let (preview, _) = run_init(
            &dir,
            InitOpts {
                print_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(preview.starts_with("# AGENTS.md"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_args() {
        let o = parse_init_args("--force agent");
        assert!(o.force && o.agent && !o.print_only);
    }
}
