use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js::{Rest, Unknown};

pub mod system;

use system::{
    Link, ReactiveFlags, ReactiveNode, ReactiveSystem, ReactiveSystemOptions,
    create_reactive_system,
};

type Value = Option<&'static Unknown>;

pub enum NodeKind {
    Signal {
        current_value: Cell<Value>,
        pending_value: Cell<Value>,
    },
    Computed {
        value: Cell<Value>,
        #[cfg_attr(rust_js, rust_js::test)]
        getter: Box<dyn Fn(Value) -> Value>,
    },
    Effect {
        r#fn: Box<dyn Fn() -> Option<Rc<dyn Fn()>>>,
        cleanup: RefCell<Option<Rc<dyn Fn()>>>,
    },
    EffectScope,
}

type Node = Rc<ReactiveNode<NodeKind>>;

// Marks a parent (effect or scope) whose deps include at least one child
// effect. Used to gate the dispose-children-first slow path in run() so
// leaf effects (no children, no own cleanup) avoid the extra deps walk.
// The bit is outside ReactiveFlags' range and never touched by system.ts.
const HAS_CHILD_EFFECT: i32 = 64;

thread_local! {
    static CYCLE: Cell<u32> = const { Cell::new(0) };
    static RUN_DEPTH: Cell<u32> = const { Cell::new(0) };
    static BATCH_DEPTH: Cell<u32> = const { Cell::new(0) };
    static NOTIFY_INDEX: Cell<usize> = const { Cell::new(0) };
    static QUEUED_LENGTH: Cell<usize> = const { Cell::new(0) };
    static ACTIVE_SUB: RefCell<Option<Node>> = const { RefCell::new(None) };

    static QUEUED: RefCell<Vec<Option<Node>>> = const { RefCell::new(Vec::new()) };
    static SYSTEM: ReactiveSystem<NodeKind> = create_reactive_system(ReactiveSystemOptions {
        update: Box::new(|node| match &node.kind {
            NodeKind::Computed { .. } => update_computed(node),
            NodeKind::Signal { .. } => update_signal(node),
            _ => {
                node.flags.set(ReactiveFlags::MUTABLE);
                true
            }
        }),
        notify: Box::new(|effect| {
            let mut insert_index = QUEUED_LENGTH.get();
            let first_inserted_index = insert_index;
            let mut effect = Rc::clone(effect);

            QUEUED.with_borrow_mut(|queued| {
                loop {
                    if insert_index < queued.len() {
                        queued[insert_index] = Some(Rc::clone(&effect));
                    } else {
                        queued.push(Some(Rc::clone(&effect)));
                    }
                    insert_index += 1;
                    effect.flags.set(effect.flags.get() & !ReactiveFlags::WATCHING);
                    let next = effect.subs.borrow().as_ref().map(|subs| Rc::clone(&subs.sub));
                    match next {
                        Some(next) if next.flags.get() & ReactiveFlags::WATCHING != 0 => {
                            effect = next;
                        }
                        _ => break,
                    }
                }

                QUEUED_LENGTH.set(insert_index);

                queued[first_inserted_index..insert_index].reverse();
            });
        }),
        unwatched: Box::new(|node| match &node.kind {
            NodeKind::Computed { .. } => {
                if node.deps_tail.borrow().is_some() {
                    node.flags.set(ReactiveFlags::MUTABLE | ReactiveFlags::DIRTY);
                    dispose_all_deps_in_reverse(node);
                }
            }
            NodeKind::Signal { .. } => {
                // Nothing to do for signals, they are always mutable and never dirty until pendingValue changes
            }
            NodeKind::Effect { .. } => effect_oper(node),
            NodeKind::EffectScope => effect_scope_oper(node),
        }),
    });
}

fn link(dep: &Node, sub: &Node, version: u32) {
    SYSTEM.with(|system| (system.link)(dep, sub, version));
}

fn unlink(link: &Rc<Link<NodeKind>>, sub: Option<&Node>) -> Option<Rc<Link<NodeKind>>> {
    SYSTEM.with(|system| (system.unlink)(link, sub))
}

