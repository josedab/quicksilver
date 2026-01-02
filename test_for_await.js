// Test for await...of
console.log("=== For Await...Of Test ===");

// Helper to create a resolved promise
function resolveWith(value) {
    return Promise.resolve(value);
}

// Test 1: Basic for await with array of promises
console.log("\nTest 1: Array of promises");
async function testArrayOfPromises() {
    let promises = [
        resolveWith(1),
        resolveWith(2),
        resolveWith(3)
    ];

    let results = [];
    for await (const value of promises) {
        console.log("Got value:", value);
        results.push(value);
    }
    console.log("All results:", results);
}
testArrayOfPromises();

// Test 2: For await with regular array (should work like regular for...of)
console.log("\nTest 2: Regular array");
async function testRegularArray() {
    let arr = [10, 20, 30];

    for await (const value of arr) {
        console.log("Regular value:", value);
    }
}
testRegularArray();

// Test 3: Mixed promises and values
console.log("\nTest 3: Mixed promises and values");
async function testMixed() {
    let mixed = [
        resolveWith("a"),
        "b",
        resolveWith("c"),
        "d"
    ];

    for await (const value of mixed) {
        console.log("Mixed value:", value);
    }
}
testMixed();

console.log("\n=== For Await...Of Test Complete ===");
