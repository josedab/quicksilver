// ES Modules Example - Main Entry Point
// Demonstrates various import syntaxes

// Named imports
import { PI, E, add, multiply, square } from './math.js';

// Default import
import Calculator from './math.js';

// Namespace import (imports all exports as an object)
import * as MathUtils from './math.js';

console.log("=== ES Modules Demo ===\n");

// Using named imports
console.log("Named imports:");
console.log("PI =", PI);
console.log("E =", E);
console.log("add(2, 3) =", add(2, 3));
console.log("multiply(4, 5) =", multiply(4, 5));
console.log("square(7) =", square(7));

// Using default import
console.log("\nDefault import (Calculator):");
console.log("Calculator.add(10, 20) =", Calculator.add(10, 20));
console.log("Calculator.cube(3) =", Calculator.cube(3));

// Using namespace import
console.log("\nNamespace import (MathUtils):");
console.log("MathUtils.PI =", MathUtils.PI);
console.log("MathUtils.divide(100, 4) =", MathUtils.divide(100, 4));

// Calculate area of a circle
const radius = 5;
const area = multiply(PI, square(radius));
console.log("\nCircle with radius", radius);
console.log("Area =", area);

console.log("\n=== ES Modules Demo Complete ===");
