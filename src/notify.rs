use notify_rust::Notification;

pub fn send_message(message: &str) {
    Notification::new()
        .summary("Received image from copy-sync")
        .body(message)
        .show()
        .unwrap();
}
