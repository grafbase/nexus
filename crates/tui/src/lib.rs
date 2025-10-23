//! Terminal user interface for exploring Nexus traces, metrics, and logs.

mod app;
mod orchestrator;
mod poll;
mod runner;
mod ui;

use std::{io, time::Duration};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use telemetry::tracing::TraceExportReceiver;
use tokio::{sync::watch, task};
use tokio_util::sync::CancellationToken;

use crate::{orchestrator::Orchestrator, poll::Poller, runner::Runner};

/// Minimum time between redraws when nothing new arrives. This throttles CPU
/// usage so we are not repainting faster than the eye can register.
const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

/// Maximum number of attribute rows shown per span detail.
const ATTRIBUTE_ROW_LIMIT: usize = 16;

/// Polling cadence for keyboard and mouse events. Short enough to feel
/// responsive but long enough to avoid busy waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Launch the TUI on a blocking thread and coordinate shutdown with the
/// async runtime.
///
/// The tracing subsystem pushes updates through `receiver`. When the user quits
/// the UI we cancel the shared `shutdown` token to stop the rest of the
/// pipeline.
pub async fn spawn(receiver: TraceExportReceiver, shutdown: CancellationToken, version: String) {
    let shutdown_for_tui = shutdown.clone();

    let handle = task::spawn(async move { run_tui(receiver, shutdown_for_tui, version).await });

    match handle.await {
        Ok(Ok(true)) => {
            // The user quit from inside the UI, so propagate shutdown to the
            // rest of the process.
            shutdown.cancel();
        }
        Ok(Ok(false)) => {}
        Ok(Err(err)) => {
            eprintln!("TUI encountered an error: {err}");
        }
        Err(err) => {
            eprintln!("TUI task failed to join: {err}");
        }
    }
}

/// Set up the terminal backend, run the interactive loop, and restore the
/// original terminal state on exit.
async fn run_tui(receiver: TraceExportReceiver, shutdown: CancellationToken, version: String) -> anyhow::Result<bool> {
    // Raw mode gives us direct access to keystrokes and mouse events without
    // line buffering or echo from the terminal driver.
    enable_raw_mode()?;

    let mut stdout = io::stdout();

    // Switch to an alternate screen so we leave the original terminal content
    // untouched and allow clean restoration afterwards.
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let (status_tx, status_rx) = watch::channel(ui::UiStatus::default());
    let (logs_tx, logs_rx) = watch::channel(ui::LogsSnapshot::default());
    let (metrics_tx, metrics_rx) = watch::channel(ui::MetricsSnapshot::default());
    let (traces_tx, traces_rx) = watch::channel(ui::TracesSnapshot::default());

    let orchestrator = Orchestrator {
        receiver,
        status_tx,
        logs_tx,
        metrics_tx,
        traces_tx,
    };

    let orchestrator = task::spawn_blocking(move || {
        orchestrator.run();
    });

    let poller = Poller {
        status_rx,
        logs_rx,
        metrics_rx,
        traces_rx,
    };

    let runner = Runner {
        version,
        poller,
        shutdown,
    };

    let result = runner.run(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    let _ = orchestrator.await;

    result
}
