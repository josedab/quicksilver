// Async Patterns in Quicksilver
// ================================
// Demonstrates async/await, promises, and structured concurrency

// Simulated async operations
async function fetchData(id) {
    // Simulating network delay
    return { id: id, data: `Record ${id}` };
}

async function processItem(item) {
    return { ...item, processed: true };
}

// Sequential async operations
async function sequential() {
    console.log("=== Sequential Execution ===");
    const result1 = await fetchData(1);
    const result2 = await fetchData(2);
    const result3 = await fetchData(3);
    console.log("Results:", result1, result2, result3);
    return [result1, result2, result3];
}

// Parallel async operations (Promise.all pattern)
async function parallel() {
    console.log("\n=== Parallel Execution ===");
    const promises = [
        fetchData(1),
        fetchData(2),
        fetchData(3)
    ];
    // Note: Promise.all would go here in full implementation
    console.log("All fetches started in parallel");
}

// Async iteration pattern
async function asyncIteration() {
    console.log("\n=== Async Pipeline ===");
    const items = [1, 2, 3, 4, 5];
    const results = [];

    for (const id of items) {
        const data = await fetchData(id);
        const processed = await processItem(data);
        results.push(processed);
        console.log(`Processed item ${id}`);
    }

    return results;
}

// Error handling in async code
async function errorHandling() {
    console.log("\n=== Error Handling ===");
    try {
        const result = await fetchData(1);
        console.log("Success:", result);

        // Simulated error
        throw new Error("Something went wrong!");
    } catch (error) {
        console.log("Caught error:", error.message);
    } finally {
        console.log("Cleanup complete");
    }
}

// Async timeout pattern
function timeout(ms) {
    return new Promise((resolve) => {
        // In real implementation, this would use setTimeout
        resolve(`Completed after ${ms}ms`);
    });
}

async function withTimeout() {
    console.log("\n=== Timeout Pattern ===");
    const result = await timeout(1000);
    console.log(result);
}

// Run all examples
async function main() {
    console.log("Quicksilver Async Patterns Demo\n");

    await sequential();
    await parallel();
    await asyncIteration();
    await errorHandling();
    await withTimeout();

    console.log("\n=== Demo Complete ===");
}

// Execute
main();
