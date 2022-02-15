/*
 * kernel_info_request.rs
 *
 * Copyright (C) 2022 by RStudio, PBC
 *
 */

use crate::wire::jupyter_message::MessageType;
use crate::wire::jupyter_message::SocketType;
use serde::{Deserialize, Serialize};

/// Represents request from the front end to the kernel to get information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KernelInfoRequest {}

impl MessageType for KernelInfoRequest {
    fn message_type() -> String {
        String::from("kernel_info_request")
    }
    fn socket_type() -> SocketType {
        SocketType::Shell
    }
}
