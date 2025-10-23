//! Rendering layer for the Nexus TUI.
//!
//! The top-level `Ui` type coordinates shared chrome like the tab strip and
//! delegates each tab to its dedicated module. Splitting the implementation
//! keeps metrics, traces, and logs concerns isolated while still sharing a
//! consistent visual theme.

use ratatui::{
    Frame,
    prelude::{Alignment, Constraint, Direction, Layout, Line, Margin, Modifier, Rect, Style},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};

mod logs;
mod metrics;
mod snapshots;
mod traces;

pub(crate) use metrics::seconds_since;
pub(crate) use snapshots::*;

use self::{logs::Logs, metrics::Metrics, traces::Traces};

pub(crate) const PANEL_BACKGROUND: ratatui::style::Color = ratatui::style::Color::Rgb(0, 0, 0);
pub(crate) const PANEL_BORDER_DIM: ratatui::style::Color = ratatui::style::Color::Rgb(73, 84, 105);
pub(crate) const PANEL_BORDER_ACTIVE: ratatui::style::Color = ratatui::style::Color::Rgb(139, 168, 255);
pub(crate) const TEXT_PRIMARY: ratatui::style::Color = ratatui::style::Color::Rgb(210, 222, 255);
pub(crate) const TEXT_MUTED: ratatui::style::Color = ratatui::style::Color::Rgb(150, 160, 185);
pub(crate) const TEXT_ACCENT: ratatui::style::Color = ratatui::style::Color::Rgb(189, 208, 255);
pub(crate) const SELECTION_BG: ratatui::style::Color = ratatui::style::Color::Rgb(32, 38, 56);
pub(crate) const SELECTION_FG: ratatui::style::Color = ratatui::style::Color::Rgb(252, 214, 87);
pub(crate) const TIMESTAMP_COLOR: ratatui::style::Color = ratatui::style::Color::Rgb(125, 138, 170);
pub(crate) const TRACE_ROOT_COLOR: ratatui::style::Color = ratatui::style::Color::Rgb(255, 163, 102);
pub(crate) const TRACE_CHILD_COLOR: ratatui::style::Color = ratatui::style::Color::Rgb(108, 220, 255);

pub(crate) const TRACE_TIMESTAMP_FORMAT: &[time::format_description::FormatItem<'static>] =
    time::macros::format_description!("[hour]:[minute]:[second]");

#[derive(Copy, Default, Clone, Eq, PartialEq)]
/// Tabs across the top of the UI, mapping to different telemetry surfaces.
pub(crate) enum Tab {
    #[default]
    Logs,
    Metrics,
    Traces,
}

impl Tab {
    /// Ordered list of all available tabs for iterating or indexing.
    pub(crate) const ALL: [Tab; 3] = [Tab::Logs, Tab::Metrics, Tab::Traces];

    /// Return the integer index used by the tab widget.
    pub(crate) fn index(self) -> usize {
        match self {
            Tab::Logs => 0,
            Tab::Metrics => 1,
            Tab::Traces => 2,
        }
    }

    /// Human-readable label shown in the tab strip.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Tab::Logs => "Logs [1]",
            Tab::Metrics => "Metrics [2]",
            Tab::Traces => "Traces [3]",
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Default)]
enum ExitOverlay {
    #[default]
    Hidden,
    Prompt,
    ShuttingDown,
}

/// Overall state holder and rendering façade for the terminal UI.
pub(crate) struct Ui {
    active_tab: Tab,
    tab_hitboxes: [Rect; Tab::ALL.len()],
    app_title: String,
    metrics: Metrics,
    logs: Logs,
    traces: Traces,
    status: UiStatus,
    logs_snapshot: LogsSnapshot,
    metrics_snapshot: MetricsSnapshot,
    traces_snapshot: TracesSnapshot,
    exit_overlay: ExitOverlay,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            active_tab: Tab::default(),
            tab_hitboxes: [Rect::default(); Tab::ALL.len()],
            app_title: "Nexus".to_string(),
            metrics: Metrics,
            logs: Logs,
            traces: Traces::default(),
            status: UiStatus::default(),
            logs_snapshot: LogsSnapshot::default(),
            metrics_snapshot: MetricsSnapshot::default(),
            traces_snapshot: TracesSnapshot::default(),
            exit_overlay: ExitOverlay::Hidden,
        }
    }
}

impl Ui {
    /// Draw the current frame by delegating to the active tab.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>) {
        let size = frame.area();

        if self.status.has_initialized {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(size);

            self.render_tabs(frame, layout[0]);

            match self.active_tab {
                Tab::Logs => self.logs.render(&self.logs_snapshot, frame, layout[1]),
                Tab::Metrics => self.metrics.render(&self.metrics_snapshot, frame, layout[1]),
                Tab::Traces => self.traces.render(&self.traces_snapshot, frame, layout[1]),
            }
        } else {
            self.render_loading(frame, size);
        }

