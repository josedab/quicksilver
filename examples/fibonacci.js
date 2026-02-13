// Fibonacci Implementations
// Demonstrates different algorithmic approaches
// Run with: cargo run -- examples/fibonacci.js

console.log("Fibonacci Number Implementations\n");

// 1. Recursive (classic but slow for large n)
function fibRecursive(n) {
  if (n <= 1) return n;
  return fibRecursive(n - 1) + fibRecursive(n - 2);
}

// 2. Memoized (fast with caching)
const fibCache = new Map();
function fibMemoized(n) {
  if (n <= 1) return n;
  if (fibCache.has(n)) return fibCache.get(n);

  const result = fibMemoized(n - 1) + fibMemoized(n - 2);
  fibCache.set(n, result);
  return result;
}

// Test the implementations
const n = 20;

console.log(`Computing Fibonacci(${n}):\n`);

// Recursive
const startRec = Date.now();
const resultRec = fibRecursive(n);
const timeRec = Date.now() - startRec;
console.log(`  Recursive:  ${resultRec} (${timeRec}ms)`);

// Memoized
const startMemo = Date.now();
const resultMemo = fibMemoized(n);
const timeMemo = Date.now() - startMemo;
console.log(`  Memoized:   ${resultMemo} (${timeMemo}ms)`);

// First 15 Fibonacci numbers using memoized
console.log("\nFirst 15 Fibonacci numbers:");
const sequence = [];
for (let i = 0; i < 15; i++) {
  sequence.push(fibMemoized(i));
}
console.log(sequence.join(", "));

// Sum of first N Fibonacci numbers
function fibSum(count) {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    sum = sum + fibMemoized(i);
  }
  return sum;
}

console.log(`\nSum of first 15 Fibonacci numbers: ${fibSum(15)}`);

// Golden ratio approximation
const f20 = fibMemoized(20);
const f19 = fibMemoized(19);
console.log(`\nGolden ratio approximation (F20/F19): ${f20 / f19}`);
console.log(`Actual golden ratio: ${(1 + Math.sqrt(5)) / 2}`);
