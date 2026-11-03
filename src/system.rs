use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct ReactiveNode<K> {
    pub deps: RefCell<Option<Rc<Link<K>>>>,
    pub deps_tail: RefCell<Option<Rc<Link<K>>>>,
    pub subs: RefCell<Option<Rc<Link<K>>>>,
    pub subs_tail: RefCell<Option<Rc<Link<K>>>>,
    pub flags: Cell<i32>,
    #[cfg_attr(rust_js, rust_js::flatten)]
    pub kind: K,
}

pub struct Link<K> {
    pub version: Cell<u32>,
    pub dep: Rc<ReactiveNode<K>>,
    pub sub: Rc<ReactiveNode<K>>,
    pub prev_sub: RefCell<Option<Rc<Link<K>>>>,
    pub next_sub: RefCell<Option<Rc<Link<K>>>>,
    pub prev_dep: RefCell<Option<Rc<Link<K>>>>,
    pub next_dep: RefCell<Option<Rc<Link<K>>>>,
}

struct Stack<T> {
    value: T,
    prev: Option<Box<Stack<T>>>,
}

pub struct ReactiveFlags;

impl ReactiveFlags {
    #[cfg_attr(rust_js, rust_js::name = "None")]
    pub const NONE: i32 = 0;
    #[cfg_attr(rust_js, rust_js::name = "Mutable")]
    pub const MUTABLE: i32 = 1;
    #[cfg_attr(rust_js, rust_js::name = "Watching")]
    pub const WATCHING: i32 = 2;
    #[cfg_attr(rust_js, rust_js::name = "RecursedCheck")]
    pub const RECURSED_CHECK: i32 = 4;
    #[cfg_attr(rust_js, rust_js::name = "Recursed")]
    pub const RECURSED: i32 = 8;
    #[cfg_attr(rust_js, rust_js::name = "Dirty")]
    pub const DIRTY: i32 = 16;
    #[cfg_attr(rust_js, rust_js::name = "Pending")]
    pub const PENDING: i32 = 32;
}

type Node<K> = Rc<ReactiveNode<K>>;

pub struct ReactiveSystemOptions<K> {
    pub update: Box<dyn Fn(&Node<K>) -> bool>,
    pub notify: Box<dyn Fn(&Node<K>)>,
    pub unwatched: Box<dyn Fn(&Node<K>)>,
}

pub struct ReactiveSystem<K> {
    pub link: Rc<dyn Fn(&Node<K>, &Node<K>, u32)>,
    pub unlink: Rc<dyn Fn(&Rc<Link<K>>, Option<&Node<K>>) -> Option<Rc<Link<K>>>>,
    pub propagate: Rc<dyn Fn(Rc<Link<K>>, bool)>,
    pub check_dirty: Rc<dyn Fn(Rc<Link<K>>, Node<K>) -> bool>,
    pub shallow_propagate: Rc<dyn Fn(Rc<Link<K>>)>,
}

struct System<K> {
    update: Box<dyn Fn(&Node<K>) -> bool>,
    notify: Box<dyn Fn(&Node<K>)>,
    unwatched: Box<dyn Fn(&Node<K>)>,
}

pub fn create_reactive_system<K: 'static>(
    ReactiveSystemOptions {
        update,
        notify,
        unwatched,
    }: ReactiveSystemOptions<K>,
) -> ReactiveSystem<K> {
    let system = Rc::new(System {
        update,
        notify,
        unwatched,
    });
    ReactiveSystem {
        link: {
            let system = Rc::clone(&system);
            Rc::new(move |dep, sub, version| system.link(dep, sub, version))
        },
        unlink: {
            let system = Rc::clone(&system);
            Rc::new(move |link, sub| system.unlink(link, sub))
        },
        propagate: {
            let system = Rc::clone(&system);
            Rc::new(move |link, inner_write| system.propagate(link, inner_write))
        },
        check_dirty: {
            let system = Rc::clone(&system);
            Rc::new(move |link, sub| system.check_dirty(link, sub))
        },
        shallow_propagate: Rc::new(move |link| system.shallow_propagate(link)),
    }
}

