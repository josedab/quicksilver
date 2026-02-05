# Quicksilver Language Bindings

Foreign function interface (FFI) bindings for embedding Quicksilver in other languages.

## Prerequisites

Build the shared library first:

```bash
cd /path/to/quicksilver
cargo build --release
# Produces: target/release/libquicksilver.{dylib,so,dll}
#           target/release/libquicksilver.a (static)
```

## Available Bindings

### C (`c/`)

Static and dynamic library bindings with a comprehensive C API.

- **`quicksilver.h`** — Header file with full API documentation
- **`example.c`** — Example embedding usage

```c
#include "quicksilver.h"

int main() {
    qs_runtime_t* rt = qs_runtime_new();
    qs_value_t* result = qs_eval(rt, "1 + 2");
    double num = qs_value_to_number(result);
    printf("Result: %f\n", num);
    qs_value_free(result);
    qs_runtime_free(rt);
    return 0;
}
```

Compile with:
```bash
gcc -o example example.c -L../../target/release -lquicksilver
```

### Python (`python/`)

Python bindings via `ctypes` — no compilation needed.

```python
from quicksilver import Runtime

rt = Runtime()
result = rt.eval("Math.sqrt(144)")
print(result)  # 12.0
```

Requires `libquicksilver.dylib`/`.so` to be built and on `LD_LIBRARY_PATH`.

### Go (`go/`)

Go bindings via `cgo`.

```go
package main

import "quicksilver"

func main() {
    rt := quicksilver.NewRuntime()
    defer rt.Free()
    result, _ := rt.Eval("'Hello from Go!'")
    println(result)
}
```

Requires `libquicksilver.a` (static) or shared library to be built.

## Rust API (Native)

For Rust projects, use Quicksilver directly as a library:

```rust
use quicksilver::runtime::Runtime;

fn main() -> quicksilver::error::Result<()> {
    let mut runtime = Runtime::new();
    let result = runtime.eval("1 + 2")?;
    println!("{:?}", result);
    Ok(())
}
```

See also: `src/c_api/mod.rs` for the full C FFI implementation.
