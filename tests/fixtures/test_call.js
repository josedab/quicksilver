// Test calling the method
class Animal {
    constructor(name) {
        this.name = name;
    }

    speak() {
        return "sound";
    }
}

let a = new Animal("Bob");
let method = a.speak;
console.log("Got method:", method);

// Try direct function call
let result = method();
console.log("Result:", result);
