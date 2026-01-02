// Test for...of inside a function
function testForOf() {
    var arr = [10, 20, 30];
    var result = [];
    for (var item of arr) {
        result.push(item);
    }
    return result.join(",");
}
console.log("for...of in function:", testForOf());
