//! Toy API for PCPN Synthesizer Testing
//!
//! This crate provides a minimal API to test the Pushdown CPN synthesizer.

/// A simple counter struct
pub struct Counter {
    value: i32,
}

/// 0-ary const producer - creates a new Counter
pub const fn make_counter() -> Counter {
    Counter { value: 0 }
}

impl Counter {
    /// 0-ary const producer - associated function
    pub const fn new() -> Self {
        Counter { value: 0 }
    }

    /// Creates a counter with an initial value (takes owned i32)
    pub fn with_value(val: i32) -> Self {
        Counter { value: val }
    }

    /// Requires &mut self - increments the counter
    pub fn inc(&mut self) {
        self.value += 1;
    }

    /// Requires &mut self - decrements the counter
    pub fn dec(&mut self) {
        self.value -= 1;
    }

    /// Requires &mut self - resets to zero
    pub fn reset(&mut self) {
        self.value = 0;
    }

    /// Requires &self - returns the current value (Copy type)
    pub fn get(&self) -> i32 {
        self.value
    }

    /// Requires &self - returns a reference to the value
    pub fn get_ref(&self) -> &i32 {
        &self.value
    }

    /// Requires &mut self - returns a mutable reference
    pub fn get_mut(&mut self) -> &mut i32 {
        &mut self.value
    }

    /// Consumes self, returns the value
    pub fn into_value(self) -> i32 {
        self.value
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// A wrapper that holds a value
pub struct Wrapper<T> {
    inner: T,
}

impl<T> Wrapper<T> {
    /// 0-ary producer (when T: Default)
    pub fn new(value: T) -> Self {
        Wrapper { inner: value }
    }

    /// Get shared reference to inner
    pub fn get(&self) -> &T {
        &self.inner
    }

    /// Get mutable reference to inner
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Unwrap the value
    pub fn into_inner(self) -> T {
        self.inner
    }
}

/// Creates a pair of values
pub fn pair<A, B>(a: A, b: B) -> (A, B) {
    (a, b)
}

/// Identity function
pub fn identity<T>(x: T) -> T {
    x
}

/// Clone a value (requires Clone bound)
pub fn duplicate<T: Clone>(x: &T) -> T {
    x.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let mut c = make_counter();
        assert_eq!(c.get(), 0);
        c.inc();
        assert_eq!(c.get(), 1);
    }

    #[test]
    fn test_wrapper() {
        let mut w = Wrapper::new(42);
        assert_eq!(*w.get(), 42);
        *w.get_mut() = 100;
        assert_eq!(w.into_inner(), 100);
    }
}
