//! The last minute of a value, for the graphs: a fixed ring, oldest overwritten first.

/// How many readings a graph shows: one minute at one reading a second.
pub const LEN: usize = 60;

#[derive(Clone, Debug)]
pub struct Ring {
    values: [f32; LEN],
    /// Where the next value goes.
    next: usize,
    len: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Self { values: [0.0; LEN], next: 0, len: 0 }
    }
}

impl Ring {
    pub fn push(&mut self, v: f32) {
        self.values[self.next] = if v.is_finite() { v } else { 0.0 };
        self.next = (self.next + 1) % LEN;
        self.len = (self.len + 1).min(LEN);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The values, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        let start = (self.next + LEN - self.len) % LEN;
        (0..self.len).map(move |i| self.values[(start + i) % LEN])
    }

    pub fn last(&self) -> Option<f32> {
        (self.len > 0).then(|| self.values[(self.next + LEN - 1) % LEN])
    }

    /// The largest value held, or 0.
    pub fn max(&self) -> f32 {
        self.iter().fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_order_values_came_in() {
        let mut r = Ring::default();
        assert!(r.is_empty() && r.last().is_none());
        for v in [1.0, 2.0, 3.0] {
            r.push(v);
        }
        assert_eq!(r.iter().collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
        assert_eq!(r.last(), Some(3.0));
        assert_eq!(r.max(), 3.0);
    }

    #[test]
    fn drops_the_oldest_when_full() {
        let mut r = Ring::default();
        for v in 0..(LEN + 5) {
            r.push(v as f32);
        }
        assert_eq!(r.len(), LEN);
        let all: Vec<f32> = r.iter().collect();
        assert_eq!(all.first(), Some(&5.0));
        assert_eq!(all.last(), Some(&((LEN + 4) as f32)));
    }

    #[test]
    fn a_bad_value_is_zero() {
        let mut r = Ring::default();
        r.push(f32::NAN);
        assert_eq!(r.last(), Some(0.0));
    }
}
