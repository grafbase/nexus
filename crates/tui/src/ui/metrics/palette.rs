use ratatui::style::Color;

/// Aggregates palette references for the metrics dashboard.
#[derive(Clone, Copy)]
pub(crate) struct PaletteBundle {
    /// Color configuration for the operation-latency chart.
    pub(crate) operation: &'static LatencyPalette,
    /// Color configuration for the time-to-first-token chart.
    pub(crate) ttft: &'static LatencyPalette,
    /// Color configuration for the token flow chart.
    pub(crate) flow: &'static FlowPalette,
    /// Color accents for the model usage table.
    pub(crate) model: &'static ModelPalette,
}

impl PaletteBundle {
    /// Build a palette bundle pointing at the statically-defined color sets.
    pub(crate) fn new() -> Self {
        Self {
            operation: &OPERATION_PALETTE,
            ttft: &TTFT_PALETTE,
            flow: &FLOW_PALETTE,
            model: &MODEL_PALETTE,
        }
    }
}

impl Default for PaletteBundle {
    fn default() -> Self {
        Self::new()
    }
}

/// Color palette used by latency charts.
pub(crate) struct LatencyPalette {
    pub(crate) border: Color,
    pub(crate) title: Color,
    pub(crate) axis: Color,
    pub(crate) series: [Color; 3],
}

/// Colors for the token flow chart lines and chrome.
pub(crate) struct FlowPalette {
    pub(crate) border: Color,
    pub(crate) title: Color,
    pub(crate) axis: Color,
    pub(crate) input: Color,
    pub(crate) output: Color,
}

/// Color accents for the token usage table.
pub(crate) struct ModelPalette {
    pub(crate) border: Color,
    pub(crate) title: Color,
    pub(crate) label: Color,
}

pub(crate) const OPERATION_PALETTE: LatencyPalette = LatencyPalette {
    border: Color::Rgb(82, 110, 173),
    title: Color::Rgb(189, 208, 255),
    axis: Color::Rgb(125, 138, 170),
    series: [
        Color::Rgb(252, 214, 87),
        Color::Rgb(255, 163, 102),
        Color::Rgb(244, 110, 196),
    ],
};

pub(crate) const TTFT_PALETTE: LatencyPalette = LatencyPalette {
    border: Color::Rgb(65, 145, 148),
    title: Color::Rgb(178, 246, 217),
    axis: Color::Rgb(110, 160, 160),
    series: [
        Color::Rgb(108, 220, 255),
        Color::Rgb(74, 207, 171),
        Color::Rgb(152, 232, 95),
    ],
};

pub(crate) const FLOW_PALETTE: FlowPalette = FlowPalette {
    border: Color::Rgb(124, 86, 166),
    title: Color::Rgb(219, 186, 255),
    axis: Color::Rgb(135, 108, 168),
    input: Color::Rgb(166, 99, 255),
    output: Color::Rgb(255, 118, 189),
};

pub(crate) const MODEL_PALETTE: ModelPalette = ModelPalette {
    border: Color::Rgb(92, 142, 130),
    title: Color::Rgb(174, 236, 220),
    label: Color::Rgb(210, 222, 255),
};
