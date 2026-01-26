// Test 2: Spread in function calls
function sum(a, b, c) {
    return a + b + c;
}
var nums = [1, 2, 3];
console.log("Test 2 - Spread call:", sum(...nums));

// Test 3: Rest parameters
function sumAll(...numbers) {
    var total = 0;
    for (var i = 0; i < numbers.length; i++) {
        total = total + numbers[i];
    }
    return total;
}
console.log("Test 3 - Rest params:", sumAll(1, 2, 3, 4, 5));

// Test 4: Optional chaining
var obj = { nested: { value: 42 } };
console.log("Test 4 - Optional chain:", obj?.nested?.value);
console.log("Test 4 - Null chain:", obj?.missing?.value);

// Test 5: Nullish coalescing
var x = null;
var y = undefined;
var z = 0;
console.log("Test 5 - Null ??:", x ?? "default");
console.log("Test 5 - Undefined ??:", y ?? "default");
console.log("Test 5 - Zero ??:", z ?? "default");

// Test 6: instanceof
class Animal {}
var dog = new Animal();
console.log("Test 6 - instanceof:", dog instanceof Animal);

// Test 7: for...of
var arr = [10, 20, 30];
var forOfResult = [];
for (var item of arr) {
    forOfResult.push(item);
}
console.log("Test 7 - for...of:", forOfResult.join(","));

// Test 8: switch statement
function getDay(n) {
    switch(n) {
        case 0: return "Sunday";
        case 1: return "Monday";
        default: return "Other";
    }
}
console.log("Test 8 - switch:", getDay(1));
