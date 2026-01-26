// Test enhanced console methods
console.log("=== Console Enhancement Test ===");

// Test console.table with array
console.log("\nTesting console.table with array:");
console.table([1, 2, 3, 4, 5]);

// Test console.table with object
console.log("\nTesting console.table with object:");
let obj = { name: "Alice", age: 30, city: "NYC" };
console.table(obj);

// Test console.group and groupEnd
console.log("\nTesting console.group:");
console.group("User Info");
console.log("Name: Alice");
console.log("Age: 30");
console.group("Address");
console.log("City: NYC");
console.log("Country: USA");
console.groupEnd();
console.log("Status: Active");
console.groupEnd();
console.log("Back to normal");

// Test console.time and timeEnd
console.log("\nTesting console.time:");
console.time("loop");
let sum = 0;
for (let i = 0; i < 1000; i++) {
    sum = sum + i;
}
console.timeEnd("loop");
console.log("Sum:", sum);

// Test console.count
console.log("\nTesting console.count:");
for (let i = 0; i < 3; i++) {
    console.count("iteration");
}
console.countReset("iteration");
console.count("iteration");

// Test console.assert
console.log("\nTesting console.assert:");
console.assert(true, "This should NOT print");
console.assert(false, "This SHOULD print");
console.assert(1 === 2, "Math is broken!");

// Test different log levels
console.log("\nTesting log levels:");
console.log("This is a log message");
console.info("This is an info message");
console.debug("This is a debug message");
console.warn("This is a warning");
console.error("This is an error");

// Test console.dir
console.log("\nTesting console.dir:");
console.dir({ nested: { value: 42 }, arr: [1, 2, 3] });

console.log("\n=== Console Enhancement Test Complete ===");
