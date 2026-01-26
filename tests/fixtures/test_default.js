// Test default parameters
function greet(name = "World") {
    return "Hello, " + name;
}
console.log("No arg:", greet());
console.log("With arg:", greet("Alice"));

function add(a, b = 10) {
    return a + b;
}
console.log("add(5):", add(5));
console.log("add(5, 3):", add(5, 3));
