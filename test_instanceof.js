// Test instanceof
class Animal {
    constructor(name) {
        this.name = name;
    }
}

class Dog {
    constructor(name) {
        this.name = name;
    }
}

var dog = new Animal("Buddy");
console.log("dog instanceof Animal:", dog instanceof Animal);
console.log("dog instanceof Dog:", dog instanceof Dog);

// Test with plain object
var obj = { name: "test" };
console.log("obj instanceof Animal:", obj instanceof Animal);
