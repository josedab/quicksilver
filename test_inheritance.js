// Test class inheritance with extends and super

class Animal {
    constructor(name) {
        this.name = name;
    }

    speak() {
        return this.name + " makes a sound";
    }
}

class Dog extends Animal {
    constructor(name, breed) {
        super(name);
        this.breed = breed;
    }

    speak() {
        return this.name + " barks!";
    }
}

// Test basic instantiation
let dog = new Dog("Rex", "German Shepherd");
console.log("Name:", dog.name);
console.log("Breed:", dog.breed);
console.log("Speak:", dog.speak());

// Test instanceof
console.log("dog instanceof Dog:", dog instanceof Dog);
console.log("dog instanceof Animal:", dog instanceof Animal);

// Test Animal directly
let animal = new Animal("Generic");
console.log("Animal name:", animal.name);
console.log("Animal speak:", animal.speak());
