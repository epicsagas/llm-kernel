//! Token budget tracking for LLM context windows.
//!
//! Provides a thread-safe token accounting type for managing context window
//! budgets without external dependencies.

use core::sync::atomic::{AtomicU32, Ordering};

/// Thread-safe token budget tracker for LLM context windows.
///
/// Tracks total capacity, current usage, and provides atomic reserve/release
/// operations for concurrent token accounting.
pub struct TokenBudget {
    /// Total token capacity.
    total: u32,
    /// Currently used tokens (atomic for thread safety).
    used: AtomicU32,
}

impl TokenBudget {
    /// Create a new budget with the given total token capacity.
    ///
    /// Initial usage is zero.
    pub fn new(total: u32) -> Self {
        Self {
            total,
            used: AtomicU32::new(0),
        }
    }

    /// Returns the total token capacity.
    pub fn total(&self) -> u32 {
        self.total
    }

    /// Returns the number of remaining tokens.
    pub fn remaining(&self) -> u32 {
        self.total.saturating_sub(self.used.load(Ordering::Relaxed))
    }

    /// Returns the number of currently used tokens.
    pub fn used(&self) -> u32 {
        self.used.load(Ordering::Relaxed)
    }

    /// Try to reserve `n` tokens from the budget.
    ///
    /// Returns `true` if the reservation succeeded (enough capacity remaining),
    /// `false` if there were insufficient tokens.
    pub fn try_reserve(&self, n: u32) -> bool {
        loop {
            let current = self.used.load(Ordering::Relaxed);
            let new_used = current.saturating_add(n);
            if new_used > self.total {
                return false;
            }
            match self.used.compare_exchange_weak(
                current,
                new_used,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(_) => continue,
            }
        }
    }

    /// Release `n` tokens back to the budget.
    ///
    /// Clamps to zero — cannot release more than currently used.
    pub fn release(&self, n: u32) {
        loop {
            let current = self.used.load(Ordering::Relaxed);
            let new_used = current.saturating_sub(n);
            match self.used.compare_exchange_weak(
                current,
                new_used,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(_) => continue,
            }
        }
    }

    /// Reset the budget, setting used tokens back to zero.
    pub fn reset(&self) {
        self.used.store(0, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for TokenBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenBudget")
            .field("total", &self.total)
            .field("used", &self.used())
            .field("remaining", &self.remaining())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_budget_starts_empty() {
        let b = TokenBudget::new(1000);
        assert_eq!(b.total(), 1000);
        assert_eq!(b.used(), 0);
        assert_eq!(b.remaining(), 1000);
    }

    #[test]
    fn reserve_success() {
        let b = TokenBudget::new(1000);
        assert!(b.try_reserve(600));
        assert_eq!(b.remaining(), 400);
        assert_eq!(b.used(), 600);
    }

    #[test]
    fn reserve_insufficient() {
        let b = TokenBudget::new(1000);
        assert!(b.try_reserve(600));
        assert!(!b.try_reserve(500));
        assert_eq!(b.remaining(), 400);
    }

    #[test]
    fn release_tokens() {
        let b = TokenBudget::new(1000);
        b.try_reserve(600);
        b.release(200);
        assert_eq!(b.remaining(), 600);
        assert_eq!(b.used(), 400);
    }

    #[test]
    fn release_clamps_to_zero() {
        let b = TokenBudget::new(1000);
        b.try_reserve(100);
        b.release(200);
        assert_eq!(b.used(), 0);
        assert_eq!(b.remaining(), 1000);
    }

    #[test]
    fn reset_clears_usage() {
        let b = TokenBudget::new(1000);
        b.try_reserve(500);
        b.reset();
        assert_eq!(b.used(), 0);
        assert_eq!(b.remaining(), 1000);
    }

    #[test]
    fn debug_format() {
        let b = TokenBudget::new(1000);
        b.try_reserve(300);
        let debug = format!("{:?}", b);
        assert!(debug.contains("total: 1000"));
        assert!(debug.contains("used: 300"));
        assert!(debug.contains("remaining: 700"));
    }

    #[test]
    fn reserve_exact_total() {
        let b = TokenBudget::new(100);
        assert!(b.try_reserve(100));
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn reserve_zero_is_noop() {
        let b = TokenBudget::new(100);
        assert!(b.try_reserve(0));
        assert_eq!(b.used(), 0);
    }
}
