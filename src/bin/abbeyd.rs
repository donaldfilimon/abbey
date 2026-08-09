//! Abbey's bounded authenticated local daemon endpoint.

use std::process::ExitCode;

use abbey::daemon::{DaemonConfig, DaemonServer, RuntimeDaemonConfig, RuntimeHandler, Shutdown};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("abbeyd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Block process-termination signals before RuntimeHandler starts its
    // manager worker. POSIX signal masks are inherited at thread creation.
    let shutdown = shutdown_on_signals()?;
    let config = DaemonConfig::from_env()?;
    let runtime = RuntimeHandler::start(RuntimeDaemonConfig::from_env()?)?;
    DaemonServer::new(config, runtime).serve(shutdown)?;
    Ok(())
}

#[cfg(unix)]
fn shutdown_on_signals() -> Result<Shutdown, nix::Error> {
    use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};

    let mut signals = SigSet::empty();
    signals.add(Signal::SIGINT);
    signals.add(Signal::SIGTERM);
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&signals), None)?;

    let shutdown = Shutdown::default();
    let signal_shutdown = shutdown.clone();
    std::thread::spawn(move || {
        if signals.wait().is_ok() {
            signal_shutdown.request();
        }
    });
    Ok(shutdown)
}

#[cfg(not(unix))]
fn shutdown_on_signals() -> Result<Shutdown, std::convert::Infallible> {
    Ok(Shutdown::default())
}
