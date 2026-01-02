// Fibonacci Implementations
// Demonstrates different algorithmic approaches
// Run with: cargo run -- examples/fibonacci.js

console.log("Fibonacci Number Implementations\n");

// 1. Recursive (classic but slow)
function fibRecursive(n) {
  if (n <= 1) return n;
  return fibRecursive(n - 1) + fibRecursive(n - 2);
}

// 2. Iterative (fast)
function fibIterative(n) {
  if (n <= 1) return n;
  let a = 0, b = 1;
  for (let i = 2; i <= n; i++) {
    const temp = a + b;
    a = b;
    b = temp;
  }
  return b;
}

// 3. Memoized (fast with caching)
function fibMemoized() {
  const cache = new Map();

  return function fib(n) {
    if (n <= 1) return n;
    if (cache.has(n)) return cache.get(n);

    const result = fib(n - 1) + fib(n - 2);
    cache.set(n, result);
    return result;
  };
}

// 4. Generator-based
function* fibGenerator() {
  let a = 0, b = 1;
  while (true) {
    yield a;
    const temp = a + b;
    a = b;
    b = temp;
  }
}

// Test the implementations
const n = 20;

console.log(`Computing Fibonacci(${n}):\n`);

// Recursive
const startRec = Date.now();
const resultRec = fibRecursive(n);
const timeRec = Date.now() - startRec;
console.log(`  Recursive:  ${resultRec} (${timeRec}ms)`);

// Iterative
const startIter = Date.now();
const resultIter = fibIterative(n);
const timeIter = Date.now() - startIter;
console.log(`  Iterative:  ${resultIter} (${timeIter}ms)`);

// Memoized
const fib = fibMemoized();
const startMemo = Date.now();
const resultMemo = fib(n);
const timeMemo = Date.now() - startMemo;
console.log(`  Memoized:   ${resultMemo} (${timeMemo}ms)`);

// First 20 Fibonacci numbers
console.log("\nFirst 20 Fibonacci numbers:");
const sequence = [];
for (let i = 0; i < 20; i++) {
  sequence.push(fibIterative(i));
}
console.log(sequence.join(", "));

// Sum of first N Fibonacci numbers
function fibSum(n) {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    sum += fibIterative(i);
  }
  return sum;
}

console.log(`\nSum of first 20 Fibonacci numbers: ${fibSum(20)}`);

// Golden ratio approximation
const f40 = fibIterative(40);
const f39 = fibIterative(39);
const phi = f40 / f39;
console.log(`\nGolden ratio approximation (F40/F39): ${phi}`);
console.log(`Actual golden ratio: ${(1 + Math.sqrt(5)) / 2}`);
