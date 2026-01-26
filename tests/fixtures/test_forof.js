// Test for...of
var arr = [10, 20, 30];
var result = [];
for (var item of arr) {
    result.push(item);
}
console.log("for...of result:", result.join(","));

// Test for...of with strings
var str = "abc";
var chars = [];
for (var c of str) {
    chars.push(c);
}
console.log("for...of string:", chars.join("-"));