        match self.exit_overlay {
            ExitOverlay::Prompt => self.render_exit_prompt(frame, size),
            ExitOverlay::ShuttingDown => self.render_shutdown_notice(frame, size),
            ExitOverlay::Hidden => {}
        }
    }

    /// Switch to a specific tab and perform any tab-specific activation.
    pub(crate) fn set_active_tab(&mut self, tab: Tab) {
        self.active_tab = tab;

        if tab == Tab::Traces {
            self.traces.activate(&self.traces_snapshot);
        }
    }

    /// Advance focus to the next interactive sub-section within the active tab.
    pub(crate) fn focus_next_section(&mut self) {
        if self.active_tab == Tab::Traces {
            self.traces.focus_next_section();
        }
    }

    /// Move focus to the previous interactive sub-section.
    pub(crate) fn focus_previous_section(&mut self) {
        if self.active_tab == Tab::Traces {
            self.traces.focus_previous_section();
        }
    }

    /// Handle keyboard navigation shortcuts that scroll lists or tables.
    pub(crate) fn handle_vertical_navigation(&mut self, delta: isize) {
        if self.active_tab == Tab::Traces {
            self.traces.handle_vertical_navigation(&self.traces_snapshot, delta);
        }
    }

    /// Dispatch mouse clicks to either the global tab strip or the active tab.
    pub(crate) fn handle_mouse_click(&mut self, column: u16, row: u16) -> bool {
        if self.exit_overlay != ExitOverlay::Hidden {
            return false;
        }

        if self.try_handle_tab_click(column, row) {
            return true;
        }

        if self.active_tab == Tab::Traces {
            self.traces.handle_mouse_click(&self.traces_snapshot, column, row);
            return true;
        }

        false
    }

    /// Update the application version displayed in the chrome.
    pub(crate) fn set_version(&mut self, version: &str) {
        let trimmed = version.trim();
        self.app_title = format!("Nexus {trimmed}");
    }

    /// Update cached status metadata.
    pub(crate) fn update_status(&mut self, status: &UiStatus) -> bool {
        if status.epoch > self.status.epoch {
            self.status = status.clone();
            true
        } else {
            false
        }
    }

    /// Update logs snapshot when a newer epoch arrives.
    pub(crate) fn update_logs(&mut self, snapshot: &LogsSnapshot) -> bool {
        if snapshot.epoch > self.logs_snapshot.epoch {
            self.logs_snapshot = snapshot.clone();
            true
        } else {
            false
        }
    }

    /// Update metrics snapshot and mark dirtiness if epoch advanced.
    pub(crate) fn update_metrics(&mut self, snapshot: &MetricsSnapshot) -> bool {
        if snapshot.epoch > self.metrics_snapshot.epoch {
            self.metrics_snapshot = snapshot.clone();
            true
        } else {
            false
        }
    }

    /// Update traces snapshot and refresh trace view state.
    pub(crate) fn update_traces(&mut self, snapshot: &TracesSnapshot) -> bool {
        if snapshot.epoch > self.traces_snapshot.epoch {
            self.traces_snapshot = snapshot.clone();
            self.traces.on_snapshot_changed(&self.traces_snapshot);
            true
        } else {
            false
        }
    }

    /// Draw the Nexus title bar and tab headers.
    fn render_tabs(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let titles = Tab::ALL.iter().map(|tab| Line::from(tab.title())).collect::<Vec<_>>();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL_BORDER_ACTIVE))
            .title(self.app_title.as_str())
            .style(Style::default().bg(PANEL_BACKGROUND));

        let tabs = Tabs::new(titles.clone())
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(SELECTION_FG)
                    .bg(PANEL_BACKGROUND)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
            .select(self.active_tab.index());

        frame.render_widget(tabs, area);
        self.tab_hitboxes = compute_tab_hitboxes(area, &titles);
    }

    /// Display the loading placeholder while waiting for telemetry.
    fn render_loading(&self, frame: &mut Frame<'_>, area: Rect) {
        let message = if self.status.channel_closed {
            "Telemetry stream closed before any data arrived"
        } else {
            "Starting Nexus… waiting for telemetry"
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .title("Nexus TUI")
            .title_style(Style::default().fg(TEXT_ACCENT))
            .style(Style::default().bg(PANEL_BACKGROUND));

        let paragraph = Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .block(block);

        frame.render_widget(paragraph, area);
    }

    /// Switch tabs in response to a mouse click on the tab strip.
    fn try_handle_tab_click(&mut self, column: u16, row: u16) -> bool {
        for (idx, rect) in self.tab_hitboxes.iter().enumerate() {
            if rect.is_empty() || !contains_point(*rect, column, row) {
                continue;
            }

            let new_tab = Tab::ALL[idx];
            if new_tab != self.active_tab {
                self.set_active_tab(new_tab);
                return true;
            }
            return false;
        }

        false
    }

    /// Indicate whether the exit confirmation prompt is currently visible.
    pub(crate) fn exit_prompt_visible(&self) -> bool {
        self.exit_overlay == ExitOverlay::Prompt
    }

    /// Whether the shutdown notice overlay is visible.
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.exit_overlay == ExitOverlay::ShuttingDown
    }

    /// Whether the telemetry channel has closed.
    pub(crate) fn channel_closed(&self) -> bool {
        self.status.channel_closed
    }

    /// Whether the server has confirmed shutdown in logs.
    pub(crate) fn shutdown_complete(&self) -> bool {
        self.status.shutdown_complete
    }

    /// Display the exit confirmation prompt. Returns true when state changed.
    pub(crate) fn show_exit_prompt(&mut self) -> bool {
        if self.exit_overlay == ExitOverlay::Prompt {
            false
        } else {
            self.exit_overlay = ExitOverlay::Prompt;
            true
        }
    }

    /// Dismiss the exit confirmation prompt. Returns true when state changed.
    pub(crate) fn hide_exit_prompt(&mut self) -> bool {
        if self.exit_overlay == ExitOverlay::Prompt {
            self.exit_overlay = ExitOverlay::Hidden;
            true
        } else {
            false
        }
    }

    /// Transition to the shutdown notice overlay.
    pub(crate) fn begin_shutdown(&mut self) -> bool {
        if self.exit_overlay == ExitOverlay::ShuttingDown {
            false
        } else {
            self.exit_overlay = ExitOverlay::ShuttingDown;
            true
        }
    }

    fn render_exit_prompt(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let width = if area.width >= 20 {
            area.width.min(50)
        } else {
            area.width
        };
        let height = if area.height >= 5 {
            area.height.min(7)
        } else {
            area.height
        };

        if width == 0 || height == 0 {
            return;
        }
        let popup_x = area.x + (area.width.saturating_sub(width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup = Rect::new(popup_x, popup_y, width, height);

        frame.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL_BORDER_ACTIVE))
            .title("Confirm exit")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(PANEL_BACKGROUND));

        let text = vec![
            Line::from("Are you sure you want to quit?"),
            Line::from("Press y to confirm, n to stay"),
        ];

        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .block(block);

        frame.render_widget(paragraph, popup);
    }

    fn render_shutdown_notice(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let width = if area.width >= 20 {
            area.width.min(60)
        } else {
            area.width
        };
        let height = if area.height >= 5 {
            area.height.min(7)
        } else {
            area.height
        };

        if width == 0 || height == 0 {
            return;
        }

        let popup_x = area.x + (area.width.saturating_sub(width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup = Rect::new(popup_x, popup_y, width, height);

        frame.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL_BORDER_ACTIVE))
            .title("Shutting down")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(PANEL_BACKGROUND));

        let text = vec![
            Line::from("Attempting a clean shutdown…"),
            Line::from("Please wait until Nexus has exited."),
        ];

        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .block(block);

        frame.render_widget(paragraph, popup);
    }
}

