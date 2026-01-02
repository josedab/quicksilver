// Test setTimeout and setInterval

console.log("=== Timer Test ===");

// Test setTimeout basic
let count = 0;
setTimeout(() => {
    count++;
    console.log("setTimeout callback 1 executed, count:", count);
}, 100);

setTimeout(() => {
    count++;
    console.log("setTimeout callback 2 executed, count:", count);
}, 200);

setTimeout(() => {
    count++;
    console.log("setTimeout callback 3 executed, count:", count);
}, 50);

console.log("Timers scheduled, count before callbacks:", count);

// Test with extra arguments
setTimeout((a, b) => {
    console.log("setTimeout with args:", a, b);
}, 150, "hello", "world");

// Test setInterval (should run 3 times before clearInterval)
let intervalCount = 0;
let intervalId = setInterval(() => {
    intervalCount++;
    console.log("setInterval callback, intervalCount:", intervalCount);
    if (intervalCount >= 3) {
        clearInterval(intervalId);
        console.log("Interval cleared after 3 executions");
    }
}, 75);

// Test clearTimeout
let cancelledTimer = setTimeout(() => {
    console.log("This should NOT appear - timer was cancelled");
}, 125);
clearTimeout(cancelledTimer);
console.log("Timer", cancelledTimer, "cancelled");

// Test queueMicrotask
queueMicrotask(() => {
    console.log("Microtask 1 executed");
});

queueMicrotask(() => {
    console.log("Microtask 2 executed");
});

console.log("=== End of Main Script ===");
