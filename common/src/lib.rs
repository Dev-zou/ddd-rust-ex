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
    },
    ReleaseResp {
        session_id: String,
    },
    QueryResp {
        session_id: String,
        resources: Vec<u16>,
    },
    TimeoutResourceInfo {
        session_id: String,
        timeout_resources: Vec<u16>,
    },
    HeartbeatResp {
        session_id: String,
    },
    ExitResp {
        session_id: String,
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