impl<K> System<K> {
    fn link(&self, dep: &Node<K>, sub: &Node<K>, version: u32) {
        let prev_dep = sub.deps_tail.borrow().clone();
        if let Some(prev_dep) = &prev_dep
            && Rc::ptr_eq(&prev_dep.dep, dep)
        {
            return;
        }
        let next_dep = match &prev_dep {
            Some(prev_dep) => prev_dep.next_dep.borrow().clone(),
            None => sub.deps.borrow().clone(),
        };
        if let Some(next_dep) = &next_dep
            && Rc::ptr_eq(&next_dep.dep, dep)
        {
            next_dep.version.set(version);
            *sub.deps_tail.borrow_mut() = Some(Rc::clone(next_dep));
            return;
        }
        let prev_sub = dep.subs_tail.borrow().clone();
        if let Some(prev_sub) = &prev_sub
            && prev_sub.version.get() == version
            && Rc::ptr_eq(&prev_sub.sub, sub)
        {
            return;
        }
        let new_link = Rc::new(Link {
            version: Cell::new(version),
            dep: Rc::clone(dep),
            sub: Rc::clone(sub),
            prev_dep: RefCell::new(prev_dep.clone()),
            next_dep: RefCell::new(next_dep.clone()),
            prev_sub: RefCell::new(prev_sub.clone()),
            next_sub: RefCell::new(None),
        });
        *sub.deps_tail.borrow_mut() = Some(Rc::clone(&new_link));
        *dep.subs_tail.borrow_mut() = Some(Rc::clone(&new_link));
        if let Some(next_dep) = &next_dep {
            *next_dep.prev_dep.borrow_mut() = Some(Rc::clone(&new_link));
        }
        if let Some(prev_dep) = &prev_dep {
            *prev_dep.next_dep.borrow_mut() = Some(Rc::clone(&new_link));
        } else {
            *sub.deps.borrow_mut() = Some(Rc::clone(&new_link));
        }
        if let Some(prev_sub) = &prev_sub {
            *prev_sub.next_sub.borrow_mut() = Some(new_link);
        } else {
            *dep.subs.borrow_mut() = Some(new_link);
        }
    }

    fn unlink(&self, link: &Rc<Link<K>>, sub: Option<&Node<K>>) -> Option<Rc<Link<K>>> {
        let sub = sub.unwrap_or(&link.sub);
        let dep = &link.dep;
        let prev_dep = link.prev_dep.borrow().clone();
        let next_dep = link.next_dep.borrow().clone();
        let next_sub = link.next_sub.borrow().clone();
        let prev_sub = link.prev_sub.borrow().clone();
        if let Some(next_dep) = &next_dep {
            *next_dep.prev_dep.borrow_mut() = prev_dep.clone();
        } else {
            *sub.deps_tail.borrow_mut() = prev_dep.clone();
        }
        if let Some(prev_dep) = &prev_dep {
            *prev_dep.next_dep.borrow_mut() = next_dep.clone();
        } else {
            *sub.deps.borrow_mut() = next_dep.clone();
        }
        if let Some(next_sub) = &next_sub {
            *next_sub.prev_sub.borrow_mut() = prev_sub.clone();
        } else {
            *dep.subs_tail.borrow_mut() = prev_sub.clone();
        }
        if let Some(prev_sub) = &prev_sub {
            *prev_sub.next_sub.borrow_mut() = next_sub;
        } else {
            let watched = next_sub.is_some();
            *dep.subs.borrow_mut() = next_sub;
            if !watched {
                (self.unwatched)(dep);
            }
        }
        next_dep
    }

