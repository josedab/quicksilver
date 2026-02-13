// ES6 Classes Demonstration
// Shows class syntax, inheritance, and common patterns
// Run with: cargo run -- examples/classes.js

console.log("ES6 Classes Demonstration\n");

// ===========================
// 1. Basic Class
// ===========================
class Animal {
  constructor(name, species) {
    this.name = name;
    this.species = species;
    this.sound = "...";
  }

  speak() {
    console.log(`${this.name} says ${this.sound}`);
  }

  describe() {
    return `${this.name} is a ${this.species}`;
  }
}

console.log("1. Basic Class:");
const animal = new Animal("Buddy", "Dog");
animal.speak();
console.log(animal.describe());

// ===========================
// 2. Inheritance with super()
// ===========================
class Dog extends Animal {
  constructor(name, breed) {
    super(name, "Dog");
    this.breed = breed;
    this.sound = "Woof!";
  }

  fetch() {
    console.log(`${this.name} the ${this.breed} fetches the ball`);
  }
}

class Cat extends Animal {
  constructor(name, color) {
    super(name, "Cat");
    this.color = color;
    this.sound = "Meow!";
  }

  describe() {
    return `${this.name} is a ${this.color} cat`;
  }
}

console.log("\n2. Inheritance:");
const dog = new Dog("Rex", "German Shepherd");
const cat = new Cat("Whiskers", "orange");

dog.speak();
cat.speak();
dog.fetch();
console.log(dog.describe());
console.log(cat.describe());

// instanceof checks
console.log("dog instanceof Dog:", dog instanceof Dog);
console.log("dog instanceof Animal:", dog instanceof Animal);
console.log("cat instanceof Dog:", cat instanceof Dog);

// ===========================
// 3. Class with computed values
// ===========================
class Rectangle {
  constructor(width, height) {
    this.width = width;
    this.height = height;
  }

  area() {
    return this.width * this.height;
  }

  perimeter() {
    return 2 * (this.width + this.height);
  }

  describe() {
    return `Rectangle(${this.width}x${this.height}), area=${this.area()}, perimeter=${this.perimeter()}`;
  }
}

console.log("\n3. Computed Values:");
const rect = new Rectangle(5, 3);
console.log(rect.describe());
console.log("Area:", rect.area());

// ===========================
// 4. Class hierarchy depth
// ===========================
class Shape {
  constructor(type) {
    this.type = type;
  }

  toString() {
    return `[Shape: ${this.type}]`;
  }
}

class Circle extends Shape {
  constructor(radius) {
    super("circle");
    this.radius = radius;
  }

  area() {
    return Math.PI * this.radius * this.radius;
  }

  toString() {
    return `Circle(r=${this.radius})`;
  }
}

console.log("\n4. Shape Hierarchy:");
const circle = new Circle(5);
console.log(circle.toString());
console.log("Area:", circle.area());
console.log("Type:", circle.type);
console.log("instanceof Shape:", circle instanceof Shape);

// ===========================
// 5. Data classes
// ===========================
class Point {
  constructor(x, y) {
    this.x = x;
    this.y = y;
  }

  distanceTo(other) {
    const dx = this.x - other.x;
    const dy = this.y - other.y;
    return Math.sqrt(dx * dx + dy * dy);
  }

  toString() {
    return `(${this.x}, ${this.y})`;
  }
}

console.log("\n5. Data Classes:");
const p1 = new Point(0, 0);
const p2 = new Point(3, 4);
console.log(`Distance from ${p1.toString()} to ${p2.toString()}: ${p1.distanceTo(p2)}`);

// ===========================
// 6. Error subclassing
// ===========================
class ValidationError extends Error {
  constructor(field, message) {
    super(message);
    this.field = field;
  }
}

console.log("\n6. Custom Errors:");
try {
  throw new ValidationError("email", "Invalid email format");
} catch (e) {
  console.log("Caught:", e.message);
  console.log("Field:", e.field);
  console.log("Is Error:", e instanceof Error);
}

console.log("\n✅ All class examples completed!");

