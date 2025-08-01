pub mod middlewares;
pub mod message;
pub mod server;
pub mod error;
pub mod message_handlers;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use std::{collections::HashMap, sync::Arc};

use crate::app::{resource_app::ResourceAppService, session_app::SessionAppService};

// #[derive(Clone)]
// /// API服务核心结构体（保存App层实例和活跃连接）
// pub struct ApiServer {
//     session_app: Arc<SessionAppService>,
//     resource_app: Arc<ResourceAppService>,
//     connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
// }
