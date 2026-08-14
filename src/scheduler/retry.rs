//! Exponential backoff with jitter and circuit breaker policies for task scheduling and retries.

use std::time::{Duration, Instant};

/// Configuration for exponential backoff retries with jitter.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Initial delay before the first retry attempt.
    pub initial_delay: Duration,
    /// Maximum backoff ceiling.
    pub max_delay: Duration,
    /// Backoff multiplier (e.g. 2.0x).
    pub multiplier: f64,
    /// Upper bound for randomized jitter addition in milliseconds.
    pub jitter_max_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter_max_ms: 100,
        }
    }
}

impl RetryPolicy {
    /// Constructs a new retry policy.
    pub fn new(
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
        jitter_max_ms: u64,
    ) -> Self {
        Self {
            initial_delay,
            max_delay,
            multiplier: multiplier.max(1.0),
            jitter_max_ms,
        }
    }

    /// Calculates the delay duration for a given 1-based retry attempt.
    ///
    /// Applies exponential backoff ($initial\_delay \times multiplier^{(attempt - 1)}$),
    /// caps at `max_delay`, and adds uniform random jitter up to `jitter_max_ms`.
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let base_millis = (self.initial_delay.as_millis() as f64)
            * self.multiplier.powi(attempt.saturating_sub(1) as i32);
        let max_millis = self.max_delay.as_millis() as f64;
        let capped_millis = base_millis.min(max_millis);

        let jitter_ms = if self.jitter_max_ms > 0 {
            use rand::Rng;
            rand::thread_rng().gen_range(0..=self.jitter_max_ms)
        } else {
            0
        };

        Duration::from_millis(capped_millis as u64 + jitter_ms)
    }
}

/// Circuit Breaker to prevent tight retry loops and cascade failures when underlying services are down.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Number of consecutive failures before the circuit opens.
    pub failure_threshold: u32,
    /// Time window during which execution is paused when the circuit is open.
    pub cooldown: Duration,
    /// Count of current consecutive failures.
    pub consecutive_failures: u32,
    /// Timestamp when the circuit was opened.
    pub opened_at: Option<Instant>,
}

impl CircuitBreaker {
    /// Creates a new CircuitBreaker with specified failure threshold and cooldown.
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            failure_threshold,
            cooldown,
            consecutive_failures: 0,
            opened_at: None,
        }
    }

    /// Checks if execution is allowed under current circuit state.
    ///
    /// - `Closed`: allowed.
    /// - `Open`: disallowed until `cooldown` duration has elapsed since `opened_at`.
    /// - If cooldown has elapsed, transitions to probe mode (allowing execution).
    pub fn can_execute(&self) -> bool {
        match self.opened_at {
            None => true,
            Some(opened_at) => opened_at.elapsed() >= self.cooldown,
        }
    }

    /// Records a successful execution attempt, resetting failures and closing the circuit.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.opened_at = None;
    }

    /// Records a failed execution attempt.
    ///
    /// Increments failure counter. If consecutive failures reach or exceed `failure_threshold`,
    /// the circuit trips open. Returns `true` if this failure caused the circuit to open.
    pub fn record_failure(&mut self) -> bool {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.failure_threshold {
            let just_tripped = self.opened_at.is_none();
            self.opened_at = Some(Instant::now());
            just_tripped
        } else {
            false
        }
    }

    /// Returns whether the circuit is currently open and in cooldown.
    pub fn is_open(&self) -> bool {
        match self.opened_at {
            None => false,
            Some(opened_at) => opened_at.elapsed() < self.cooldown,
        }
    }

    /// Current circuit breaker state name: "closed", "open", or "half-open".
    pub fn state_name(&self) -> &'static str {
        match self.opened_at {
            None => "closed",
            Some(opened_at) => {
                if opened_at.elapsed() < self.cooldown {
                    "open"
                } else {
                    "half-open"
                }
            }
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(60))
    }
}