fn propagate(link: Rc<Link<NodeKind>>, inner_write: bool) {
    SYSTEM.with(|system| (system.propagate)(link, inner_write));
}

fn check_dirty(link: Rc<Link<NodeKind>>, sub: Node) -> bool {
    SYSTEM.with(|system| (system.check_dirty)(link, sub))
}

fn shallow_propagate(link: Rc<Link<NodeKind>>) {
    SYSTEM.with(|system| (system.shallow_propagate)(link));
}

pub fn get_active_sub() -> Option<Node> {
    ACTIVE_SUB.with_borrow(Clone::clone)
}

pub fn set_active_sub(sub: Option<Node>) -> Option<Node> {
    ACTIVE_SUB.replace(sub)
}

pub fn get_batch_depth() -> u32 {
    BATCH_DEPTH.get()
}

pub fn start_batch() {
    BATCH_DEPTH.set(BATCH_DEPTH.get() + 1);
}

pub fn end_batch() {
    BATCH_DEPTH.set(BATCH_DEPTH.get() - 1);
    if BATCH_DEPTH.get() == 0 {
        flush();
    }
}

pub fn is_signal(r#fn: &dyn Fn()) -> bool {
    js::function::name(r#fn) == format!("bound {}", js::function::name(&signal_oper::<()>))
}

pub fn is_computed(r#fn: &dyn Fn()) -> bool {
    js::function::name(r#fn) == format!("bound {}", js::function::name(&computed_oper::<()>))
}

pub fn is_effect(r#fn: &dyn Fn()) -> bool {
    js::function::name(r#fn) == format!("bound {}", js::function::name(&effect_oper))
}

pub fn is_effect_scope(r#fn: &dyn Fn()) -> bool {
    js::function::name(r#fn) == format!("bound {}", js::function::name(&effect_scope_oper))
}

pub fn signal<T: 'static>(initial_value: T) -> impl Fn(&dyn Rest<Rc<T>>) -> Option<Rc<T>> + Clone {
    let initial_value = Rc::new(initial_value);
    let this = Rc::new(ReactiveNode {
        kind: NodeKind::Signal {
            current_value: Cell::new(js::any_of(Rc::clone(&initial_value))),
            pending_value: Cell::new(js::any_of(initial_value)),
        },
        subs: RefCell::new(None),
        subs_tail: RefCell::new(None),
        deps: RefCell::new(None),
        deps_tail: RefCell::new(None),
        flags: Cell::new(ReactiveFlags::MUTABLE),
    });
    move |value: &dyn Rest<Rc<T>>| signal_oper(&this, value)
}

pub fn computed<T: 'static>(
    getter: impl Fn(Option<Rc<T>>) -> T + 'static,
) -> impl Fn() -> Rc<T> + Clone {
    let this = Rc::new(ReactiveNode {
        kind: NodeKind::Computed {
            value: Cell::new(None),
            getter: Box::new(move |previous: Value| {
                // SAFETY: a computed's value is what its getter gave, an `Rc<T>` (ADR 0909).
                let previous = previous
                    .map(|previous| Rc::clone(unsafe { js::cast::<&Rc<T>>(Some(previous)) }));
                js::any_of(Rc::new(getter(previous)))
            }),
        },
        subs: RefCell::new(None),
        subs_tail: RefCell::new(None),
        deps: RefCell::new(None),
        deps_tail: RefCell::new(None),
        flags: Cell::new(ReactiveFlags::NONE),
    });
    move || computed_oper::<T>(&this)
}

pub fn effect(r#fn: impl Fn() -> Option<Rc<dyn Fn()>> + 'static) -> impl Fn() + Clone {
    let e = Rc::new(ReactiveNode {
        kind: NodeKind::Effect {
            r#fn: Box::new(r#fn),
            cleanup: RefCell::new(None),
        },
        subs: RefCell::new(None),
        subs_tail: RefCell::new(None),
        deps: RefCell::new(None),
        deps_tail: RefCell::new(None),
        flags: Cell::new(ReactiveFlags::WATCHING | ReactiveFlags::RECURSED_CHECK),
    });
    let prev_sub = set_active_sub(Some(Rc::clone(&e)));
    if let Some(prev_sub) = &prev_sub {
        link(&e, prev_sub, 0);
        prev_sub.flags.set(prev_sub.flags.get() | HAS_CHILD_EFFECT);
    }
    {
        let _restore = Finally(|| {
            RUN_DEPTH.set(RUN_DEPTH.get() - 1);
            ACTIVE_SUB.set(prev_sub.clone());
            e.flags.set(e.flags.get() & !ReactiveFlags::RECURSED_CHECK);
        });
        RUN_DEPTH.set(RUN_DEPTH.get() + 1);
        if let NodeKind::Effect { r#fn, cleanup } = &e.kind {
            *cleanup.borrow_mut() = r#fn();
        }
    }
    move || effect_oper(&e)
}

pub fn effect_scope(r#fn: impl Fn()) -> impl Fn() {
    let e = Rc::new(ReactiveNode {
        deps: RefCell::new(None),
        deps_tail: RefCell::new(None),
        subs: RefCell::new(None),
        subs_tail: RefCell::new(None),
        flags: Cell::new(ReactiveFlags::MUTABLE),
        kind: NodeKind::EffectScope,
    });
    let prev_sub = set_active_sub(Some(Rc::clone(&e)));
    if let Some(prev_sub) = &prev_sub {
        link(&e, prev_sub, 0);
        prev_sub.flags.set(prev_sub.flags.get() | HAS_CHILD_EFFECT);
    }
    {
        let _restore = Finally(|| ACTIVE_SUB.set(prev_sub.clone()));
        r#fn();
    }
    move || effect_scope_oper(&e)
}

pub fn trigger(r#fn: impl Fn()) {
    let sub = Rc::new(ReactiveNode {
        deps: RefCell::new(None),
        deps_tail: RefCell::new(None),
        subs: RefCell::new(None),
        subs_tail: RefCell::new(None),
        flags: Cell::new(ReactiveFlags::WATCHING | ReactiveFlags::RECURSED_CHECK),
        kind: NodeKind::EffectScope,
    });
    let prev_sub = set_active_sub(Some(Rc::clone(&sub)));
    BATCH_DEPTH.set(BATCH_DEPTH.get() + 1);
    let _restore = Finally(|| {
        ACTIVE_SUB.set(prev_sub.clone());
        sub.flags.set(ReactiveFlags::NONE);
        let mut link = sub.deps.borrow().clone();
        while let Some(current) = link {
            let dep = Rc::clone(&current.dep);
            link = unlink(&current, Some(&sub));
            let subs = dep.subs.borrow().clone();
            if let Some(subs) = subs {
                propagate(Rc::clone(&subs), RUN_DEPTH.get() != 0);
                shallow_propagate(subs);
            }
        }
        BATCH_DEPTH.set(BATCH_DEPTH.get() - 1);
        if BATCH_DEPTH.get() == 0 {
            flush();
        }
    });
    r#fn();
}

struct Finally<F: FnMut()>(F);

impl<F: FnMut()> Drop for Finally<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

fn update_computed(c: &Node) -> bool {
    let NodeKind::Computed { value, getter } = &c.kind else {
        unreachable!()
    };
    if c.flags.get() & HAS_CHILD_EFFECT != 0 {
        let mut link = c.deps_tail.borrow().clone();
        while let Some(current) = link {
            let prev = current.prev_dep.borrow().clone();
            let dep = &current.dep;
            if matches!(dep.kind, NodeKind::Effect { .. } | NodeKind::EffectScope) {
                unlink(&current, Some(c));
            }
            link = prev;
        }
    }
    *c.deps_tail.borrow_mut() = None;
    c.flags
        .set(ReactiveFlags::MUTABLE | ReactiveFlags::RECURSED_CHECK);
    let prev_sub = set_active_sub(Some(Rc::clone(c)));
    let _restore = Finally(|| {
        ACTIVE_SUB.set(prev_sub.clone());
        c.flags.set(c.flags.get() & !ReactiveFlags::RECURSED_CHECK);
        purge_deps(c);
    });
    CYCLE.set(CYCLE.get() + 1);
    let old_value = value.get();
    value.set(getter(old_value));
    !js::strict_equals(old_value, value.get())
}

fn update_signal(s: &Node) -> bool {
    let NodeKind::Signal {
        current_value,
        pending_value,
    } = &s.kind
    else {
        unreachable!()
    };
    s.flags.set(ReactiveFlags::MUTABLE);
    !js::strict_equals(
        current_value.replace(pending_value.get()),
        current_value.get(),
    )
}

fn run(e: &Node) {
    let NodeKind::Effect { r#fn, cleanup } = &e.kind else {
        unreachable!()
    };
    let flags = e.flags.get();
    if flags & ReactiveFlags::DIRTY != 0
        || (flags & ReactiveFlags::PENDING != 0
            && check_dirty(
                {
                    let deps = e.deps.borrow().clone();
                    deps.unwrap()
                },
                Rc::clone(e),
            ))
    {
        if flags & HAS_CHILD_EFFECT != 0 {
            let mut link = e.deps_tail.borrow().clone();
            while let Some(current) = link {
                let prev = current.prev_dep.borrow().clone();
                let dep = &current.dep;
                if matches!(dep.kind, NodeKind::Effect { .. } | NodeKind::EffectScope) {
                    unlink(&current, Some(e));
                }
                link = prev;
            }
        }
        if cleanup.borrow().is_some() {
            run_cleanup(e);
            if e.flags.get() == 0 {
                return;
            }
        }
        *e.deps_tail.borrow_mut() = None;
        e.flags
            .set(ReactiveFlags::WATCHING | ReactiveFlags::RECURSED_CHECK);
        let prev_sub = set_active_sub(Some(Rc::clone(e)));
        let _restore = Finally(|| {
            RUN_DEPTH.set(RUN_DEPTH.get() - 1);
            ACTIVE_SUB.set(prev_sub.clone());
            e.flags.set(e.flags.get() & !ReactiveFlags::RECURSED_CHECK);
            purge_deps(e);
        });
        CYCLE.set(CYCLE.get() + 1);
        RUN_DEPTH.set(RUN_DEPTH.get() + 1);
        *cleanup.borrow_mut() = r#fn();
    } else if e.deps.borrow().is_some() {
        e.flags
            .set(ReactiveFlags::WATCHING | (flags & HAS_CHILD_EFFECT));
    }
}

fn flush() {
    let _restore = Finally(|| {
        while NOTIFY_INDEX.get() < QUEUED_LENGTH.get() {
            let effect =
                QUEUED.with_borrow_mut(|queued| queued[NOTIFY_INDEX.get()].take().unwrap());
            NOTIFY_INDEX.set(NOTIFY_INDEX.get() + 1);
            effect
                .flags
                .set(effect.flags.get() | ReactiveFlags::WATCHING | ReactiveFlags::RECURSED);
        }
        NOTIFY_INDEX.set(0);
        QUEUED_LENGTH.set(0);
    });
    while NOTIFY_INDEX.get() < QUEUED_LENGTH.get() {
        let effect = QUEUED.with_borrow_mut(|queued| queued[NOTIFY_INDEX.get()].take().unwrap());
        NOTIFY_INDEX.set(NOTIFY_INDEX.get() + 1);
        run(&effect);
    }
}

fn computed_oper<T: 'static>(this: &Node) -> Rc<T> {
    let NodeKind::Computed { value, getter } = &this.kind else {
        unreachable!()
    };
    let flags = this.flags.get();
    if flags & ReactiveFlags::DIRTY != 0
        || (flags & ReactiveFlags::PENDING != 0
            && (check_dirty(
                {
                    let deps = this.deps.borrow().clone();
                    deps.unwrap()
                },
                Rc::clone(this),
            ) || {
                this.flags.set(flags & !ReactiveFlags::PENDING);
                false
            }))
    {
        if update_computed(this) {
            let subs = this.subs.borrow().clone();
            if let Some(subs) = subs {
                shallow_propagate(subs);
            }
        }
    } else if flags == 0 {
        this.flags
            .set(ReactiveFlags::MUTABLE | ReactiveFlags::RECURSED_CHECK);
        let prev_sub = set_active_sub(Some(Rc::clone(this)));
        let _restore = Finally(|| {
            ACTIVE_SUB.set(prev_sub.clone());
            this.flags
                .set(this.flags.get() & !ReactiveFlags::RECURSED_CHECK);
        });
        value.set(getter(None));
    }
    let sub = ACTIVE_SUB.with_borrow(Clone::clone);
    if let Some(sub) = sub {
        link(this, &sub, CYCLE.get());
    }
    // SAFETY: a computed's value is what its getter gave, an `Rc<T>` (ADR 0909).
    Rc::clone(unsafe { js::cast::<&Rc<T>>(value.get()) })
}

fn signal_oper<T: 'static>(
    this: &Node,
    #[cfg_attr(rust_js, rust_js::rest)] value: &[Rc<T>],
) -> Option<Rc<T>> {
    let NodeKind::Signal {
        current_value,
        pending_value,
    } = &this.kind
    else {
        unreachable!()
    };
    if !value.is_empty() {
        if !js::strict_equals(
            pending_value.replace(js::any_of(Rc::clone(&value[0]))),
            pending_value.get(),
        ) {
            this.flags
                .set(ReactiveFlags::MUTABLE | ReactiveFlags::DIRTY);
            let subs = this.subs.borrow().clone();
            if let Some(subs) = subs {
                propagate(subs, RUN_DEPTH.get() != 0);
                if BATCH_DEPTH.get() == 0 {
                    flush();
                }
            }
        }
        None
    } else {
        if this.flags.get() & ReactiveFlags::DIRTY != 0 && update_signal(this) {
            let subs = this.subs.borrow().clone();
            if let Some(subs) = subs {
                shallow_propagate(subs);
            }
        }
        let sub = ACTIVE_SUB.with_borrow(Clone::clone);
        if let Some(sub) = sub {
            link(this, &sub, CYCLE.get());
        }
        // SAFETY: a signal's value is one it was given, an `Rc<T>` (ADR 0909).
        Some(Rc::clone(unsafe {
            js::cast::<&Rc<T>>(current_value.get())
        }))
    }
}

