// ES Modules Example - Math Module
// This module exports various math utilities

// Named exports
export const PI = 3.14159265359;
export const E = 2.71828182846;

export function add(a, b) {
    return a + b;
}

export function subtract(a, b) {
    return a - b;
}

export function multiply(a, b) {
    return a * b;
}

export function divide(a, b) {
    if (b === 0) {
        throw new Error("Division by zero");
    }
    return a / b;
}

export function square(x) {
    return x * x;
}

export function cube(x) {
    return x * x * x;
}

// Default export - a calculator object
export default {
    PI: PI,
    E: E,
    add: add,
    subtract: subtract,
    multiply: multiply,
    divide: divide,
    square: square,
    cube: cube
};
