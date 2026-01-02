// Hello World - Quicksilver JavaScript Runtime
// Run with: cargo run -- examples/hello_world.js

console.log("Hello from Quicksilver! 🚀");

// Basic operations
const greeting = "Welcome to the future of JavaScript runtimes";
console.log(greeting);

// Numbers and Math
const result = Math.sqrt(16) + Math.pow(2, 3);
console.log("Math result:", result);

// Template literals
const name = "Quicksilver";
const version = "0.1.0";
console.log(`Running ${name} v${version}`);

// Arrays and iteration
const features = [
  "Memory-safe (Rust)",
  "Fast cold starts",
  "Time-travel debugging",
  "Capability security"
];

console.log("\nKey Features:");
for (let i = 0; i < features.length; i++) {
  console.log(`  ${i + 1}. ${features[i]}`);
}

// Objects and destructuring
const runtime = {
  name: "Quicksilver",
  language: "JavaScript",
  engine: "Custom bytecode VM",
  written_in: "Rust"
};

const { name: rtName, written_in } = runtime;
console.log(`\n${rtName} is written in ${written_in}`);

// Functions
function factorial(n) {
  if (n <= 1) return 1;
  return n * factorial(n - 1);
}

console.log("\nFactorial(10):", factorial(10));

// Arrow functions
const double = x => x * 2;
console.log("Double 5:", double(5));

// Classes
class Point {
  constructor(x, y) {
    this.x = x;
    this.y = y;
  }
}

const p = new Point(3, 4);
console.log("Point:", p.x, p.y);
console.log("Distance from origin:", Math.sqrt(p.x * p.x + p.y * p.y));

console.log("\n✅ All examples completed successfully!");
