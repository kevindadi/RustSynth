//! Simple lifetime example demonstrating Rust's borrowing rules

/// A simple container holding a value
pub struct Container {
    value: i32,
}

impl Container {
    /// Create a new container
    pub fn new(value: i32) -> Self {
        Container { value }
    }

    /// Get a reference to the value (demonstrates borrowing)
    pub fn get_ref(&self) -> &i32 {
        &self.value
    }

    /// Get a mutable reference to the value (demonstrates mutable borrowing)
    pub fn get_mut(&mut self) -> &mut i32 {
        &mut self.value
    }

    /// Increment the value
    pub fn increment(&mut self) {
        self.value += 1;
    }

    /// Get the value (consumes ownership)
    pub fn into_value(self) -> i32 {
        self.value
    }
}

/// Helper function that borrows and returns a reference
pub fn borrow_and_read(container: &Container) -> &i32 {
    container.get_ref()
}

/// Helper function that mutably borrows
pub fn borrow_and_increment(container: &mut Container) {
    container.increment();
}
