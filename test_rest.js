// Test rest parameters
function sumAll(...numbers) {
    var total = 0;
    for (var i = 0; i < numbers.length; i++) {
        total = total + numbers[i];
    }
    return total;
}
console.log("Rest sum:", sumAll(1, 2, 3, 4, 5));
console.log("Rest empty:", sumAll());

// Test rest with regular params
function greet(greeting, ...names) {
    return greeting + " " + names.join(" and ");
}
console.log("Mixed rest:", greet("Hello", "Alice", "Bob", "Charlie"));
