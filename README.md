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

| | upstream | before rust-js #499 | after #499 |
|---|---|---|---|
| propagate 1×1 | 0.1 µs | 2.8 µs | 0.3 µs |
| propagate 10×10 | 2.0–3.6 µs | 199 µs | 5.6–6.8 µs |
| propagate 100×100 | 0.42–0.51 ms | 149 ms | 0.65–0.67 ms |
| create 100k signal+computed+effect | 44 ms | 1.4–3 s | 86–91 ms |

Before #499, each momentary `RefCell` borrow of the graph's links was
checked and counted at runtime (`$borrow`/`$fieldCell`/`$unborrow`, a
`defineProperty` on every borrow), which was 62% of the profile. rust-js
now sees that `Option<Rc<Link<K>>>`'s `clone` runs no code that could
borrow the cell, and reads the fields directly, as upstream does.
