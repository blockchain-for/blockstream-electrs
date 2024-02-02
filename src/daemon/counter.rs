use std::sync::Mutex;

pub struct Counter {
    value: Mutex<u64>,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            value: Mutex::new(0),
        }
    }
}

impl Counter {
    pub fn next(&self) -> u64 {
        let mut value = self.value.lock().unwrap();
        *value += 1;

        *value
    }
}
