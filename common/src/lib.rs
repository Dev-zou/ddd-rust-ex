use serde::{Deserialize, Serialize};

/// 请求消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RequestMessage {
    Allocate {
        session_id: String,
        resources: Vec<u16>,
    },
    Release {
        session_id: String,
        resources: Vec<u16>,
    },
    Query {
        session_id: String,
    },
    Heartbeat {
        session_id: String,
    },
    Exit {
        session_id: String,
    },
    AsyncAllocate {
        session_id: String,
        resources: Vec<u16>,
    },
    AsyncRelease {
        session_id: String,
        resources: Vec<u16>,
    },
}

/// 响应消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ResponseMessage {
    SessionIdResp {
        session_id: String,
    },
    AllocateResp {
        session_id: String,
        success_resources: Vec<u16>,
        failed_resources: Vec<(u16, String)>, // (resource_id, error_reason)
        error_code: u32,
    },
    ReleaseResp {
        session_id: String,
        success_resources: Vec<u16>,
        failed_resources: Vec<(u16, String)>, // (resource_id, error_reason)
        error_code: u32,
    },
    QueryResp {
        session_id: String,
        resources: Vec<u16>,
        error_code: u32,
    },
    TimeoutResourceInfo {
        session_id: String,
        timeout_resources: Vec<u16>,
    },
    HeartbeatResp {
        session_id: String,
        error_code: u32,
    },
    ExitResp {
        session_id: String,
    },
    AsyncAllocateResp {
        session_id: String,
        error_code: u32,
    },
    AsyncReleaseResp {
        session_id: String,
        error_code: u32,
    },
    AsyncAllocateResult {
        session_id: String,
        success_resources: Vec<u16>,
        failed_resources: Vec<(u16, String)>, // (resource_id, error_reason)
        error_code: u32,
    },
    AsyncReleaseResupt {
        session_id: String,
        success_resources: Vec<u16>,
        failed_resources: Vec<(u16, String)>, // (resource_id, error_reason)
        error_code: u32,
    },
}

/// WebSocket消息包装
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage<T> {
    pub id: Option<String>,
    pub data: T,
}

impl<T> WsMessage<T> {
    pub fn new(id: Option<String>, data: T) -> Self {
        Self {
            id,
            data,
        }
    }
}