//! Presentation-neutral protocol-v2 run control for the CLI and TUI.
//!
//! This module deliberately exposes one-request commands and snapshot-consistent
//! event pages. It does not poll, subscribe, retry mutations, or grant callers a
//! way to choose executables, arguments, environment variables, or workspaces.

use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

use crate::app_core::{
    AppCommand, AppEvent, BackendSelection, ConversationId, IdempotencyKey, MAX_RUN_EVENT_PAGE,
    RunEventPage, RunEventsQuery, RunId, RunLifecycleEvent, RunMode, RunQuery, RunRequest,
    RunSnapshot, RunState, RunSubmissionDisposition,
};
#[cfg(not(unix))]
use crate::daemon::ClientError;
#[cfg(unix)]
use crate::daemon::{DaemonClient, DaemonConfig};

/// Closed provider choices exposed by the bounded daemon runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunBackendArg {
    Abi,
    #[value(name = "foundation-models", alias = "fm")]
    FoundationModels,
}

impl From<RunBackendArg> for BackendSelection {
    fn from(value: RunBackendArg) -> Self {
        match value {
            RunBackendArg::Abi => Self::Abi,
            RunBackendArg::FoundationModels => Self::FoundationModels,
        }
    }
}

/// Closed scheduling modes supported by the startup-bound daemon routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunModeArg {
    #[value(name = "one-shot")]
    OneShot,
    Background,
}

impl From<RunModeArg> for RunMode {
    fn from(value: RunModeArg) -> Self {
        match value {
            RunModeArg::OneShot => Self::OneShot,
            RunModeArg::Background => Self::Background,
        }
    }
}

/// The exact run-control grammar shared by Clap and /daemon run.
#[derive(Clone, PartialEq, Eq, Subcommand)]
pub enum RunControlCliCommand {
    /// Submit one bounded request to a startup-configured provider route
    Submit {
        #[arg(long, value_enum)]
        backend: RunBackendArg,
        #[arg(long, value_enum, default_value = "background")]
        run_mode: RunModeArg,
        #[arg(long)]
        idempotency_key: Option<IdempotencyKey>,
        #[arg(long)]
        conversation_id: Option<ConversationId>,
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Emit the sanitized frontend-neutral reducer view as JSON
        #[arg(long)]
        json: bool,
        /// Provider input; never echoed by the run-control view
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        input: Vec<String>,
    },
    /// Read one durable run snapshot
    Status {
        run_id: RunId,
        /// Emit the sanitized frontend-neutral reducer view as JSON
        #[arg(long)]
        json: bool,
    },
    /// Request cancellation of one durable run
    Cancel {
        run_id: RunId,
        /// Emit the sanitized frontend-neutral reducer view as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read one bounded, fixed-watermark lifecycle-event page
    Events {
        run_id: RunId,
        #[arg(long, default_value_t = 0)]
        after_sequence: u64,
        #[arg(long)]
        through_sequence: Option<u64>,
        #[arg(long, default_value_t = MAX_RUN_EVENT_PAGE, value_parser = clap::value_parser!(u16).range(1..=i64::from(MAX_RUN_EVENT_PAGE)))]
        limit: u16,
        /// Emit the sanitized frontend-neutral reducer view as JSON
        #[arg(long)]
        json: bool,
    },
}

impl fmt::Debug for RunControlCliCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Submit {
                backend,
                run_mode,
                idempotency_key,
                conversation_id,
                labels,
                json,
                input,
            } => formatter
                .debug_struct("Submit")
                .field("backend", backend)
                .field("run_mode", run_mode)
                .field("idempotency_key", idempotency_key)
                .field("conversation_id", conversation_id)
                .field("label_count", &labels.len())
                .field("json", json)
                .field("input", &"[REDACTED]")
                .field("input_words", &input.len())
                .finish(),
            Self::Status { run_id, json } => formatter
                .debug_struct("Status")
                .field("run_id", run_id)
                .field("json", json)
                .finish(),
            Self::Cancel { run_id, json } => formatter
                .debug_struct("Cancel")
                .field("run_id", run_id)
                .field("json", json)
                .finish(),
            Self::Events {
                run_id,
                after_sequence,
                through_sequence,
                limit,
                json,
            } => formatter
                .debug_struct("Events")
                .field("run_id", run_id)
                .field("after_sequence", after_sequence)
                .field("through_sequence", through_sequence)
                .field("limit", limit)
                .field("json", json)
                .finish(),
        }
    }
}

