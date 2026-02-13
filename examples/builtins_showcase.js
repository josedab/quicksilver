// Built-in Objects Showcase
// ==========================
// Demonstrates all built-in objects available in Quicksilver

console.log("Quicksilver Built-ins Demo\n");
console.log("=".repeat(50));

// ===================
// Console Object
// ===================
console.log("\n1. Console Object");
console.log("-".repeat(30));
console.log("Standard output");
console.warn("Warning message");
console.error("Error message");
console.info("Info message");
console.debug("Debug message");

// ===================
// Math Object
// ===================
console.log("\n2. Math Object");
console.log("-".repeat(30));
console.log("Math.PI:", Math.PI);
console.log("Math.E:", Math.E);
console.log("Math.abs(-5):", Math.abs(-5));
console.log("Math.floor(3.7):", Math.floor(3.7));
console.log("Math.ceil(3.2):", Math.ceil(3.2));
console.log("Math.round(3.5):", Math.round(3.5));
console.log("Math.max(1, 5, 3):", Math.max(1, 5, 3));
console.log("Math.min(1, 5, 3):", Math.min(1, 5, 3));
console.log("Math.pow(2, 8):", Math.pow(2, 8));
console.log("Math.sqrt(144):", Math.sqrt(144));
console.log("Math.random():", Math.random());
console.log("Math.sin(Math.PI/2):", Math.sin(Math.PI / 2));
console.log("Math.cos(0):", Math.cos(0));

// ===================
// JSON Object
// ===================
console.log("\n3. JSON Object");
console.log("-".repeat(30));
const obj = { name: "Quicksilver", version: "0.1.0", features: ["fast", "safe", "small"] };
const jsonStr = JSON.stringify(obj);
console.log("JSON.stringify:", jsonStr);

const parsed = JSON.parse(jsonStr);
console.log("JSON.parse:", parsed.name, parsed.version);

// Pretty printing
const pretty = JSON.stringify(obj, null, 2);
console.log("Pretty printed:", pretty);

// ===================
// Date Object
// ===================
console.log("\n4. Date Object");
console.log("-".repeat(30));
const now = new Date();
console.log("new Date():", now);
console.log("Date.now():", Date.now());

const specific = new Date(2024, 0, 1); // Jan 1, 2024
console.log("Specific date:", specific);
console.log("getFullYear():", now.getFullYear());
console.log("getMonth():", now.getMonth());
console.log("getDate():", now.getDate());
console.log("getHours():", now.getHours());

// ===================
// Array Object
// ===================
console.log("\n5. Array Object & Methods");
console.log("-".repeat(30));
const arr = [1, 2, 3, 4, 5];
console.log("Original:", arr);
console.log("length:", arr.length);
console.log("push(6):", arr.push(6), "->", arr);
console.log("pop():", arr.pop(), "->", arr);
console.log("join('-'):", arr.join("-"));
console.log("reverse():", [1, 2, 3].reverse());
console.log("indexOf(3):", arr.indexOf(3));
console.log("includes(3):", arr.includes(3));
console.log("slice(1, 3):", arr.slice(1, 3));

// Higher-order functions
const doubled = arr.map(x => x * 2);
console.log("map(x => x * 2):", doubled);

const evens = arr.filter(x => x % 2 === 0);
console.log("filter(x => x % 2 === 0):", evens);

const sum = arr.reduce((acc, x) => acc + x, 0);
console.log("reduce((acc, x) => acc + x):", sum);

console.log("forEach:", arr.forEach(x => console.log("  item:", x)));

// ===================
// Object Methods
// ===================
console.log("\n6. Object Methods");
console.log("-".repeat(30));
const person = { name: "Alice", age: 30, city: "NYC" };
console.log("Object.keys:", Object.keys(person));
console.log("Object.values:", Object.values(person));
console.log("Object.entries:", Object.entries(person));

// ===================
// String Methods
// ===================
console.log("\n7. String Methods");
console.log("-".repeat(30));
const str = "  Hello, Quicksilver!  ";
console.log("Original:", `'${str}'`);
console.log("trim():", `'${str.trim()}'`);
console.log("toUpperCase():", str.toUpperCase());
console.log("toLowerCase():", str.toLowerCase());
console.log("charAt(7):", str.charAt(7));
console.log("indexOf('Quick'):", str.indexOf("Quick"));
console.log("slice(2, 7):", str.slice(2, 7));
console.log("split(', '):", str.trim().split(", "));
console.log("replace('Quick', 'Super'):", str.replace("Quick", "Super"));
console.log("startsWith('  Hello'):", str.startsWith("  Hello"));
console.log("endsWith('!  '):", str.endsWith("!  "));

// Template literals
const name = "World";
console.log(`Template: Hello, ${name}!`);

// ===================
// Map Collection
// ===================
console.log("\n8. Map Collection");
console.log("-".repeat(30));
const map = new Map();
map.set("a", 1);
map.set("b", 2);
map.set("c", 3);
console.log("Map size:", map.size);
console.log("get('a'):", map.get("a"));
console.log("has('b'):", map.has("b"));
console.log("delete('c'):", map.delete("c"));
console.log("Map size after delete:", map.size);

// ===================
// Set Collection
// ===================
console.log("\n9. Set Collection");
console.log("-".repeat(30));
const mySet = new Set([1, 2, 2, 3, 3, 3]);
console.log("Set created from [1, 2, 2, 3, 3, 3]");
console.log("Set size:", mySet.size);
mySet.add(4);
console.log("has(2):", mySet.has(2));
console.log("has(5):", mySet.has(5));
mySet.delete(1);
console.log("delete(1) — has(1):", mySet.has(1));

// ===================
// Number & Type Checking
// ===================
console.log("\n10. Number & Type Utilities");
console.log("-".repeat(30));
console.log("Number.isNaN(NaN):", Number.isNaN(NaN));
console.log("Number.isFinite(100):", Number.isFinite(100));
console.log("Number.isInteger(3.14):", Number.isInteger(3.14));
console.log("Number.parseInt('42'):", Number.parseInt("42"));
console.log("Number.parseFloat('3.14'):", Number.parseFloat("3.14"));
console.log("isNaN('hello'):", isNaN("hello"));
console.log("typeof 42:", typeof 42);
console.log("typeof 'str':", typeof "str");
console.log("typeof {}:", typeof {});
console.log("typeof []:", typeof []);
console.log("Array.isArray([]):", Array.isArray([]));

console.log("\n" + "=".repeat(50));
console.log("Built-ins Demo Complete!");
