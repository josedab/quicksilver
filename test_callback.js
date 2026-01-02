// Test callback invocation
console.log("=== Callback Test ===");

// Simple function test
function double(x) {
    return x * 2;
}

console.log("double(5):", double(5));

// Test with forEach (doesn't return values)
let arr = [1, 2, 3];
let sum = 0;
arr.forEach(function(x) {
    sum = sum + x;
});
console.log("forEach sum:", sum);

// Test with reduce
let total = arr.reduce(function(acc, x) {
    return acc + x;
}, 0);
console.log("reduce total:", total);

// Test with filter
let evens = [1, 2, 3, 4].filter(function(x) {
    return x % 2 === 0;
});
console.log("filter result type:", typeof evens);
if (typeof evens === "object") {
    console.log("filter result:", evens.length, evens[0], evens[1]);
}

console.log("=== Callback Test Done ===");