fn run_cleanup(e: &Node) {
    let NodeKind::Effect { cleanup, .. } = &e.kind else {
        unreachable!()
    };
    let cleanup = cleanup.take().unwrap();
    let prev_sub = ACTIVE_SUB.take();
    let _restore = Finally(|| ACTIVE_SUB.set(prev_sub.clone()));
    cleanup();
}

fn effect_oper(this: &Node) {
    effect_scope_oper(this);
    let NodeKind::Effect { cleanup, .. } = &this.kind else {
        unreachable!()
    };
    if cleanup.borrow().is_some() {
        run_cleanup(this);
    }
}

fn effect_scope_oper(this: &Node) {
    this.flags.set(ReactiveFlags::NONE);
    dispose_all_deps_in_reverse(this);
    let sub = this.subs.borrow().clone();
    if let Some(sub) = sub {
        unlink(&sub, None);
    }
}

fn dispose_all_deps_in_reverse(sub: &Node) {
    let mut link = sub.deps_tail.borrow().clone();
    while let Some(current) = link {
        let prev = current.prev_dep.borrow().clone();
        unlink(&current, Some(sub));
        link = prev;
    }
}

fn purge_deps(sub: &Node) {
    let deps_tail = sub.deps_tail.borrow().clone();
    let mut dep = match deps_tail {
        Some(deps_tail) => deps_tail.next_dep.borrow().clone(),
        None => sub.deps.borrow().clone(),
    };
    while let Some(current) = dep {
        dep = unlink(&current, Some(sub));
    }
}