    fn propagate(&self, mut link: Rc<Link<K>>, inner_write: bool) {
        let mut next = link.next_sub.borrow().clone();
        let mut stack: Option<Box<Stack<Option<Rc<Link<K>>>>>> = None;

        'top: loop {
            let sub = Rc::clone(&link.sub);
            let mut flags = sub.flags.get();

            if flags
                & (ReactiveFlags::RECURSED_CHECK
                    | ReactiveFlags::RECURSED
                    | ReactiveFlags::DIRTY
                    | ReactiveFlags::PENDING)
                == 0
            {
                sub.flags.set(flags | ReactiveFlags::PENDING);
                if inner_write {
                    sub.flags.set(sub.flags.get() | ReactiveFlags::RECURSED);
                }
            } else if flags & (ReactiveFlags::RECURSED_CHECK | ReactiveFlags::RECURSED) == 0 {
                flags = ReactiveFlags::NONE;
            } else if flags & ReactiveFlags::RECURSED_CHECK == 0 {
                sub.flags
                    .set((flags & !ReactiveFlags::RECURSED) | ReactiveFlags::PENDING);
            } else if flags & (ReactiveFlags::DIRTY | ReactiveFlags::PENDING) == 0
                && self.is_valid_link(&link, &sub)
            {
                sub.flags
                    .set(flags | (ReactiveFlags::RECURSED | ReactiveFlags::PENDING));
                flags &= ReactiveFlags::MUTABLE;
            } else {
                flags = ReactiveFlags::NONE;
            }

            if flags & ReactiveFlags::WATCHING != 0 {
                (self.notify)(&sub);
            }

            if flags & ReactiveFlags::MUTABLE != 0 {
                let sub_subs = sub.subs.borrow().clone();
                if let Some(sub_subs) = sub_subs {
                    link = sub_subs;
                    let next_sub = link.next_sub.borrow().clone();
                    if let Some(next_sub) = next_sub {
                        stack = Some(Box::new(Stack {
                            value: next,
                            prev: stack,
                        }));
                        next = Some(next_sub);
                    }
                    continue;
                }
            }

            if let Some(next_link) = next {
                link = next_link;
                next = link.next_sub.borrow().clone();
                continue;
            }

            while let Some(top) = stack {
                stack = top.prev;
                if let Some(top_link) = top.value {
                    link = top_link;
                    next = link.next_sub.borrow().clone();
                    continue 'top;
                }
            }

            break;
        }
    }

    fn check_dirty(&self, mut link: Rc<Link<K>>, mut sub: Node<K>) -> bool {
        let mut stack: Option<Box<Stack<Rc<Link<K>>>>> = None;
        let mut check_depth = 0;
        let mut dirty = false;

        'top: loop {
            let dep = Rc::clone(&link.dep);
            let flags = dep.flags.get();

            if sub.flags.get() & ReactiveFlags::DIRTY != 0 {
                dirty = true;
            } else if flags & (ReactiveFlags::MUTABLE | ReactiveFlags::DIRTY)
                == ReactiveFlags::MUTABLE | ReactiveFlags::DIRTY
            {
                let subs = dep.subs.borrow().clone();
                if (self.update)(&dep) {
                    let subs = subs.unwrap();
                    if subs.next_sub.borrow().is_some() {
                        self.shallow_propagate(subs);
                    }
                    dirty = true;
                }
            } else if flags & (ReactiveFlags::MUTABLE | ReactiveFlags::PENDING)
                == ReactiveFlags::MUTABLE | ReactiveFlags::PENDING
            {
                let deps = dep.deps.borrow().clone().unwrap();
                stack = Some(Box::new(Stack {
                    value: link,
                    prev: stack,
                }));
                link = deps;
                sub = dep;
                check_depth += 1;
                continue;
            }

            if !dirty {
                let next_dep = link.next_dep.borrow().clone();
                if let Some(next_dep) = next_dep {
                    link = next_dep;
                    continue;
                }
            }

            while check_depth > 0 {
                check_depth -= 1;
                let top = stack.unwrap();
                link = top.value;
                stack = top.prev;
                if dirty {
                    let subs = sub.subs.borrow().clone();
                    if (self.update)(&sub) {
                        let subs = subs.unwrap();
                        if subs.next_sub.borrow().is_some() {
                            self.shallow_propagate(subs);
                        }
                        sub = Rc::clone(&link.sub);
                        continue;
                    }
                    dirty = false;
                } else {
                    sub.flags.set(sub.flags.get() & !ReactiveFlags::PENDING);
                }
                sub = Rc::clone(&link.sub);
                let next_dep = link.next_dep.borrow().clone();
                if let Some(next_dep) = next_dep {
                    link = next_dep;
                    continue 'top;
                }
            }

            return dirty && sub.flags.get() != 0;
        }
    }

    fn shallow_propagate(&self, mut link: Rc<Link<K>>) {
        loop {
            let sub = &link.sub;
            let flags = sub.flags.get();
            if flags & (ReactiveFlags::PENDING | ReactiveFlags::DIRTY) == ReactiveFlags::PENDING {
                sub.flags.set(flags | ReactiveFlags::DIRTY);
                if flags & (ReactiveFlags::WATCHING | ReactiveFlags::RECURSED_CHECK)
                    == ReactiveFlags::WATCHING
                {
                    (self.notify)(sub);
                }
            }
            let Some(next_sub) = link.next_sub.borrow().clone() else {
                break;
            };
            link = next_sub;
        }
    }

    fn is_valid_link(&self, check_link: &Rc<Link<K>>, sub: &ReactiveNode<K>) -> bool {
        let mut link = sub.deps_tail.borrow().clone();
        while let Some(current) = link {
            if Rc::ptr_eq(&current, check_link) {
                return true;
            }
            link = current.prev_dep.borrow().clone();
        }
        false
    }
}
