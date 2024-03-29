use tungstenite::protocol::WebSocketConfig;

pub const WEB_SOCKET_CONFIG: WebSocketConfig = WebSocketConfig {
    max_send_queue: None,
    max_message_size: None,
    max_frame_size: None,
    accept_unmasked_frames: false,
};

pub const RETRY_CONNECT_INTERVAL_IN_SECONDS: u64 = 60;
