// ES6 Classes Demonstration
// Shows class syntax, inheritance, and design patterns
// Run with: cargo run -- examples/classes.js

console.log("ES6 Classes Demonstration\n");

// Basic Class
class Animal {
  constructor(name, species) {
    this.name = name;
    this.species = species;
  }

  speak() {
    console.log(`${this.name} makes a sound`);
  }

  describe() {
    return `${this.name} is a ${this.species}`;
  }
}

// Inheritance
class Dog extends Animal {
  constructor(name, breed) {
    super(name, "Dog");
    this.breed = breed;
  }

  speak() {
    console.log(`${this.name} barks!`);
  }

  fetch() {
    console.log(`${this.name} fetches the ball`);
  }
}

class Cat extends Animal {
  constructor(name, color) {
    super(name, "Cat");
    this.color = color;
  }

  speak() {
    console.log(`${this.name} meows!`);
  }

  climb() {
    console.log(`${this.name} climbs the tree`);
  }
}

// Test basic classes
console.log("1. Basic Inheritance:");
const dog = new Dog("Rex", "German Shepherd");
const cat = new Cat("Whiskers", "Orange");

dog.speak();
cat.speak();
dog.fetch();
cat.climb();
console.log(dog.describe());
console.log(cat.describe());

// Builder Pattern
class QueryBuilder {
  constructor() {
    this.query = { select: "*", from: "", where: [], orderBy: "" };
  }

  select(fields) {
    this.query.select = fields;
    return this;
  }

  from(table) {
    this.query.from = table;
    return this;
  }

  where(condition) {
    this.query.where.push(condition);
    return this;
  }

  orderBy(field) {
    this.query.orderBy = field;
    return this;
  }

  build() {
    let sql = `SELECT ${this.query.select} FROM ${this.query.from}`;
    if (this.query.where.length > 0) {
      sql += ` WHERE ${this.query.where.join(" AND ")}`;
    }
    if (this.query.orderBy) {
      sql += ` ORDER BY ${this.query.orderBy}`;
    }
    return sql;
  }
}

console.log("\n2. Builder Pattern:");
const query = new QueryBuilder()
  .select("id, name, email")
  .from("users")
  .where("active = true")
  .where("role = 'admin'")
  .orderBy("name")
  .build();
console.log("Generated SQL:", query);

// State Machine
class TrafficLight {
  constructor() {
    this.states = ["red", "yellow", "green"];
    this.currentIndex = 0;
  }

  get current() {
    return this.states[this.currentIndex];
  }

  next() {
    this.currentIndex = (this.currentIndex + 1) % this.states.length;
    return this.current;
  }

  reset() {
    this.currentIndex = 0;
    return this.current;
  }
}

console.log("\n3. State Machine:");
const light = new TrafficLight();
console.log("Current:", light.current);
console.log("Next:", light.next());
console.log("Next:", light.next());
console.log("Next:", light.next());
console.log("Reset:", light.reset());

// Observable Pattern
class EventEmitter {
  constructor() {
    this.events = {};
  }

  on(event, listener) {
    if (!this.events[event]) {
      this.events[event] = [];
    }
    this.events[event].push(listener);
    return this;
  }

  emit(event, data) {
    const listeners = this.events[event];
    if (listeners) {
      for (const listener of listeners) {
        listener(data);
      }
    }
    return this;
  }

  off(event, listener) {
    const listeners = this.events[event];
    if (listeners) {
      const index = listeners.indexOf(listener);
      if (index > -1) {
        listeners.splice(index, 1);
      }
    }
    return this;
  }
}

console.log("\n4. Event Emitter:");
const emitter = new EventEmitter();

emitter.on("data", (data) => {
  console.log("Received:", data);
});

emitter.on("data", (data) => {
  console.log("Also received:", data.toUpperCase());
});

emitter.emit("data", "hello world");

// Collection Class
class Stack {
  constructor() {
    this.items = [];
  }

  push(item) {
    this.items.push(item);
    return this;
  }

  pop() {
    return this.items.pop();
  }

  peek() {
    return this.items[this.items.length - 1];
  }

  isEmpty() {
    return this.items.length === 0;
  }

  size() {
    return this.items.length;
  }
}

console.log("\n5. Stack Data Structure:");
const stack = new Stack();
stack.push(1).push(2).push(3);
console.log("Size:", stack.size());
console.log("Peek:", stack.peek());
console.log("Pop:", stack.pop());
console.log("Pop:", stack.pop());
console.log("Size after pops:", stack.size());

console.log("\n✅ All class examples completed!");
