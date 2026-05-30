pub struct ProgressTracker {
    total: usize,
    current: usize,
}

impl ProgressTracker {
    pub fn new(total: usize) -> Self {
        Self { total, current: 0 }
    }

    pub fn increment(&mut self) {
        self.current += 1;
        self.report();
    }

    pub fn set(&mut self, current: usize) {
        self.current = current;
        self.report();
    }

    fn report(&self) {
        let percent = (self.current as f64 / self.total as f64) * 100.0;
        log::info!(
            "Progress: {}/{} ({:.1}%)",
            self.current,
            self.total,
            percent
        );
    }
}
