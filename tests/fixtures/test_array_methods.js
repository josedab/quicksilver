// Test array method accessibility
console.log("=== Array Method Access Test ===");

// Test that array methods are accessible as properties
let arr = [1, 2, 3];

console.log("Testing typeof array methods:");
console.log("typeof arr.push:", typeof arr.push);
console.log("typeof arr.pop:", typeof arr.pop);
console.log("typeof arr.map:", typeof arr.map);
console.log("typeof arr.filter:", typeof arr.filter);

// Test detached method calls
console.log("\nTesting bound method calls:");
let pushFn = arr.push;
pushFn(4);
console.log("After detached push(4):", arr.join(","));

let popFn = arr.pop;
let popped = popFn();
console.log("Popped value:", popped);
console.log("After detached pop:", arr.join(","));

// Test map with bound method
let nums = [1, 2, 3, 4, 5];
let mapFn = nums.map;
let doubled = mapFn(function(x) { return x * 2; });
console.log("Mapped doubles:", doubled.join(","));

// Test filter with bound method
let filterFn = nums.filter;
let evens = filterFn(function(x) { return x % 2 === 0; });
console.log("Filtered evens:", evens.join(","));

// Test string methods
console.log("\nTesting string method access:");
let str = "hello world";
console.log("typeof str.toUpperCase:", typeof str.toUpperCase);
console.log("typeof str.split:", typeof str.split);
console.log("typeof str.slice:", typeof str.slice);

// Test bound string method calls
let upperFn = str.toUpperCase;
console.log("Bound toUpperCase():", upperFn());

let splitFn = str.split;
let words = splitFn(" ");
console.log("Bound split(' '):", words.join(", "));

let sliceFn = str.slice;
console.log("Bound slice(0, 5):", sliceFn(0, 5));

console.log("\n=== Array Method Access Test Complete ===");
