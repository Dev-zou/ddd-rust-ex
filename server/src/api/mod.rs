pub mod middlewares;
pub mod message;
pub mod server;
pub mod error;
pub mod message_handlers;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use std::{collections::HashMap, sync::Arc};

use crate::app::{resource_app::ResourceAppService, session_app::SessionAppService};
