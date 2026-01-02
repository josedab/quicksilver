// Security & Sandboxing in Quicksilver
// =====================================
// Demonstrates capability-based security model

/**
 * Quicksilver uses a capability-based security model inspired by Deno.
 * Code runs in a sandbox by default with no permissions.
 * Permissions must be explicitly granted.
 *
 * Permission types:
 * - FileRead: Access to read files from specific paths
 * - FileWrite: Access to write files to specific paths
 * - Network: Access to specific network hosts
 * - Env: Access to environment variables
 * - Subprocess: Ability to spawn child processes
 * - DynamicCode: eval() and new Function()
 * - HighResTime: High-resolution timing (security sensitive)
 */

// Example: File System with Permissions
// In Rust, you would create a sandbox like:
//
// let sandbox = Sandbox::new()
//     .allow_read(&["./data"])
//     .allow_write(&["./output"])
//     .deny_net_all();
//
// runtime.execute_with_sandbox(code, sandbox)?;

console.log("=== Security Sandbox Demo ===\n");

// Safe file operations within allowed paths
function safeFileOps() {
    console.log("1. File System Security");
    console.log("   - Read allowed: ./data/*");
    console.log("   - Write allowed: ./output/*");
    console.log("   - Denied: /etc/passwd, /home/*\n");

    // These would succeed with proper permissions:
    // await Deno.readTextFile("./data/config.json");
    // await Deno.writeTextFile("./output/results.txt", data);

    // This would fail (permission denied):
    // await Deno.readTextFile("/etc/passwd");  // PermissionDenied
}

// Network security
function networkSecurity() {
    console.log("2. Network Security");
    console.log("   - Allowed hosts: api.example.com, cdn.example.com");
    console.log("   - Denied: All other hosts\n");

    // Allowed:
    // await fetch("https://api.example.com/data");

    // Denied:
    // await fetch("https://evil.com/steal"); // PermissionDenied
}

// Environment variable security
function envSecurity() {
    console.log("3. Environment Variables");
    console.log("   - Allowed: PATH, HOME, USER");
    console.log("   - Denied: AWS_*, API_KEY, SECRETS\n");

    // Allowed:
    // Deno.env.get("HOME");

    // Denied:
    // Deno.env.get("AWS_SECRET_KEY"); // PermissionDenied
}

// Subprocess security
function subprocessSecurity() {
    console.log("4. Subprocess Execution");
    console.log("   - By default: DENIED");
    console.log("   - Must explicitly: .allow_subprocess()\n");

    // Only works if subprocess is allowed:
    // await Deno.run({ cmd: ["ls", "-la"] });
}

// Resource limits
function resourceLimits() {
    console.log("5. Resource Limits");
    console.log("   - memory_limit: 128MB");
    console.log("   - time_limit: 30000ms (30 seconds)");
    console.log("   - stack_limit: 1000 calls\n");

    // In Rust:
    // let sandbox = Sandbox::new()
    //     .with_memory_limit(128 * 1024 * 1024)
    //     .with_time_limit(30000)
    //     .with_stack_limit(1000);
}

// Permission checker example
function permissionChecking() {
    console.log("6. Permission States");
    console.log("   - Granted: Explicitly allowed");
    console.log("   - Denied: Explicitly denied");
    console.log("   - Prompt: Interactive mode (ask user)\n");
}

// Defense in depth
function defenseInDepth() {
    console.log("7. Defense in Depth");
    console.log("   - Memory safety: Rust prevents buffer overflows");
    console.log("   - Type safety: Strong typing prevents type confusion");
    console.log("   - Capability model: Least privilege by default");
    console.log("   - Resource limits: Prevent DoS attacks\n");
}

// Run all examples
function main() {
    safeFileOps();
    networkSecurity();
    envSecurity();
    subprocessSecurity();
    resourceLimits();
    permissionChecking();
    defenseInDepth();

    console.log("=== Security Demo Complete ===");
    console.log("\nQuicksilver: Secure by default, powerful when needed.");
}

main();
