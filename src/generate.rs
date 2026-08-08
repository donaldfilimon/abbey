//! Image/video generation and structured reasoning via cursor-agent.
//!
//! Abbey does **not** ship a local image or video model. These commands craft
//! prompts that ask the backend agent to use whatever generation tools it has
//! (built-in or MCP) and write files into the workspace. Under `ABBEY_BACKEND=fm`
//! or `ABBEY_BACKEND=abi` generation is refused — those backends have no
//! image/video tool surface.
//!
//! Reasoning is a Cursor `*-thinking-*` model + a structured reasoning wrap,
//! not an Abbey-owned chain-of-thought UI.

use crate::actions::{RunSpec, run_agent};
use crate::agent::{AgentBackend, AgentConfig};
use crate::media::{self, MediaAttach, MediaKind};
use crate::models::resolve_model;
use crate::session::apply_media_attach;
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenKind {
    Image,
    Video,
}

fn refuse_no_tools(backend: AgentBackend, kind: &str) -> Result<i32> {
    eprintln!(
        "abbey: `{kind}` needs cursor-agent tool access.\n\
         The `{}` backend (ABBEY_BACKEND={}) has no image/video generation surface.\n\
         Unset ABBEY_BACKEND to use cursor-agent.",
        backend.label(),
        backend.label(),
    );
    Ok(2)
}

fn default_out(kind: GenKind) -> PathBuf {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    match kind {
        GenKind::Image => PathBuf::from(format!("abbey-gen-{stamp}.png")),
        GenKind::Video => PathBuf::from(format!("abbey-gen-{stamp}.mp4")),
    }
}

/// Prompt that asks the agent to generate (or edit) an image via its tools.
pub fn image_prompt(desc: &str, out: &Path, aspect: Option<&str>, edit: Option<&Path>) -> String {
    let aspect = aspect.unwrap_or("auto");
    let edit_line = match edit {
        Some(p) => format!(
            "Edit the existing image at `{}` according to the description \
             (prefer an image_edit-style tool if available).\n",
            p.display()
        ),
        None => "Generate a new image from the description \
                 (prefer an image_gen-style tool if available).\n"
            .into(),
    };
    format!(
        "You are generating a visual asset for the user.\n\
         {edit_line}\
         Description:\n{desc}\n\n\
         Requirements:\n\
         - Prefer available image generation/edit tools or MCP image tools.\n\
         - Aspect ratio preference: {aspect}.\n\
         - Write the final image file to: `{out}`\n\
         - If no image tool is available, say so plainly and do not pretend you wrote a file.\n\
         - When done, print the absolute path of the written file.\n\
         - Do not invent a base64 dump in chat unless a tool requires it.\n",
        out = out.display()
    )
}

/// Prompt that asks the agent to generate a video via its tools (best-effort).
pub fn video_prompt(desc: &str, out: &Path) -> String {
    format!(
        "You are generating a short video asset for the user.\n\
         Description:\n{desc}\n\n\
         Requirements:\n\
         - Use any available video generation tool or MCP video tool.\n\
         - Write the final video file to: `{out}`\n\
         - If no video generation tool is available, say so plainly — do not claim success.\n\
         - When done, print the absolute path of the written file.\n\
         - Abbey itself has no local video model; this depends entirely on agent tools.\n",
        out = out.display()
    )
}

/// Structured reasoning wrap — pairs with a Cursor thinking model.
pub fn reason_prompt(task: &str) -> String {
    format!(
        "Reason carefully before concluding.\n\
         Structure your answer as:\n\
         1. Restate the question/goal\n\
         2. Key assumptions and unknowns\n\
         3. Step-by-step reasoning (show work)\n\
         4. Alternatives considered\n\
         5. Conclusion and confidence\n\
         6. What you would verify next\n\n\
         Task:\n{task}"
    )
}

