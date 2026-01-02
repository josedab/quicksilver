// Test URL and URLSearchParams
console.log("=== URL and URLSearchParams Test ===");

// Test URL constructor
console.log("\nTesting URL constructor:");
let url = new URL("https://example.com:8080/path/to/file?query=value&foo=bar#section");
console.log("URL created:", typeof url);

// Test URL properties
console.log("\nURL properties:");
console.log("href:", url.href);
console.log("protocol:", url.protocol);
console.log("host:", url.host);
console.log("hostname:", url.hostname);
console.log("port:", url.port);
console.log("pathname:", url.pathname);
console.log("search:", url.search);
console.log("hash:", url.hash);
console.log("origin:", url.origin);

// Test URL with auth
console.log("\nURL with authentication:");
let authUrl = new URL("https://user:pass@example.com/path");
console.log("username:", authUrl.username);
console.log("password:", authUrl.password);

// Test URL with base
console.log("\nURL with base:");
let relativeUrl = new URL("/api/users", "https://example.com");
console.log("relative URL href:", relativeUrl.href);

// Test URLSearchParams from string
console.log("\nURLSearchParams from string:");
let params = new URLSearchParams("foo=bar&baz=qux&hello=world");
console.log("URLSearchParams created:", typeof params);
console.log("size:", params.size);

// Test URLSearchParams methods
console.log("\nURLSearchParams methods:");

// get
console.log("get('foo'):", params.get("foo"));
console.log("get('nonexistent'):", params.get("nonexistent"));

// has
console.log("has('foo'):", params.has("foo"));
console.log("has('nonexistent'):", params.has("nonexistent"));

// getAll
let params2 = new URLSearchParams("a=1&a=2&a=3&b=4");
console.log("getAll('a'):", params2.getAll("a"));

// append
params.append("newKey", "newValue");
console.log("After append, get('newKey'):", params.get("newKey"));
console.log("After append, size:", params.size);

// set
params.set("foo", "updated");
console.log("After set, get('foo'):", params.get("foo"));

// delete
params.delete("baz");
console.log("After delete, has('baz'):", params.has("baz"));

// toString
console.log("toString():", params.toString());

// Test forEach
console.log("\nforEach:");
let forEachParams = new URLSearchParams("x=1&y=2");
forEachParams.forEach(function(value, key) {
    console.log("  " + key + "=" + value);
});

// Test entries iteration (using for...of)
console.log("\nfor...of entries:");
let entryParams = new URLSearchParams("one=1&two=2");
for (let entry of entryParams.entries()) {
    console.log("  " + entry[0] + "=" + entry[1]);
}

// Test keys iteration
console.log("\nfor...of keys:");
for (let key of entryParams.keys()) {
    console.log("  key: " + key);
}

// Test values iteration
console.log("\nfor...of values:");
for (let val of entryParams.values()) {
    console.log("  value: " + val);
}

// Test sort
console.log("\nTest sort:");
let unsorted = new URLSearchParams("z=3&a=1&m=2");
unsorted.sort();
console.log("After sort:", unsorted.toString());

// Test searchParams from URL
console.log("\nSearchParams from URL:");
let searchParams = url.searchParams;
console.log("searchParams type:", typeof searchParams);
console.log("searchParams.get('query'):", searchParams.get("query"));
console.log("searchParams.get('foo'):", searchParams.get("foo"));

console.log("\n=== URL and URLSearchParams Test Complete ===");
