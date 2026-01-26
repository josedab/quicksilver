// Simple map test
console.log("=== Map Test ===");

let nums = [1, 2, 3];

// Test regular map call (via CallMethod opcode)
let doubled = nums.map(function(x) { return x * 2; });
console.log("Regular map:", typeof doubled, doubled.length);
console.log("Elements:", doubled[0], doubled[1], doubled[2]);

// Test bound map call
let mapFn = nums.map;
console.log("mapFn type:", typeof mapFn);

let tripled = mapFn(function(x) { return x * 3; });
console.log("Bound map:", typeof tripled);
if (typeof tripled === "object") {
    console.log("Bound map length:", tripled.length);
}

console.log("=== Map Test Done ===");
