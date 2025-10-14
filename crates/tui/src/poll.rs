use tokio::sync::watch;

use crate::ui::{self, Ui};

pub struct Poller {
    pub status_rx: watch::Receiver<ui::UiStatus>,
    pub logs_rx: watch::Receiver<ui::LogsSnapshot>,
    pub metrics_rx: watch::Receiver<ui::MetricsSnapshot>,
    pub traces_rx: watch::Receiver<ui::TracesSnapshot>,
}

impl Poller {
    pub fn poll(&mut self, ui: &mut Ui) -> bool {
        let mut dirty = false;

        let status = self.status_rx.borrow().clone();
        dirty |= ui.update_status(&status);

        let logs = self.logs_rx.borrow().clone();
        dirty |= ui.update_logs(&logs);

        let metrics = self.metrics_rx.borrow().clone();
        dirty |= ui.update_metrics(&metrics);

        let traces = self.traces_rx.borrow().clone();
        dirty |= ui.update_traces(&traces);

        dirty
    }
}
