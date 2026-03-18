use std::collections::VecDeque;

pub struct PerformanceTracker {
    pub server_history: VecDeque<f32>,
    pub received_history: VecDeque<f32>,
    pub render_history: VecDeque<f32>,
    pub max_samples: usize,
}

impl PerformanceTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            server_history: VecDeque::with_capacity(max_samples),
            received_history: VecDeque::with_capacity(max_samples),
            render_history: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn add_samples(&mut self, server: f32, received: f32, render: f32) {
        if self.server_history.len() >= self.max_samples {
            self.server_history.pop_front();
            self.received_history.pop_front();
            self.render_history.pop_front();
        }
        self.server_history.push_back(server);
        self.received_history.push_back(received);
        self.render_history.push_back(render);
    }
}
