mod common;

use std::rc::Rc;

use alien_signals::{effect, effect_scope, signal};
use common::Log;

/// scope dispose runs child effect cleanup
#[test]
fn scope_dispose_runs_child_effect_cleanup() {
    let log = Log::default();
    let dispose = effect_scope(|| {
        let log = log.clone();
        let _ = effect(move || log.cleanup("inner:cleanup"));
    });
    dispose();
    assert_eq!(log.entries(), ["inner:cleanup"]);
}

/// scope dispose: sibling effects clean up in reverse creation (LIFO)
#[test]
fn scope_dispose_sibling_effects_clean_up_in_reverse_creation_lifo() {
    let log = Log::default();
    let dispose = effect_scope(|| {
        let e1 = log.clone();
        let _ = effect(move || e1.cleanup("e1:cleanup"));
        let e2 = log.clone();
        let _ = effect(move || e2.cleanup("e2:cleanup"));
        let e3 = log.clone();
        let _ = effect(move || e3.cleanup("e3:cleanup"));
    });
    dispose();
    assert_eq!(log.entries(), ["e3:cleanup", "e2:cleanup", "e1:cleanup"]);
}

/// scope dispose: nested effect cleanup runs depth-first reverse
#[test]
fn scope_dispose_nested_effect_cleanup_runs_depth_first_reverse() {
    let log = Log::default();
    let dispose = effect_scope(|| {
        let log = log.clone();
        let _ = effect(move || {
            let grandchild = log.clone();
            let _ = effect(move || grandchild.cleanup("grandchild:cleanup"));
            log.cleanup("child:cleanup")
        });
    });
    dispose();
    assert_eq!(log.entries(), ["grandchild:cleanup", "child:cleanup"]);
}

/// scope as intermediate parent: cleanup order respects nesting
#[test]
fn scope_as_intermediate_parent_cleanup_order_respects_nesting() {
    // When effectScope is used as an intermediate scope inside an outer
    // effect, the outer's re-run must still dispose the scope (and its
    // effects) before running the outer's own cleanup.
    let a = signal(0);
    let log = Log::default();

    let _ = effect({
        let (a, log) = (a.clone(), log.clone());
        move || {
            a(&());
            log.push("outer:run");
            let _ = effect_scope(|| {
                let log = log.clone();
                let _ = effect(move || {
                    log.push("inner:run");
                    log.cleanup("inner:cleanup")
                });
            });
            log.cleanup("outer:cleanup")
        }
    });
    log.clear();

    a(&(Rc::new(1),));
    assert_eq!(
        log.entries(),
        ["inner:cleanup", "outer:cleanup", "outer:run", "inner:run"]
    );
}
