"""
Quicksilver Python Bindings (via ctypes)

A zero-dependency Python wrapper for the Quicksilver JavaScript runtime.
Uses ctypes to load the shared library directly — no compilation needed.

Usage:
    from quicksilver import Runtime

    rt = Runtime()
    result = rt.eval("1 + 2 * 3")
    print(result)  # 7.0

    rt.set_global("name", "World")
    greeting = rt.eval("`Hello, ${name}!`")
    print(greeting)  # Hello, World!
"""

import ctypes
import ctypes.util
import os
import platform
from pathlib import Path
from typing import Any, Optional, Union


def _find_library() -> str:
    """Find the quicksilver shared library."""
    system = platform.system()
    if system == "Darwin":
        lib_name = "libquicksilver.dylib"
    elif system == "Windows":
        lib_name = "quicksilver.dll"
    else:
        lib_name = "libquicksilver.so"

    # Search paths in order of priority
    search_paths = [
        Path(__file__).parent / lib_name,
        Path(__file__).parent.parent / "target" / "release" / lib_name,
        Path(__file__).parent.parent / "target" / "debug" / lib_name,
    ]

    for path in search_paths:
        if path.exists():
            return str(path)

    # Try system library path
    found = ctypes.util.find_library("quicksilver")
    if found:
        return found

    raise RuntimeError(
        f"Could not find {lib_name}. "
        f"Build with: cargo build --release"
    )


class QuicksilverError(Exception):
    """Error raised by the Quicksilver runtime."""
    pass


class Runtime:
    """Quicksilver JavaScript runtime.

    Example:
        >>> rt = Runtime()
        >>> rt.eval("Math.sqrt(144)")
        12.0
        >>> rt.eval("'hello'.toUpperCase()")
        'HELLO'
    """

    def __init__(self, lib_path: Optional[str] = None):
        self._lib = ctypes.cdll.LoadLibrary(lib_path or _find_library())
        self._setup_bindings()
        self._rt = self._lib.qs_runtime_new()
        if not self._rt:
            raise QuicksilverError("Failed to create runtime")

    def _setup_bindings(self):
        lib = self._lib

        # Runtime
        lib.qs_runtime_new.restype = ctypes.c_void_p
        lib.qs_runtime_free.argtypes = [ctypes.c_void_p]

        # Eval
        lib.qs_eval.restype = ctypes.c_void_p
        lib.qs_eval.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_void_p]

        # Value creation
        lib.qs_value_undefined.restype = ctypes.c_void_p
        lib.qs_value_null.restype = ctypes.c_void_p
        lib.qs_value_boolean.restype = ctypes.c_void_p
        lib.qs_value_boolean.argtypes = [ctypes.c_bool]
        lib.qs_value_number.restype = ctypes.c_void_p
        lib.qs_value_number.argtypes = [ctypes.c_double]
        lib.qs_value_string.restype = ctypes.c_void_p
        lib.qs_value_string.argtypes = [ctypes.c_char_p]

        # Value inspection
        lib.qs_value_type.restype = ctypes.c_int
        lib.qs_value_type.argtypes = [ctypes.c_void_p]
        lib.qs_value_to_boolean.restype = ctypes.c_bool
        lib.qs_value_to_boolean.argtypes = [ctypes.c_void_p]
        lib.qs_value_to_number.restype = ctypes.c_double
        lib.qs_value_to_number.argtypes = [ctypes.c_void_p]
        lib.qs_value_to_string.restype = ctypes.c_char_p
        lib.qs_value_to_string.argtypes = [ctypes.c_void_p]

        # Memory
        lib.qs_value_free.argtypes = [ctypes.c_void_p]
        lib.qs_string_free.argtypes = [ctypes.c_char_p]

        # Globals
        lib.qs_global_set.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_void_p]
        lib.qs_global_get.restype = ctypes.c_void_p
        lib.qs_global_get.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

    def eval(self, source: str) -> Any:
        """Evaluate JavaScript and return the result as a Python value.

        Args:
            source: JavaScript source code to evaluate.

        Returns:
            The result converted to an appropriate Python type.

        Raises:
            QuicksilverError: If evaluation fails.
        """
        # Create error struct (message pointer + code)
        error_buf = ctypes.create_string_buffer(256)

        result_ptr = self._lib.qs_eval(
            self._rt,
            source.encode("utf-8"),
            None,  # We check for null result instead
        )

        if not result_ptr:
            raise QuicksilverError(f"Evaluation failed: {source[:100]}")

        try:
            return self._value_to_python(result_ptr)
        finally:
            self._lib.qs_value_free(result_ptr)

    def set_global(self, name: str, value: Any):
        """Set a global variable in the runtime.

        Args:
            name: Variable name.
            value: Python value (converted to JS automatically).
        """
        val_ptr = self._python_to_value(value)
        try:
            self._lib.qs_global_set(self._rt, name.encode("utf-8"), val_ptr)
        finally:
            self._lib.qs_value_free(val_ptr)

    def get_global(self, name: str) -> Any:
        """Get a global variable from the runtime."""
        val_ptr = self._lib.qs_global_get(self._rt, name.encode("utf-8"))
        if not val_ptr:
            return None
        try:
            return self._value_to_python(val_ptr)
        finally:
            self._lib.qs_value_free(val_ptr)

    def _value_to_python(self, val_ptr) -> Any:
        """Convert a QsValue pointer to a Python value."""
        type_tag = self._lib.qs_value_type(val_ptr)
        if type_tag == 0:  # Undefined
            return None
        elif type_tag == 1:  # Null
            return None
        elif type_tag == 2:  # Boolean
            return self._lib.qs_value_to_boolean(val_ptr)
        elif type_tag == 3:  # Number
            return self._lib.qs_value_to_number(val_ptr)
        elif type_tag == 4:  # String
            s = self._lib.qs_value_to_string(val_ptr)
            if s:
                result = s.decode("utf-8")
                return result
            return ""
        else:
            # Objects, arrays, functions → return as string representation
            s = self._lib.qs_value_to_string(val_ptr)
            if s:
                return s.decode("utf-8")
            return "[object]"

    def _python_to_value(self, value: Any):
        """Convert a Python value to a QsValue pointer."""
        if value is None:
            return self._lib.qs_value_null()
        elif isinstance(value, bool):
            return self._lib.qs_value_boolean(value)
        elif isinstance(value, (int, float)):
            return self._lib.qs_value_number(float(value))
        elif isinstance(value, str):
            return self._lib.qs_value_string(value.encode("utf-8"))
        else:
            return self._lib.qs_value_string(str(value).encode("utf-8"))

    def __del__(self):
        if hasattr(self, "_rt") and self._rt:
            self._lib.qs_runtime_free(self._rt)
            self._rt = None

    def __enter__(self):
        return self

    def __exit__(self, *args):
        if self._rt:
            self._lib.qs_runtime_free(self._rt)
            self._rt = None


if __name__ == "__main__":
    # Quick demo
    with Runtime() as rt:
        print(rt.eval("1 + 2 * 3"))
        print(rt.eval("'hello'.toUpperCase()"))
        print(rt.eval("Math.sqrt(144)"))
        rt.set_global("x", 42)
        print(rt.eval("x * 2"))
