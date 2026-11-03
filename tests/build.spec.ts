import { expect } from 'vitest';
import { test } from 'vitest';

declare function require(module: string): any;

test('build: cjs', () => {
	const index = require('../src/index.js');
	const system = require('../src/system.js');

	expect(typeof index.getActiveSub).toBe('function');
	expect(typeof system.createReactiveSystem).toBe('function');
});

test('build: esm', async () => {
	const index = await import('../src/index.js');
	const system = await import('../src/system.js');

	expect(typeof index.getActiveSub).toBe('function');
	expect(typeof system.createReactiveSystem).toBe('function');
});
