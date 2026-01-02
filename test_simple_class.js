// Test simple class without inheritance
class Animal {
    constructor(name) {
        this.name = name;
    }

    speak() {
        return this.name + " makes a sound";
    }
}

let animal = new Animal("Generic");
console.log("Name:", animal.name);
console.log("Speak:", animal.speak());
