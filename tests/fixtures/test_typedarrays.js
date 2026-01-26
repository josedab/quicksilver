// Test TypedArrays and ArrayBuffer
console.log("=== TypedArray Test ===");

// Test ArrayBuffer
let buffer = new ArrayBuffer(16);
console.log("ArrayBuffer byteLength:", buffer.byteLength);

// Test Uint8Array
let u8 = new Uint8Array(4);
console.log("Uint8Array length:", u8.length);
console.log("Uint8Array BYTES_PER_ELEMENT:", u8.BYTES_PER_ELEMENT);

u8[0] = 1;
u8[1] = 2;
u8[2] = 3;
u8[3] = 4;
console.log("Uint8Array elements:", u8[0], u8[1], u8[2], u8[3]);

// Test Int16Array
let i16 = new Int16Array(3);
i16[0] = -1000;
i16[1] = 0;
i16[2] = 1000;
console.log("Int16Array elements:", i16[0], i16[1], i16[2]);
console.log("Int16Array byteLength:", i16.byteLength);

// Test Float64Array
let f64 = new Float64Array(2);
f64[0] = 3.14159;
f64[1] = 2.71828;
console.log("Float64Array elements:", f64[0], f64[1]);

// Test creating TypedArray from array
let fromArray = new Uint8Array([10, 20, 30, 40, 50]);
console.log("From array:", fromArray[0], fromArray[1], fromArray[2], fromArray[3], fromArray[4]);
console.log("From array length:", fromArray.length);

// Test Uint8ClampedArray (clamps values to 0-255)
let clamped = new Uint8ClampedArray(3);
clamped[0] = -10;    // Should clamp to 0
clamped[1] = 300;    // Should clamp to 255
clamped[2] = 128;    // Normal value
console.log("Uint8ClampedArray clamped values:", clamped[0], clamped[1], clamped[2]);

// Test creating TypedArray from ArrayBuffer
let sharedBuffer = new ArrayBuffer(8);
let view1 = new Uint8Array(sharedBuffer);
let view2 = new Uint32Array(sharedBuffer);

view1[0] = 0xFF;
view1[1] = 0x00;
view1[2] = 0x00;
view1[3] = 0x00;
console.log("Shared buffer Uint32 view:", view2[0]); // Should be 255 (little endian)

// Test ArrayBuffer.isView
console.log("ArrayBuffer.isView(u8):", ArrayBuffer.isView(u8));
console.log("ArrayBuffer.isView({}):", ArrayBuffer.isView({}));

console.log("=== TypedArray Test Complete ===");
