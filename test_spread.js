// Test object spread
console.log("=== Object Spread Test ===");

// Basic spread
let a = { x: 1, y: 2 };
let b = { ...a, z: 3 };
console.log("Basic spread:", b.x, b.y, b.z);

// Multiple spreads
let c = { a: 1 };
let d = { b: 2 };
let e = { ...c, ...d, c: 3 };
console.log("Multiple spreads:", e.a, e.b, e.c);

// Spread overwrites earlier properties
let f = { x: 100 };
let g = { ...f };
g.y = 200;
console.log("Copy object:", g.x, g.y);
console.log("Original unchanged:", f.x, f.y);

// Spread with override
let h = { x: 1, y: 2, z: 3 };
let i = { ...h, y: 100 };
console.log("Override during spread:", i.x, i.y, i.z);

// Later spread overwrites earlier
let j = { x: 1, y: 2 };
let k = { x: 10, y: 20 };
let m = { ...j, ...k };
console.log("Later spread wins:", m.x, m.y);

// Spread empty object
let n = { ...{}, x: 1 };
console.log("Spread empty:", n.x);

// Nested object (shallow copy)
let o = { nested: { value: 42 } };
let p = { ...o };
console.log("Nested object:", p.nested.value);

console.log("=== Object Spread Test Complete ===");
