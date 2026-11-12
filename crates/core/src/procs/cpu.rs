//! CPU use and rates from two readings, the way Task Manager counts them.

/// A process's share of the whole machine between two readings, 0 to 100: the CPU time it used
/// (kernel + user, in 100 ns steps) over the time that passed times the number of cores. So a
/// process keeping one of 8 cores busy shows 12.5, and the column adds up to the total.
pub fn process_percent(before: i64, after: i64, elapsed_100ns: i64, cores: u32) -> f32 {
    if elapsed_100ns <= 0 || cores == 0 || after < before {
        return 0.0;
    }
    let share = (after - before) as f64 / (elapsed_100ns as f64 * cores as f64);
    (share * 100.0).clamp(0.0, 100.0) as f32
}

/// Times a core (or the whole machine) spent, in 100 ns steps. `kernel` includes `idle`, as
/// Windows reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Times {
    pub idle: i64,
    pub kernel: i64,
    pub user: i64,
}

impl Times {
    fn total(&self) -> i64 {
        self.kernel + self.user
    }
}

/// How busy a core was between two readings, 0 to 100.
pub fn busy_percent(before: Times, after: Times) -> f32 {
    let total = after.total() - before.total();
    let idle = after.idle - before.idle;
    if total <= 0 {
        return 0.0;
    }
    ((total - idle.clamp(0, total)) as f64 / total as f64 * 100.0) as f32
}

/// Each core's busy share, and the machine's (every core's time added up).
pub fn cores_percent(before: &[Times], after: &[Times]) -> (f32, Vec<f32>) {
    if before.len() != after.len() {
        return (0.0, vec![0.0; after.len()]);
    }
    let cores = before.iter().zip(after).map(|(b, a)| busy_percent(*b, *a)).collect();
    let sum = |t: &[Times]| {
        t.iter().fold(Times::default(), |s, c| Times {
            idle: s.idle + c.idle,
            kernel: s.kernel + c.kernel,
            user: s.user + c.user,
        })
    };
    (busy_percent(sum(before), sum(after)), cores)
}

/// Bytes per second from two running totals. A total that went down (a counter that restarted)
/// gives zero, not a huge number.
pub fn rate(before: u64, after: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        return 0.0;
    }
    after.saturating_sub(before) as f64 / secs
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: i64 = 10_000_000;

    #[test]
    fn one_busy_core_of_eight_is_an_eighth() {
        assert_eq!(process_percent(0, SECOND, SECOND, 8), 12.5);
        assert_eq!(process_percent(5, 5, SECOND, 8), 0.0);
    }

    #[test]
    fn process_percent_stays_in_range() {
        // A reading taken a little late can count more than the elapsed time.
        assert_eq!(process_percent(0, 20 * SECOND, SECOND, 8), 100.0);
        assert_eq!(process_percent(10, 5, SECOND, 8), 0.0, "time going back");
        assert_eq!(process_percent(0, 5, 0, 8), 0.0, "no time passed");
        assert_eq!(process_percent(0, 5, SECOND, 0), 0.0);
    }

    #[test]
    fn busy_is_what_was_not_idle() {
        let a = Times { idle: 0, kernel: 0, user: 0 };
        // 1 s passed: 0.75 s idle (counted in kernel), 0.1 s kernel work, 0.15 s user.
        let b = Times { idle: 7_500_000, kernel: 8_500_000, user: 1_500_000 };
        assert!((busy_percent(a, b) - 25.0).abs() < 1e-4);
        assert_eq!(busy_percent(b, b), 0.0);
    }

    #[test]
    fn the_total_weighs_every_core() {
        let zero = [Times::default(); 2];
        let after = [
            Times { idle: 0, kernel: SECOND, user: 0 },
            Times { idle: SECOND, kernel: SECOND, user: 0 },
        ];
        let (total, cores) = cores_percent(&zero, &after);
        assert_eq!(cores, vec![100.0, 0.0]);
        assert_eq!(total, 50.0);
        let (_, mismatched) = cores_percent(&zero[..1], &after);
        assert_eq!(mismatched, vec![0.0, 0.0]);
    }

    #[test]
    fn rates_never_go_negative() {
        assert_eq!(rate(1000, 3000, 2.0), 1000.0);
        assert_eq!(rate(3000, 1000, 1.0), 0.0);
        assert_eq!(rate(0, 1000, 0.0), 0.0);
    }
}
