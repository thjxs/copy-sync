use notify_rust::Notification;

pub fn notify(message: &str) {
    Notification::new()
        .summary("Received image from copy-sync")
        .body(message)
        .show()
        .unwrap();
}
