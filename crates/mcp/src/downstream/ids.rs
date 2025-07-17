use std::ops::Index;

use rmcp::model::Tool;

use super::Downstream;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct ToolId(u64);

impl From<usize> for ToolId {
    fn from(id: usize) -> Self {
        ToolId(id as u64)
    }
}

impl From<ToolId> for usize {
    fn from(id: ToolId) -> Self {
        id.0 as usize
    }
}

impl From<u64> for ToolId {
    fn from(id: u64) -> Self {
        ToolId(id)
    }
}

impl From<ToolId> for u64 {
    fn from(id: ToolId) -> Self {
        id.0
    }
}

impl Index<ToolId> for Downstream {
    type Output = Tool;

    fn index(&self, index: ToolId) -> &Self::Output {
        &self.tools[index.0 as usize]
    }
}
