// Test Date
console.log("=== Date Tests ===");

// Date.now()
var now = Date.now();
console.log("Date.now() returns a number:", typeof now === "number");

// new Date()
var d = new Date();
console.log("new Date() type:", typeof d);
console.log("new Date() getTime():", typeof d.getTime() === "number");

// new Date(timestamp)
var d2 = new Date(0);
console.log("new Date(0) ISO:", d2.toISOString());
console.log("new Date(0) year:", d2.getFullYear());
console.log("new Date(0) month:", d2.getMonth());
console.log("new Date(0) date:", d2.getDate());

// new Date(year, month, day)
var d3 = new Date(2024, 0, 15);
console.log("new Date(2024, 0, 15) year:", d3.getFullYear());
console.log("new Date(2024, 0, 15) month:", d3.getMonth());
console.log("new Date(2024, 0, 15) date:", d3.getDate());

// Date.parse
var ts = Date.parse("2024-06-15T12:30:00.000Z");
console.log("Date.parse result:", typeof ts === "number");

// === Map Tests ===
console.log("\n=== Map Tests ===");

var map = new Map();
console.log("new Map() created:", typeof map === "object");

map.set("key1", "value1");
map.set("key2", 42);
console.log("map.get('key1'):", map.get("key1"));
console.log("map.get('key2'):", map.get("key2"));
console.log("map.has('key1'):", map.has("key1"));
console.log("map.has('nonexistent'):", map.has("nonexistent"));
console.log("map.size():", map.size());

map.delete("key1");
console.log("after delete, map.has('key1'):", map.has("key1"));

// === Set Tests ===
console.log("\n=== Set Tests ===");

var mySet = new Set();
console.log("new Set() created:", typeof mySet === "object");

mySet.add(1);
mySet.add(2);
mySet.add(3);
mySet.add(2); // duplicate, should not be added
console.log("set.has(1):", mySet.has(1));
console.log("set.has(2):", mySet.has(2));
console.log("set.has(4):", mySet.has(4));
console.log("set.size():", mySet.size()); // Should be 3

mySet.delete(2);
console.log("after delete, set.has(2):", mySet.has(2));
console.log("after delete, set.size():", mySet.size()); // Should be 2

mySet.clear();
console.log("after clear, set.size():", mySet.size()); // Should be 0

console.log("\nAll tests completed!");
