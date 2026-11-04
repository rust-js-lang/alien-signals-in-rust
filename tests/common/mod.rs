use std::cell::RefCell;
use std::rc::Rc;

/// What the effects did, in order: one list they all push to.
#[derive(Clone, Default)]
pub struct Log(Rc<RefCell<Vec<&'static str>>>);

impl Log {
    pub fn push(&self, entry: &'static str) {
        self.0.borrow_mut().push(entry);
    }

    /// An effect's cleanup that pushes `entry`.
    pub fn cleanup(&self, entry: &'static str) -> Option<Rc<dyn Fn()>> {
        let log = self.clone();
        Some(Rc::new(move || log.push(entry)))
    }

    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub fn entries(&self) -> Vec<&'static str> {
        self.0.borrow().clone()
    }
}