impl RunControlCliCommand {
    #[must_use]
    pub const fn json(&self) -> bool {
        match self {
            Self::Submit { json, .. }
            | Self::Status { json, .. }
            | Self::Cancel { json, .. }
            | Self::Events { json, .. } => *json,
        }
    }

    /// Convert the closed presentation grammar into the shared application command.
    pub fn into_app_command(self) -> Result<AppCommand> {
        let command = match self {
            Self::Submit {
                backend,
                run_mode,
                idempotency_key,
                conversation_id,
                labels,
                input,
                ..
            } => AppCommand::SubmitRun(RunRequest {
                idempotency_key: idempotency_key.unwrap_or_default(),
                conversation_id,
                mode: run_mode.into(),
                backend: backend.into(),
                input: input.join(" "),
                labels,
            }),
            Self::Status { run_id, .. } => AppCommand::GetRun(RunQuery { run_id }),
            Self::Cancel { run_id, .. } => AppCommand::CancelRun(RunQuery { run_id }),
            Self::Events {
                run_id,
                after_sequence,
                through_sequence,
                limit,
                ..
            } => AppCommand::RunEvents(RunEventsQuery {
                run_id,
                after_sequence,
                through_sequence,
                limit,
            }),
        };
        command
            .validate()
            .map_err(|error| anyhow!(error.to_string()))?;
        Ok(command)
    }
}

#[derive(Debug, Parser)]
#[command(no_binary_name = true)]
struct SlashRunParser {
    #[command(subcommand)]
    command: RunControlCliCommand,
}

/// Parse tokens after /daemon run with the same subcommand type Clap uses.
pub fn parse_slash_run_args(args: &[String]) -> Result<RunControlCliCommand> {
    SlashRunParser::try_parse_from(args)
        .map(|parsed| parsed.command)
        .map_err(|_| anyhow!("invalid /daemon run arguments; use /daemon run --help"))
}

/// Sanitized frontend-neutral projection accumulated from validated responses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunControlView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<RunSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submission: Option<RunSubmissionDisposition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_pages: Vec<RunEventPage>,
}

impl RunControlView {
    #[must_use]
    pub fn run_id(&self) -> Option<&RunId> {
        self.snapshot
            .as_ref()
            .map(|snapshot| &snapshot.run_id)
            .or_else(|| self.event_pages.first().map(|page| &page.run_id))
    }
}

/// Fail-closed projection errors. Messages never include request input or secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RunControlError {
    #[error("run-control command is invalid")]
    InvalidCommand,
    #[error("run-control response is invalid")]
    InvalidEvent,
    #[error("run-control response does not match its command")]
    MismatchedResponse,
    #[error("run-control reducer cannot combine different run identities")]
    MismatchedRun,
    #[error("run-control snapshot regressed")]
    SnapshotRegression,
    #[error("run-control event page changed its fixed watermark or cursor")]
    PaginationRegression,
    #[error("run-control lifecycle events are out of order")]
    LifecycleRegression,
}

/// Reducer shared by terminal frontends. It owns no transport or execution authority.
#[derive(Debug, Clone, Default)]
pub struct RunLifecycleReducer {
    view: RunControlView,
}

impl RunLifecycleReducer {
    #[must_use]
    pub fn view(&self) -> &RunControlView {
        &self.view
    }

    pub fn apply(&mut self, command: &AppCommand, event: AppEvent) -> Result<(), RunControlError> {
        command
            .validate()
            .map_err(|_| RunControlError::InvalidCommand)?;
        event
            .validate()
            .map_err(|_| RunControlError::InvalidEvent)?;

        match (command, event) {
            (AppCommand::SubmitRun(request), AppEvent::RunSubmitted(submission)) => {
                if submission.run.idempotency_key != request.idempotency_key
                    || submission.run.conversation_id != request.conversation_id
                {
                    return Err(RunControlError::MismatchedResponse);
                }
                self.merge_snapshot(submission.run)?;
                self.view.submission = Some(submission.disposition);
            }
            (AppCommand::GetRun(query), AppEvent::RunStatus(snapshot))
            | (AppCommand::CancelRun(query), AppEvent::CancellationAcknowledged(snapshot)) => {
                if snapshot.run_id != query.run_id {
                    return Err(RunControlError::MismatchedResponse);
                }
                self.merge_snapshot(snapshot)?;
            }
            (AppCommand::RunEvents(query), AppEvent::RunEvents(page)) => {
                self.merge_page(query, page)?;
            }
            _ => return Err(RunControlError::MismatchedResponse),
        }
        Ok(())
    }

