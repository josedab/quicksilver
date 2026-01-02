// ES Modules Example - Utilities Module
// Demonstrates re-exports and combined exports

// Re-export specific items from another module
export { PI, E } from './math.js';

// Re-export with renaming
export { add as sum, subtract as difference } from './math.js';

// Local exports combined with re-exports
export function formatNumber(n, decimals = 2) {
    return n.toFixed(decimals);
}

export function isEven(n) {
    return n % 2 === 0;
}

export function isOdd(n) {
    return n % 2 !== 0;
}

export function factorial(n) {
    if (n <= 1) return 1;
    return n * factorial(n - 1);
}

export function fibonacci(n) {
    if (n <= 1) return n;
    let a = 0, b = 1;
    for (let i = 2; i <= n; i++) {
        let temp = a + b;
        a = b;
        b = temp;
    }
    return b;
}

// Default export
export default {
    formatNumber,
    isEven,
    isOdd,
    factorial,
    fibonacci
};
