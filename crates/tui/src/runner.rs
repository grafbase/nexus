use std::time::Instant;

use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{Terminal, prelude::Backend};
use tokio_util::sync::CancellationToken;

use crate::{
    POLL_INTERVAL, REFRESH_INTERVAL,
    poll::Poller,
    ui::{Tab, Ui},
};

/// The main application runner that manages the event loop, UI rendering, and user input handling.
///
/// The `Runner` coordinates between polling for telemetry data, rendering the terminal UI,
/// and processing user interactions. It handles the complete lifecycle of the application
/// from initialization through shutdown.
pub struct Runner {
    /// Version string displayed in the UI
    pub version: String,
    /// Poller for collecting telemetry data
    pub poller: Poller,
    /// Cancellation token for coordinating shutdown across async tasks
    pub shutdown: CancellationToken,
}

impl Runner {
    /// Drive the main event loop: process incoming telemetry, render frames, and
    /// react to user input.
    ///
    /// Returns `true` when the user explicitly asked to quit (so the caller can
    /// cascade the shutdown) and `false` when the stream ended on its own.
    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> anyhow::Result<bool> {
        let mut ui = self.initialize_ui();
        let mut state = EventLoopState::new();

        loop {
            self.update_and_render(&mut ui, &mut state, terminal)?;

            if state.exit_requested {
                break;
            }

            self.handle_events(&mut ui, &mut state)?;
            self.check_shutdown_completion(&mut ui, &mut state);
        }

        terminal.draw(|frame| ui.render(frame))?;
        Ok(state.exit_requested)
    }

    /// Initialize the UI with version information and perform initial polling.
    fn initialize_ui(&mut self) -> Ui {
        let mut ui = Ui::default();
        ui.set_version(&self.version);
        self.poller.poll(&mut ui);
        ui
    }

    /// Update telemetry data and render the UI if needed.
    ///
    /// Renders when either new data is available (dirty flag) or when the refresh
    /// interval has elapsed since the last render.
    fn update_and_render<B: Backend>(
        &mut self,
        ui: &mut Ui,
        state: &mut EventLoopState,
        terminal: &mut Terminal<B>,
    ) -> anyhow::Result<()> {
        state.dirty |= self.poller.poll(ui);
        let should_render = state.dirty || state.last_render.elapsed() >= REFRESH_INTERVAL;

        if should_render {
            terminal.draw(|frame| ui.render(frame))?;
            state.last_render = Instant::now();
            state.dirty = false;
        }

        Ok(())
    }

    /// Process incoming terminal events (keyboard, mouse, resize).
    fn handle_events(&mut self, ui: &mut Ui, state: &mut EventLoopState) -> anyhow::Result<()> {
        if event::poll(POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) => {
                    self.handle_key_event(key, ui, state);
                }
                Event::Resize(_, _) => {
                    self.handle_resize_event(state);
                }
                Event::Mouse(mouse) => {
                    self.handle_mouse_event(mouse, ui, state);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Handle keyboard input, dispatching to appropriate handlers based on application state.
    fn handle_key_event(&mut self, key: event::KeyEvent, ui: &mut Ui, state: &mut EventLoopState) {
        let is_ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'));

        if ui.exit_prompt_visible() {
            self.handle_exit_prompt_keys(key, is_ctrl_c, ui, state);
            return;
        }

        if ui.is_shutting_down() {
            if is_ctrl_c {
                state.exit_requested = true;
            }
            return;
        }

        self.handle_normal_keys(key, is_ctrl_c, ui, state);
    }

    /// Handle keyboard input when the exit confirmation prompt is visible.
    fn handle_exit_prompt_keys(
        &mut self,
        key: event::KeyEvent,
        is_ctrl_c: bool,
        ui: &mut Ui,
        state: &mut EventLoopState,
    ) {
        if is_ctrl_c {
            state.exit_requested = true;
            return;
        }

        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                _ = ui.begin_shutdown();
                if !state.shutdown_initiated {
                    self.shutdown.cancel();
                    state.shutdown_initiated = true;
                }
                state.dirty = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                _ = ui.hide_exit_prompt();
                state.dirty = true;
            }
            _ => {}
        }
    }

    /// Handle keyboard input during normal operation (not in exit prompt or shutdown).
    fn handle_normal_keys(&mut self, key: event::KeyEvent, is_ctrl_c: bool, ui: &mut Ui, state: &mut EventLoopState) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                state.dirty |= ui.show_exit_prompt();
            }
            KeyCode::Char('1') => {
                ui.set_active_tab(Tab::Logs);
                state.dirty = true;
            }
            KeyCode::Char('2') => {
                ui.set_active_tab(Tab::Metrics);
                state.dirty = true;
            }
            KeyCode::Char('3') => {
                ui.set_active_tab(Tab::Traces);
                state.dirty = true;
            }
            KeyCode::Tab => {
                ui.focus_next_section();
                state.dirty = true;
            }
            KeyCode::BackTab => {
                ui.focus_previous_section();
                state.dirty = true;
            }
            KeyCode::Up => {
                ui.handle_vertical_navigation(-1);
                state.dirty = true;
            }
            KeyCode::Down => {
                ui.handle_vertical_navigation(1);
                state.dirty = true;
            }
            KeyCode::Char('c') | KeyCode::Char('C') if is_ctrl_c => state.exit_requested = true,
            _ => {}
        }
    }

    /// Handle terminal resize events by marking the UI as needing a refresh.
    fn handle_resize_event(&self, state: &mut EventLoopState) {
        state.dirty = true;
        let now = Instant::now();
        state.last_render = now.checked_sub(REFRESH_INTERVAL).unwrap_or(now);
    }

    /// Handle mouse events, currently only processing left clicks.
    fn handle_mouse_event(&self, mouse: event::MouseEvent, ui: &mut Ui, state: &mut EventLoopState) {
        let is_left_click = matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        );

        if is_left_click {
            state.dirty |= ui.handle_mouse_click(mouse.column, mouse.row);
        }
    }

    /// Check if shutdown is complete and exit if so.
    fn check_shutdown_completion(&self, ui: &mut Ui, state: &mut EventLoopState) {
        if state.shutdown_initiated && (ui.channel_closed() || ui.shutdown_complete()) {
            state.exit_requested = true;
        }
    }
}

/// Internal state for the event loop, tracking render timing and application lifecycle.
struct EventLoopState {
    /// Timestamp of the last UI render
    last_render: Instant,
    /// Whether the user has requested to exit the application
    exit_requested: bool,
    /// Whether shutdown has been initiated
    shutdown_initiated: bool,
    /// Whether the UI needs to be redrawn on the next render cycle
    dirty: bool,
}

impl EventLoopState {
    /// Create a new event loop state with default values.
    fn new() -> Self {
        Self {
            last_render: Instant::now(),
            exit_requested: false,
            shutdown_initiated: false,
            dirty: true,
        }
    }
}
