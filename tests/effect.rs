mod common;

use std::cell::Cell;
use std::rc::Rc;

use alien_signals::system::ReactiveFlags;
use alien_signals::{computed, effect, get_active_sub, signal};
use common::Log;

/// should support custom recurse effect
#[test]
fn should_support_custom_recurse_effect() {
    let src = signal(0);

    let triggers = Rc::new(Cell::new(0));

    let _ = effect({
        let triggers = Rc::clone(&triggers);
        move || {
            let sub = get_active_sub().unwrap();
            sub.flags
                .set(sub.flags.get() & !ReactiveFlags::RECURSED_CHECK);
            triggers.set(triggers.get() + 1);
            src(&(Rc::new((*src(&()).unwrap() + 1).min(5)),));
            None
        }
    });

    assert_eq!(triggers.get(), 6);
}

/// cleanup order on outer re-run: inner before outer, before new run
#[test]
fn cleanup_order_on_outer_re_run_inner_before_outer_before_new_run() {
    let log = Log::default();
    let a = signal(0);

    let _ = effect({
        let (a, log) = (a.clone(), log.clone());
        move || {
            a(&());
            log.push("outer:run");
            let inner = log.clone();
            let _ = effect(move || {
                inner.push("inner:run");
                inner.cleanup("inner:cleanup")
            });
            log.cleanup("outer:cleanup")
        }
    });
    assert_eq!(log.entries(), ["outer:run", "inner:run"]);

    log.clear();
    a(&(Rc::new(1),));
    assert_eq!(
        log.entries(),
        ["inner:cleanup", "outer:cleanup", "outer:run", "inner:run"]
    );
}

/// cleanup order on dispose: inner before outer
#[test]
fn cleanup_order_on_dispose_inner_before_outer() {
    let log = Log::default();

    let dispose = effect({
        let log = log.clone();
        move || {
            log.push("outer:run");
            let inner = log.clone();
            let _ = effect(move || {
                inner.push("inner:run");
                inner.cleanup("inner:cleanup")
            });
            log.cleanup("outer:cleanup")
        }
    });
    log.clear();
    dispose();
    assert_eq!(log.entries(), ["inner:cleanup", "outer:cleanup"]);
}

/// sibling cleanup order on dispose: reverse creation (LIFO)
#[test]
fn sibling_cleanup_order_on_dispose_reverse_creation_lifo() {
    let log = Log::default();

    let dispose = effect({
        let log = log.clone();
        move || {
            let inner1 = log.clone();
            let _ = effect(move || inner1.cleanup("inner1:cleanup"));
            let inner2 = log.clone();
            let _ = effect(move || inner2.cleanup("inner2:cleanup"));
            let inner3 = log.clone();
            let _ = effect(move || inner3.cleanup("inner3:cleanup"));
            log.cleanup("outer:cleanup")
        }
    });
    dispose();
    assert_eq!(
        log.entries(),
        [
            "inner3:cleanup",
            "inner2:cleanup",
            "inner1:cleanup",
            "outer:cleanup",
        ]
    );
}

/// sibling cleanup order on outer re-run: reverse creation (LIFO)
#[test]
fn sibling_cleanup_order_on_outer_re_run_reverse_creation_lifo() {
    let log = Log::default();
    let a = signal(0);

    let _ = effect({
        let (a, log) = (a.clone(), log.clone());
        move || {
            a(&());
            let inner1 = log.clone();
            let _ = effect(move || inner1.cleanup("inner1:cleanup"));
            let inner2 = log.clone();
            let _ = effect(move || inner2.cleanup("inner2:cleanup"));
            let inner3 = log.clone();
            let _ = effect(move || inner3.cleanup("inner3:cleanup"));
            log.cleanup("outer:cleanup")
        }
    });
    log.clear();

    a(&(Rc::new(1),));
    assert_eq!(
        log.entries()[..4],
        [
            "inner3:cleanup",
            "inner2:cleanup",
            "inner1:cleanup",
            "outer:cleanup",
        ]
    );
}

/// three-level nested cleanup on dispose: deepest first (depth-first reverse)
#[test]
fn three_level_nested_cleanup_on_dispose_deepest_first_depth_first_reverse() {
    let log = Log::default();

    let dispose = effect({
        let log = log.clone();
        move || {
            let child = log.clone();
            let _ = effect(move || {
                let grandchild = child.clone();
                let _ = effect(move || grandchild.cleanup("grandchild:cleanup"));
                child.cleanup("child:cleanup")
            });
            log.cleanup("outer:cleanup")
        }
    });
    dispose();
    assert_eq!(
        log.entries(),
        ["grandchild:cleanup", "child:cleanup", "outer:cleanup"]
    );
}

