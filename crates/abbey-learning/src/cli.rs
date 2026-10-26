use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "abbey-model",
    about = "Explicit offline Rust model-family pipeline"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Args)]
struct Consent {
    #[arg(long)]
    consent: PathBuf,
}
#[derive(Args)]
struct Runtime {
    #[command(flatten)]
    grant: Consent,
    #[arg(long, default_value_t = 3600)]
    timeout: u64,
    #[arg(long, default_value = "cpu")]
    device: String,
}
#[derive(Subcommand)]
enum Cmd {
    Inventory {
        roots: Vec<PathBuf>,
    },
    CollectLearning {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        abbey_root: PathBuf,
        #[arg(long)]
        abi_root: PathBuf,
        #[arg(long)]
        wdbx_root: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        grant: Consent,
    },
    Prepare {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        private: bool,
        #[command(flatten)]
        grant: Consent,
    },
    FreezeSuite {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        grant: Consent,
    },
    Train {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        dataset: PathBuf,
        #[arg(long)]
        suite: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        license: String,
        #[arg(long, default_value_t = 100)]
        iters: u32,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[command(flatten)]
        runtime: Runtime,
    },
    Evaluate {
        #[arg(long)]
        run: PathBuf,
        #[arg(long)]
        suite: PathBuf,
        #[command(flatten)]
        runtime: Runtime,
    },
    Export {
        #[arg(long)]
        run: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        bits: Option<u8>,
        #[command(flatten)]
        runtime: Runtime,
    },
    Benchmark {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        suite: PathBuf,
        #[arg(long)]
        heldout: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        runtime: Runtime,
    },
    Promote {
        #[arg(long)]
        run: PathBuf,
        #[arg(long)]
        registry: PathBuf,
        #[command(flatten)]
        grant: Consent,
    },
    Rollback {
        #[arg(long)]
        registry: PathBuf,
        #[command(flatten)]
        grant: Consent,
    },
    /// Return a compact verified receipt; recording it needs separate memory consent.
    RecordReceipt {
        #[arg(long)]
        run: PathBuf,
    },
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        request: PathBuf,
    },
}
pub(crate) fn run(args: &[String], prefix: &[String]) -> Result<Value> {
    let cli = Cli::try_parse_from(
        std::iter::once("abbey-model".to_string()).chain(args.iter().cloned()),
    )?;
    let result = match cli.command {
        Cmd::Inventory { roots } => crate::pipeline::inventory(&roots)?,
        Cmd::CollectLearning {
            config,
            abbey_root,
            abi_root,
            wdbx_root,
            output,
            grant,
        } => crate::learning::collect(
            &config,
            &[
                ("abbey".into(), abbey_root),
                ("abi".into(), abi_root),
                ("wdbx".into(), wdbx_root),
            ]
            .into(),
            &output,
            &grant.consent,
        )?,
        Cmd::Prepare {
            source,
            output,
            private,
            grant,
        } => crate::data::prepare(&source, &output, &grant.consent, private)?,
        Cmd::FreezeSuite {
            source,
            output,
            grant,
        } => crate::data::freeze_suite(&source, &output, &grant.consent)?,
        Cmd::Train {
            base,
            dataset,
            suite,
            output,
            license,
            iters,
            seed,
            runtime,
        } => crate::pipeline::train(
            &json!({"base":base,"dataset":dataset,"suite":suite,"output":output,"license":license,"iterations":iters,"seed":seed,"device":runtime.device,"consent":runtime.grant.consent,"timeout":runtime.timeout}),
            prefix,
        )?,
        Cmd::Evaluate {
            run,
            suite,
            runtime,
        } => crate::pipeline::evaluate(
            &run,
            &suite,
            &runtime.grant.consent,
            &runtime.device,
            runtime.timeout,
            prefix,
        )?,
        Cmd::Export {
            run,
            output,
            bits,
            runtime,
        } => crate::pipeline::export(
            &run,
            &output,
            bits,
            &runtime.grant.consent,
            &runtime.device,
            runtime.timeout,
            prefix,
        )?,
        Cmd::Benchmark {
            base,
            suite,
            heldout,
            output,
            runtime,
        } => crate::pipeline::benchmark(
            &base,
            &suite,
            &heldout,
            &output,
            &runtime.grant.consent,
            &runtime.device,
            runtime.timeout,
            prefix,
        )?,
        Cmd::Promote {
            run,
            registry,
            grant,
        } => crate::registry::promote(&run, &registry, &grant.consent)?,
        Cmd::Rollback { registry, grant } => crate::registry::rollback(&registry, &grant.consent)?,
        Cmd::RecordReceipt { run } => crate::registry::compact_receipt(&run)?,
        Cmd::Worker { request } => crate::native::execute(&crate::io::read_json(&request)?)?,
    };
    Ok(result)
}
