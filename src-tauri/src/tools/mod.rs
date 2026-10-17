//! Computer-control tools. Each tool declares its risk level; see SECURITY.md.

pub mod open_app;
#[cfg(windows)]
pub mod win;

use serde::Serialize;

#[allow(dead_code)] // Medium and High arrive with the confirmation flow (Phase 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}
