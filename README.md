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

## Performance

Each update of a grid of computeds is timed, `w` chains of `h` computeds
under one signal, with one effect at the end of each chain. Creation is
timed as 100,000 signals, each with one computed and one effect. The
times are upstream's compiled ESM against the port's `src/index.js` on
Node 24, on a loaded machine, so compare the ratios.

The port's time as a multiple of upstream's, each run beside upstream's:

| | before rust-js #499 | after #499 | after #501 and #524 | after #500 |
|---|---|---|---|---|
| propagate 1×1 (2,000,000 updates) | 28× | 3.5× | 1.3–1.4× | 1.1–1.2× |
| propagate 10×10 | 60–100× | 2–3× | 1.0–1.3× | 1.1× |
| propagate 100×10 | | | 0.9–1.1× | 1.1× |
| propagate 10×100 | | | 1.2× | 1.05–1.1× |
| propagate 100×100 | 300× | 1.3–1.6× | 1.0–1.3× | 1.1× |
| create 100k signal+computed+effect | 30–70× | 2× | 1.4–2× | 1.35× |

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

#524 made `queued[a..b].reverse()` in `notify` a swap in place,
`$reverseRange(queued, a, b)`, where it had made a `Proxy` view on every
update. #501 put the `Finally` guards' closures' bodies in their
`finally` blocks.

Before #499, each momentary `RefCell` borrow of the graph's links was
checked and counted at runtime (`$borrow`/`$fieldCell`/`$unborrow`, a
`defineProperty` on every borrow), which was 62% of the profile. rust-js
now sees that `Option<Rc<Link<K>>>`'s `clone` runs no code that could
borrow the cell, and reads the fields directly, as upstream does.