    /// Build the next explicit page request. This never polls or subscribes.
    pub fn next_events_command(&self, limit: u16) -> Result<Option<AppCommand>, RunControlError> {
        if limit == 0 || limit > MAX_RUN_EVENT_PAGE {
            return Err(RunControlError::InvalidCommand);
        }
        let Some(page) = self.view.event_pages.last() else {
            return Ok(None);
        };
        if !page.has_more {
            return Ok(None);
        }
        Ok(Some(AppCommand::RunEvents(RunEventsQuery {
            run_id: page.run_id.clone(),
            after_sequence: page.next_after_sequence,
            through_sequence: Some(page.through_sequence),
            limit,
        })))
    }

    fn merge_snapshot(&mut self, snapshot: RunSnapshot) -> Result<(), RunControlError> {
        self.bind_run(&snapshot.run_id)?;
        if let Some(previous) = &self.view.snapshot
            && (snapshot.event_count < previous.event_count
                || snapshot.idempotency_key != previous.idempotency_key
                || snapshot.conversation_id != previous.conversation_id
                || snapshot.created_at != previous.created_at
                || (snapshot.state != previous.state
                    && snapshot.event_count == previous.event_count)
                || (previous.state.is_terminal() && &snapshot != previous)
                || !snapshot_state_can_follow(previous.state, snapshot.state))
        {
            return Err(RunControlError::SnapshotRegression);
        }
        self.view.snapshot = Some(snapshot);
        Ok(())
    }

    fn merge_page(
        &mut self,
        query: &RunEventsQuery,
        page: RunEventPage,
    ) -> Result<(), RunControlError> {
        if page.run_id != query.run_id
            || page.after_sequence != query.after_sequence
            || query
                .through_sequence
                .is_some_and(|through| through != page.through_sequence)
            || page.events.len() > usize::from(query.limit)
        {
            return Err(RunControlError::MismatchedResponse);
        }
        self.bind_run(&page.run_id)?;

        if let Some(previous) = self.view.event_pages.last()
            && (query.after_sequence != previous.next_after_sequence
                || query.through_sequence != Some(previous.through_sequence)
                || page.through_sequence != previous.through_sequence
                || !previous.has_more)
        {
            return Err(RunControlError::PaginationRegression);
        }
        validate_event_order(self.view.event_pages.last(), &page)?;
        self.view.event_pages.push(page);
        Ok(())
    }

    fn bind_run(&self, run_id: &RunId) -> Result<(), RunControlError> {
        if self.view.run_id().is_some_and(|current| current != run_id) {
            Err(RunControlError::MismatchedRun)
        } else {
            Ok(())
        }
    }
}

fn snapshot_state_can_follow(previous: RunState, next: RunState) -> bool {
    if previous == next {
        return true;
    }
    match previous {
        RunState::Queued => true,
        RunState::Starting => !matches!(next, RunState::Queued),
        RunState::Running => !matches!(next, RunState::Queued | RunState::Starting),
        RunState::CancelRequested => next.is_terminal(),
        RunState::Succeeded | RunState::Failed | RunState::Cancelled | RunState::Interrupted => {
            false
        }
    }
}

fn validate_event_order(
    previous_page: Option<&RunEventPage>,
    page: &RunEventPage,
) -> Result<(), RunControlError> {
    let previous_event = previous_page
        .and_then(|previous| previous.events.last())
        .map(|record| &record.event);
    let mut previous_state = previous_event.map(event_state);
    if page.after_sequence == 0
        && page
            .events
            .first()
            .is_some_and(|record| !matches!(record.event, RunLifecycleEvent::Queued))
    {
        return Err(RunControlError::LifecycleRegression);
    }
    for record in &page.events {
        let state = event_state(&record.event);
        if let Some(previous) = previous_state
            && previous.validate_transition(state).is_err()
        {
            return Err(RunControlError::LifecycleRegression);
        }
        previous_state = Some(state);
    }
    Ok(())
}

