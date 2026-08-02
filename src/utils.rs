use std::time::Duration;
use std::thread::sleep;

pub fn delay() {
    sleep(Duration::from_millis(100));
}
