# alien-signals in Rust

[alien-signals](https://github.com/stackblitz/alien-signals) 3.2.1
(6562fb0), ported to Rust and compiled to JavaScript by
[rust-js](https://github.com/rust-js-lang/rust-js). `src/system.rs` and
`src/index.rs` are the port. `src/system.js`, `src/index.js` and their
`.d.ts` files are what rust-js writes from them. Upstream's test suite
runs against the generated JavaScript.

```sh
node ../rust-js/tooling/cli.js build   # rust-js is a checkout beside this one
npx vitest run                         # upstream's suite, 202 tests
```

## Differences from upstream

Where the generated JavaScript does what the Rust does, which upstream's
TypeScript doesn't check:

- **`Option::unwrap()` is Rust's check of upstream's `!`**: `const subs$1 =
  $unwrap(subs)` throws ``called `Option::unwrap()` on a `None` value``
  where upstream's `dep.subs!` would go on with `undefined` (rust-js #531,
  kept).
- **Counters wrap as `u32` and `i32` do**, `(insertIndex + 1) >>> 0`,
  `| 0`.
- **An index is checked**, `$index(queued, index)`, before `queued[index]`
  is read, as Rust's indexing panics past the end.
- **`let .. else { unreachable!() }`** throws Rust's ``internal error:
  entered unreachable code`` where upstream's node is of the kind it tests.
- **`take()` empties the slot before `unwrap()` checks it**, `const old =
  queued[index]; queued[index] = undefined; const effect = $unwrap(old);`,
  as Rust's `queued[i].take().unwrap()` does, where upstream reads the
  effect with `!` first.

And where Rust writes it otherwise: no assignment in a condition, `const
watched = nextSub != null`; no chained assignment; a `loop` whose end
tests a binding, `while (true) { .. break; }`, for upstream's `do { .. }
while ((link = link.nextSub!) !== undefined)`; and `Vec`'s growth by
`push`, `if (i < queued.length) queued[i] = e; else queued.push(e);`.

`NodeKind` is exported, as a `pub` type Rust's `get_active_sub` gives is.
And `unlink` reads a link's fields one `const` each, `const dep =
link.dep; const prevDep = link.prevDep;`, where upstream destructures
them, as each is a `let` of its own in Rust (rust-js ADR 0598, #534).

The user accepted each of these differences (2026-11-03).

## Performance

Each update of a grid of computeds is timed, `w` chains of `h` computeds
under one signal, with one effect at the end of each chain. Creation is
timed as 100,000 signals, each with one computed and one effect. The
times are upstream's compiled ESM against the port's `src/index.js` on
Node 24, on a loaded machine, so compare the ratios.

The port's time as a multiple of upstream's, each run beside upstream's:

| | before rust-js #499 | after #499 | after #501 and #524 | after #500 | after #503 (#542) |
|---|---|---|---|---|---|
| propagate 1×1 (2,000,000 updates) | 28× | 3.5× | 1.3–1.4× | 1.1–1.2× | 2.3–2.7× |
| propagate 10×10 | 60–100× | 2–3× | 1.0–1.3× | 1.1× | 1.5× |
| propagate 100×10 | | | 0.9–1.1× | 1.1× | 1.4× |
| propagate 10×100 | | | 1.2× | 1.05–1.1× | 1.1–1.2× |
| propagate 100×100 | 300× | 1.3–1.6× | 1.0–1.3× | 1.1× | 1.2× |
| create 100k signal+computed+effect | 30–70× | 2× | 1.4–2× | 1.35× | 3.3× (best of five 5×) |

Today's times, upstream then the port: 1×1 0.031–0.032 µs and 0.034–0.037
µs; 100×100 290–292 µs and 315–317 µs; creating 100k, 20 ms and 27 ms
(the best of five, 9 ms and 7 ms).

#500 made `createReactiveSystem`'s scope struct, `System<K>`, generic over
the node's kind, its functions: `link`, `propagate` and the rest are
declared in it, calling each other and its callbacks by name, `update(dep)`,
where each was a method of an object given the state,
`System.link(system, ..)`, calling `(0, system.update)(dep)`; and it
returns them, `{ link, unlink, propagate, checkDirty, shallowPropagate }`,
where each was an arrow calling it.

#503 made an effect's `cleanup` a plain field, as upstream's is, but each
of its borrows is a checked cell, `$fieldCell(e, "cleanup")`, which is all
of the last column's slowdown: with it written `e.cleanup`, each time is
as before (rust-js #542).

#533 made the module's functions that each call one of the system's,
`function link(dep, sub, version) { system.link(dep, sub, version); }`,
the system taken apart where it's made, upstream's `const { link, unlink,
propagate, checkDirty, shallowPropagate } = createReactiveSystem({ .. })`.

#524 made `queued[a..b].reverse()` in `notify` a swap in place,
`$reverseRange(queued, a, b)`, where it had made a `Proxy` view on every
update. #501 put the `Finally` guards' closures' bodies in their
`finally` blocks.

Before #499, each momentary `RefCell` borrow of the graph's links was
checked and counted at runtime (`$borrow`/`$fieldCell`/`$unborrow`, a
`defineProperty` on every borrow), which was 62% of the profile. rust-js
now sees that `Option<Rc<Link<K>>>`'s `clone` runs no code that could
borrow the cell, and reads the fields directly, as upstream does.