/// computed unwatched: child effect cleanups run in reverse creation (LIFO)
#[test]
fn computed_unwatched_child_effect_cleanups_run_in_reverse_creation_lifo() {
    // When the computed loses its last subscriber and gets unwatched, any
    // effects it created during its getter must be cleaned up LIFO, not FIFO.
    let log = Log::default();
    let c = computed({
        let log = log.clone();
        move |_| {
            let e1 = log.clone();
            let _ = effect(move || e1.cleanup("e1"));
            let e2 = log.clone();
            let _ = effect(move || e2.cleanup("e2"));
            let e3 = log.clone();
            let _ = effect(move || e3.cleanup("e3"));
            0
        }
    });
    let dispose = effect(move || {
        c();
        None
    });
    log.clear();
    dispose();
    assert_eq!(log.entries(), ["e3", "e2", "e1"]);
}

/// effect created inside computed: old inner cleanup runs before new inner setup
#[test]
fn effect_created_inside_computed_old_inner_cleanup_runs_before_new_inner_setup() {
    // When a computed re-evaluates, any effects its getter created on the
    // previous run must be disposed (with cleanup) before the getter runs
    // again. Otherwise the old inner's cleanup ends up running after the
    // new inner has already been set up.
    let a = signal(0);
    let log = Log::default();

    let c = computed({
        let (a, log) = (a.clone(), log.clone());
        move |_| {
            log.push("computed:eval");
            let inner = log.clone();
            let _ = effect(move || {
                inner.push("inner:run");
                inner.cleanup("inner:cleanup")
            });
            *a(&()).unwrap()
        }
    });

    let _ = effect(move || {
        c();
        None
    });
    log.clear();

    a(&(Rc::new(1),));
    assert_eq!(
        log.entries(),
        ["inner:cleanup", "computed:eval", "inner:run"]
    );
}

/// cleanup order is correct on outer re-run after a prior inner-only re-run
#[test]
fn cleanup_order_is_correct_on_outer_re_run_after_a_prior_inner_only_re_run() {
    // Regression: inner re-running alone routes outer through run()'s
    // not-dirty branch (restore Watching), which must preserve any
    // "has child effect" tracking so the next real outer re-run still
    // disposes children before its own cleanup.
    let a = signal(0);
    let b = signal(0);
    let log = Log::default();

    let _ = effect({
        let (a, b, log) = (a.clone(), b.clone(), log.clone());
        move || {
            a(&());
            log.push("outer:run");
            let (b, inner) = (b.clone(), log.clone());
            let _ = effect(move || {
                b(&());
                inner.push("inner:run");
                inner.cleanup("inner:cleanup")
            });
            log.cleanup("outer:cleanup")
        }
    });

    b(&(Rc::new(1),)); // inner re-runs alone; outer is touched via notify chain
    log.clear();

    a(&(Rc::new(1),));
    assert_eq!(
        log.entries(),
        ["inner:cleanup", "outer:cleanup", "outer:run", "inner:run"]
    );
}

// https://github.com/stackblitz/alien-signals/issues/115
/// outer effect keeps responding to its own dep after inner re-runs
#[test]
fn outer_effect_keeps_responding_to_its_own_dep_after_inner_re_runs() {
    let a = signal(0);
    let b = signal(0);
    let outer_runs = Rc::new(Cell::new(0));
    let inner_runs = Rc::new(Cell::new(0));

    let _ = effect({
        let (a, b) = (a.clone(), b.clone());
        let (outer_runs, inner_runs) = (Rc::clone(&outer_runs), Rc::clone(&inner_runs));
        move || {
            a(&());
            outer_runs.set(outer_runs.get() + 1);
            let (b, inner_runs) = (b.clone(), Rc::clone(&inner_runs));
            let _ = effect(move || {
                b(&());
                inner_runs.set(inner_runs.get() + 1);
                None
            });
            None
        }
    });
    assert_eq!(outer_runs.get(), 1);
    assert_eq!(inner_runs.get(), 1);

    b(&(Rc::new(1),));
    assert_eq!(outer_runs.get(), 1);
    assert!(inner_runs.get() >= 2);

    a(&(Rc::new(1),));
    assert_eq!(outer_runs.get(), 2);
}
