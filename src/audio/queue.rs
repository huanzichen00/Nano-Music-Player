use super::Repeat;
use rand::seq::SliceRandom;

#[derive(Default, Debug)]
pub struct Queue {
    pub order: Vec<usize>,
    pub position: usize,
    canonical_order: Vec<usize>,
}

impl Queue {
    pub fn set(&mut self, tracks: Vec<usize>, start: usize, shuffle: bool) {
        self.canonical_order = tracks.clone();
        self.order = tracks;
        self.position = self.order.iter().position(|id| *id == start).unwrap_or(0);
        if shuffle {
            self.shuffle();
        }
    }

    pub fn current(&self) -> Option<usize> {
        self.order.get(self.position).copied()
    }

    pub fn advance(&mut self, repeat: Repeat, manual: bool) -> Option<usize> {
        if self.order.is_empty() {
            return None;
        }
        if repeat == Repeat::One && !manual {
            return self.current();
        }
        if self.position + 1 < self.order.len() {
            self.position += 1;
        } else if manual || repeat == Repeat::All {
            self.position = 0;
        } else {
            return None;
        }
        self.current()
    }

    pub fn previous(&mut self) -> Option<usize> {
        if self.order.is_empty() {
            return None;
        }
        self.position = if self.position == 0 {
            self.order.len() - 1
        } else {
            self.position - 1
        };
        self.current()
    }

    pub fn shuffle(&mut self) {
        let current = self.current();
        self.order = self.canonical_order.clone();
        self.order.shuffle(&mut rand::rng());
        if let Some(current) = current {
            self.position = self.order.iter().position(|id| *id == current).unwrap_or(0);
            // Start the shuffled run with the selected song so Repeat Off still visits every song.
            self.order.rotate_left(self.position);
            self.position = 0;
        }
    }

    pub fn unshuffle(&mut self) {
        let current = self.current();
        self.order = self.canonical_order.clone();
        if let Some(current) = current {
            self.position = self.order.iter().position(|id| *id == current).unwrap_or(0);
        }
    }
}