fn event_state(event: &RunLifecycleEvent) -> RunState {
    match event {
        RunLifecycleEvent::Queued => RunState::Queued,
        RunLifecycleEvent::Starting => RunState::Starting,
        RunLifecycleEvent::Running => RunState::Running,
        RunLifecycleEvent::CancelRequested => RunState::CancelRequested,
        RunLifecycleEvent::Succeeded => RunState::Succeeded,
        RunLifecycleEvent::Failed { .. } => RunState::Failed,
        RunLifecycleEvent::Cancelled { .. } => RunState::Cancelled,
        RunLifecycleEvent::Interrupted { .. } => RunState::Interrupted,
    }
}

/// Execute and render one bounded run-control operation.
pub fn dispatch(command: RunControlCliCommand) -> Result<i32> {
    let json = command.json();
    let app_command = command.into_app_command()?;
    let event = request_daemon(app_command.clone())?;
    let mut reducer = RunLifecycleReducer::default();
    reducer.apply(&app_command, event)?;
    if json {
        println!("{}", serde_json::to_string_pretty(reducer.view())?);
    } else {
        print!("{}", render_human(reducer.view()));
    }
    Ok(0)
}

#[cfg(unix)]
fn request_daemon(command: AppCommand) -> Result<AppEvent> {
    let config = DaemonConfig::from_env()?;
    Ok(DaemonClient::new(config).request(command)?)
}

#[cfg(not(unix))]
fn request_daemon(_command: AppCommand) -> Result<AppEvent> {
    Err(ClientError::UnsupportedPlatform.into())
}

#[must_use]
pub fn render_human(view: &RunControlView) -> String {
    let mut rendered = String::new();
    if let Some(disposition) = view.submission {
        rendered.push_str(&format!("submission: {}\n", submission_label(disposition)));
    }
    if let Some(snapshot) = &view.snapshot {
        rendered.push_str(&format!(
            "run: {}\nstate: {}\nevents: {}\n",
            snapshot.run_id,
            state_label(snapshot.state),
            snapshot.event_count
        ));
        if let Some(failure) = &snapshot.failure {
            rendered.push_str(&format!(
                "failure: {} · {} · retryable={}\n",
                failure.code, failure.message, failure.retryable
            ));
        }
    }
    for page in &view.event_pages {
        for record in &page.events {
            rendered.push_str(&format!(
                "event {}: {} · {}\n",
                record.sequence,
                lifecycle_label(&record.event),
                record.recorded_at
            ));
        }
        rendered.push_str(&format!(
            "page: after={} next={} through={} has_more={}\n",
            page.after_sequence, page.next_after_sequence, page.through_sequence, page.has_more
        ));
    }
    rendered
}

fn submission_label(disposition: RunSubmissionDisposition) -> &'static str {
    match disposition {
        RunSubmissionDisposition::Enqueued => "enqueued",
        RunSubmissionDisposition::Existing => "existing",
        RunSubmissionDisposition::QueueFull => "queue_full",
    }
}

fn state_label(state: RunState) -> &'static str {
    match state {
        RunState::Queued => "queued",
        RunState::Starting => "starting",
        RunState::Running => "running",
        RunState::CancelRequested => "cancel_requested",
        RunState::Succeeded => "succeeded",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
        RunState::Interrupted => "interrupted",
    }
}

fn lifecycle_label(event: &RunLifecycleEvent) -> &'static str {
    match event {
        RunLifecycleEvent::Queued => "queued",
        RunLifecycleEvent::Starting => "starting",
        RunLifecycleEvent::Running => "running",
        RunLifecycleEvent::CancelRequested => "cancel_requested",
        RunLifecycleEvent::Succeeded => "succeeded",
        RunLifecycleEvent::Failed { .. } => "failed",
        RunLifecycleEvent::Cancelled { .. } => "cancelled",
        RunLifecycleEvent::Interrupted { .. } => "interrupted",
    }
}
