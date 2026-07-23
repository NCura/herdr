use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct PingParams {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServerLiveHandoffParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_exe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_protocol: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServerCapabilities {
    pub live_handoff: bool,
    #[serde(default)]
    pub detached_server_daemon: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_observation: Option<TerminalObservationCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TerminalObservationCapabilities {
    pub exact_pane: bool,
    pub read_only: bool,
    pub view_only_dimensions: bool,
    /// Omitted exact-pane dimensions use the current pane runtime geometry.
    #[serde(default)]
    pub native_dimensions: bool,
    pub full_frame_first: bool,
    pub bounded_frames: bool,
    pub coalesced_updates: bool,
    pub pty_survives_disconnect: bool,
    /// Maximum accepted presentation viewport width in columns.
    pub max_cols: u16,
    /// Maximum accepted presentation viewport height in rows.
    pub max_rows: u16,
    pub max_cells: u32,
    pub max_frame_bytes: u32,
    pub max_ansi_bytes: u32,
    pub max_record_bytes: u32,
}

impl TerminalObservationCapabilities {
    pub fn current() -> Self {
        Self {
            exact_pane: true,
            read_only: true,
            view_only_dimensions: true,
            native_dimensions: true,
            full_frame_first: true,
            bounded_frames: true,
            coalesced_updates: true,
            pty_survives_disconnect: true,
            max_cols: crate::protocol::MAX_OBSERVATION_COLS,
            max_rows: crate::protocol::MAX_OBSERVATION_ROWS,
            max_cells: crate::protocol::MAX_OBSERVATION_CELLS as u32,
            max_frame_bytes: crate::protocol::MAX_FRAME_SIZE as u32,
            max_ansi_bytes: crate::protocol::MAX_OBSERVATION_ANSI_BYTES as u32,
            max_record_bytes: crate::protocol::MAX_OBSERVATION_NDJSON_RECORD_SIZE as u32,
        }
    }
}
