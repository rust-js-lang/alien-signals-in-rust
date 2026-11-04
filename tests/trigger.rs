use std::cell::{Cell, RefCell};
use std::rc::Rc;

use alien_signals::{computed, effect, signal, trigger};

/// should not throw when triggering with no dependencies
#[test]
fn should_not_throw_when_triggering_with_no_dependencies() {
    trigger(|| {});
}

/// should trigger updates for dependent computed signals
#[test]
fn should_trigger_updates_for_dependent_computed_signals() {
    let arr = signal(RefCell::new(Vec::<i32>::new()));
    let length = computed({
        let arr = arr.clone();
        move |_| arr(&()).unwrap().borrow().len()
    });

    assert_eq!(*length(), 0);
    arr(&()).unwrap().borrow_mut().push(1);
    trigger(|| {
        arr(&());
    });
    assert_eq!(*length(), 1);
}

/// should trigger updates for the second source signal
#[test]
fn should_trigger_updates_for_the_second_source_signal() {
    let src1 = signal(RefCell::new(Vec::<i32>::new()));
    let src2 = signal(RefCell::new(Vec::<i32>::new()));
    let length = computed({
        let src2 = src2.clone();
        move |_| src2(&()).unwrap().borrow().len()
    });

    assert_eq!(*length(), 0);
    src2(&()).unwrap().borrow_mut().push(1);
    trigger(|| {
        src1(&());
        src2(&());
    });
    assert_eq!(*length(), 1);
}

/// should trigger effect once
#[test]
fn should_trigger_effect_once() {
    let src1 = signal(RefCell::new(Vec::<i32>::new()));
    let src2 = signal(RefCell::new(Vec::<i32>::new()));

    let triggers = Rc::new(Cell::new(0));

    let _ = effect({
        let (src1, src2, triggers) = (src1.clone(), src2.clone(), Rc::clone(&triggers));
        move || {
            triggers.set(triggers.get() + 1);
            src1(&());
            src2(&());
            None
        }
    });

    assert_eq!(triggers.get(), 1);
    trigger(|| {
        src1(&());
        src2(&());
    });
    assert_eq!(triggers.get(), 2);
}

/// should not notify the trigger function sub
#[test]
fn should_not_notify_the_trigger_function_sub() {
    let src1 = signal(RefCell::new(Vec::<i32>::new()));
    let src2 = computed({
        let src1 = src1.clone();
        move |_| src1(&()).unwrap()
    });

    let _ = effect({
        let (src1, src2) = (src1.clone(), src2.clone());
        move || {
            src1(&());
            src2();
            None
        }
    });
    trigger(|| {
        src1(&());
        src2();
    });
}

/// should allow writing a signal after reading it
#[test]
fn should_allow_writing_a_signal_after_reading_it() {
    let src1 = signal(1);

    trigger(|| {
        src1(&());
        src1(&(Rc::new(*src1(&()).unwrap() + 1),));
    });
    assert_eq!(*src1(&()).unwrap(), 2);
}

/// should rerun effect once when writing a signal after reading it
#[test]
fn should_rerun_effect_once_when_writing_a_signal_after_reading_it() {
    let src1 = signal(1);

    let triggers = Rc::new(Cell::new(0));

    let _ = effect({
        let (src1, triggers) = (src1.clone(), Rc::clone(&triggers));
        move || {
            triggers.set(triggers.get() + 1);
            src1(&());
            None
        }
    });

    assert_eq!(triggers.get(), 1);
    trigger(|| {
        src1(&());
        src1(&(Rc::new(*src1(&()).unwrap() + 1),));
    });
    assert_eq!(triggers.get(), 2);
    assert_eq!(*src1(&()).unwrap(), 2);
}
