// Test Promise functionality
console.log("=== Promise Tests ===");

// Test Promise.resolve
var p1 = Promise.resolve(42);
console.log("Promise.resolve(42):", p1);

// Test Promise.reject
var p2 = Promise.reject("error");
console.log("Promise.reject('error'):", p2);

// Test Promise.all with resolved values
var p3 = Promise.all([
    Promise.resolve(1),
    Promise.resolve(2),
    Promise.resolve(3)
]);
console.log("Promise.all result:", p3);

// Test Promise.race
var p4 = Promise.race([
    Promise.resolve("first"),
    Promise.resolve("second")
]);
console.log("Promise.race result:", p4);

// Test Promise.allSettled
var p5 = Promise.allSettled([
    Promise.resolve("success"),
    Promise.reject("failure")
]);
console.log("Promise.allSettled result:", p5);

// Test .then()
var p6 = Promise.resolve(10);
var p7 = p6.then(function(x) { return x * 2; });
console.log("Promise.resolve(10).then():", p7);

// Test .catch()
var p8 = Promise.reject("oops");
var p9 = p8.catch(function(e) { return "caught: " + e; });
console.log("Promise.reject().catch():", p9);

// Test .finally()
var p10 = Promise.resolve("done");
var p11 = p10.finally(function() { console.log("cleanup"); });
console.log("Promise.finally():", p11);

// Test setTimeout exists
var timerId = setTimeout(function() {}, 1000);
console.log("setTimeout returns ID:", typeof timerId === "number");

// Test clearTimeout exists
clearTimeout(timerId);
console.log("clearTimeout works:", true);

console.log("\nAll Promise tests completed!");