pub fn run_generate(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    kind: GenKind,
    desc: &[String],
    out: Option<PathBuf>,
    aspect: Option<&str>,
    edit: Option<PathBuf>,
) -> Result<i32> {
    if matches!(cfg.backend, AgentBackend::Fm | AgentBackend::Abi) {
        return refuse_no_tools(
            cfg.backend,
            match kind {
                GenKind::Image => "imagine/generate image",
                GenKind::Video => "generate video",
            },
        );
    }
    let desc = desc.join(" ");
    if desc.trim().is_empty() {
        bail!(
            "usage: abbey {} <description…> [--out PATH]",
            match kind {
                GenKind::Image => "imagine",
                GenKind::Video => "generate video",
            }
        );
    }

    let out = out.unwrap_or_else(|| default_out(kind));
    let out_abs = if out.is_absolute() {
        out
    } else {
        state.cwd.join(out)
    };
    if let Some(parent) = out_abs.parent() {
        if !parent.as_os_str().is_empty() && !cfg.add_dirs.iter().any(|d| d == parent) {
            cfg.add_dirs.push(parent.to_path_buf());
        }
    }

    if let Some(ref edit_path) = edit {
        let (abs, _) =
            media::resolve_media_path(&edit_path.display().to_string(), Some(MediaKind::Image))?;
        let mut attach = MediaAttach::default();
        attach.extend_paths([(abs, MediaKind::Image)]);
        apply_media_attach(cfg, &attach);
    }

    // Tools need write/shell; prefer force so generation isn't stuck on prompts.
    cfg.force = true;
    if !cfg.extra_args.iter().any(|a| a == "--approve-mcps") {
        cfg.extra_args.push("--approve-mcps".into());
    }

    let prompt = match kind {
        GenKind::Image => image_prompt(&desc, &out_abs, aspect, edit.as_deref()),
        GenKind::Video => video_prompt(&desc, &out_abs),
    };

    eprintln!(
        "abbey: {} → {} (via cursor-agent tools; not a local model)",
        match kind {
            GenKind::Image => "imagine",
            GenKind::Video => "generate-video",
        },
        out_abs.display()
    );

    run_agent(cfg, state, &[prompt], RunSpec::gemma())
}

pub fn run_reason(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    task: &[String],
    thinking_level: Option<&str>,
) -> Result<i32> {
    let task = task.join(" ");
    if task.trim().is_empty() {
        bail!("usage: abbey reason [--thinking LEVEL] <task…>");
    }
    let level = thinking_level.unwrap_or("xhigh");
    cfg.model = resolve_model(&format!("fable-thinking-{level}"));
    cfg.print = true;
    cfg.force_capture = true;
    cfg.cot_path = Some(crate::surfaces::cot_path(state));
    let prompt = reason_prompt(&task);
    eprintln!(
        "abbey: reason with {} (Cursor thinking model; structured wrap + cot save)",
        cfg.model
    );
    run_agent(cfg, state, &[prompt], RunSpec::max())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_prompt_names_out_and_aspect() {
        let p = image_prompt("a fox", Path::new("/tmp/fox.png"), Some("16:9"), None);
        assert!(p.contains("/tmp/fox.png"));
        assert!(p.contains("16:9"));
        assert!(p.contains("image_gen"));
        assert!(!p.contains("Edit the existing"));
    }

    #[test]
    fn image_edit_prompt_references_source() {
        let p = image_prompt(
            "make it night",
            Path::new("/tmp/out.png"),
            None,
            Some(Path::new("/tmp/in.png")),
        );
        assert!(p.contains("/tmp/in.png"));
        assert!(p.contains("image_edit"));
    }

    #[test]
    fn video_prompt_is_honest_about_tools() {
        let p = video_prompt("timelapse", Path::new("out.mp4"));
        assert!(p.contains("out.mp4"));
        assert!(p.contains("no local video model"));
    }

    #[test]
    fn reason_prompt_has_structure() {
        let p = reason_prompt("should we split the module?");
        assert!(p.contains("Step-by-step reasoning"));
        assert!(p.contains("should we split the module?"));
    }
}