pub(super) fn contains_point(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.x + area.width && row >= area.y && row < area.y + area.height
}

fn compute_tab_hitboxes(area: Rect, titles: &[Line<'_>]) -> [Rect; Tab::ALL.len()] {
    let mut hitboxes = [Rect::default(); Tab::ALL.len()];
    let mut populated = 0;

    let inner = area.inner(Margin::new(1, 1));
    let target = if inner.width > 0 && inner.height > 0 {
        inner
    } else {
        area
    };

    if target.width == 0 || target.height == 0 {
        return hitboxes;
    }

    let mut cursor_x = target.x;
    let right_edge = target.x.saturating_add(target.width);

    for (idx, title) in titles.iter().enumerate() {
        if cursor_x >= right_edge || populated >= hitboxes.len() {
            break;
        }

        let tab_start = cursor_x;
        let mut tab_width: u16 = 0;

        let remaining_width = right_edge.saturating_sub(cursor_x);
        if remaining_width == 0 {
            break;
        }

        let pad_left = remaining_width.min(1);
        cursor_x = cursor_x.saturating_add(pad_left);
        tab_width = tab_width.saturating_add(pad_left);

        let remaining_after_left = right_edge.saturating_sub(cursor_x);
        let title_width = title.width().min(usize::from(remaining_after_left));
        let title_width = u16::try_from(title_width).unwrap_or(u16::MAX);

        cursor_x = cursor_x.saturating_add(title_width);
        tab_width = tab_width.saturating_add(title_width);

        let remaining_after_title = right_edge.saturating_sub(cursor_x);
        let pad_right = remaining_after_title.min(1);
        cursor_x = cursor_x.saturating_add(pad_right);
        tab_width = tab_width.saturating_add(pad_right);

        if tab_width > 0 {
            hitboxes[idx] = Rect::new(tab_start, target.y, tab_width, target.height);
            populated += 1;
        }

        if idx + 1 < titles.len() {
            let remaining_post = right_edge.saturating_sub(cursor_x);
            let divider = remaining_post.min(1);
            cursor_x = cursor_x.saturating_add(divider);
        }
    }

    hitboxes
}
