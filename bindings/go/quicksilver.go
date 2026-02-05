// Package quicksilver provides Go bindings for the Quicksilver JavaScript runtime.
//
// This package uses cgo to call the Quicksilver C API. You must have the
// Quicksilver shared library built and accessible.
//
// Build the library first:
//
//	cd /path/to/quicksilver && cargo build --release
//
// Usage:
//
//	rt := quicksilver.New()
//	defer rt.Close()
//	result, err := rt.Eval("1 + 2 * 3")
//	fmt.Println(result) // 7
package quicksilver

/*
#cgo LDFLAGS: -L../../target/release -lquicksilver -lm -ldl -lpthread
#include "../../bindings/c/quicksilver.h"
#include <stdlib.h>
*/
import "C"
import (
	"fmt"
	"runtime"
	"unsafe"
)

// Runtime represents a Quicksilver JavaScript runtime instance.
type Runtime struct {
	rt *C.QsRuntime
}

// New creates a new Quicksilver runtime.
func New() *Runtime {
	rt := C.qs_runtime_new()
	if rt == nil {
		panic("quicksilver: failed to create runtime")
	}
	r := &Runtime{rt: rt}
	runtime.SetFinalizer(r, (*Runtime).Close)
	return r
}

// Close frees the runtime and all associated resources.
func (r *Runtime) Close() {
	if r.rt != nil {
		C.qs_runtime_free(r.rt)
		r.rt = nil
	}
}

// Eval evaluates JavaScript source code and returns the result.
func (r *Runtime) Eval(source string) (interface{}, error) {
	cs := C.CString(source)
	defer C.free(unsafe.Pointer(cs))

	var cerr C.QsError
	result := C.qs_eval(r.rt, cs, &cerr)

	if result == nil {
		msg := "evaluation failed"
		if cerr.message != nil {
			msg = C.GoString(cerr.message)
			C.qs_error_free(&cerr)
		}
		return nil, fmt.Errorf("quicksilver: %s", msg)
	}
	defer C.qs_value_free(result)

	return valueToGo(result), nil
}

// SetGlobal sets a global variable in the runtime.
func (r *Runtime) SetGlobal(name string, value interface{}) {
	cn := C.CString(name)
	defer C.free(unsafe.Pointer(cn))

	val := goToValue(value)
	defer C.qs_value_free(val)

	C.qs_global_set(r.rt, cn, val)
}

// GetGlobal gets a global variable from the runtime.
func (r *Runtime) GetGlobal(name string) interface{} {
	cn := C.CString(name)
	defer C.free(unsafe.Pointer(cn))

	result := C.qs_global_get(r.rt, cn)
	if result == nil {
		return nil
	}
	defer C.qs_value_free(result)

	return valueToGo(result)
}

func valueToGo(val *C.QsValue) interface{} {
	switch C.qs_value_type(val) {
	case C.QS_TYPE_UNDEFINED, C.QS_TYPE_NULL:
		return nil
	case C.QS_TYPE_BOOLEAN:
		return bool(C.qs_value_to_boolean(val))
	case C.QS_TYPE_NUMBER:
		return float64(C.qs_value_to_number(val))
	case C.QS_TYPE_STRING:
		cs := C.qs_value_to_string(val)
		if cs != nil {
			s := C.GoString(cs)
			C.qs_string_free(cs)
			return s
		}
		return ""
	default:
		cs := C.qs_value_to_string(val)
		if cs != nil {
			s := C.GoString(cs)
			C.qs_string_free(cs)
			return s
		}
		return "[object]"
	}
}

func goToValue(value interface{}) *C.QsValue {
	switch v := value.(type) {
	case nil:
		return C.qs_value_null()
	case bool:
		return C.qs_value_boolean(C.bool(v))
	case int:
		return C.qs_value_number(C.double(v))
	case int64:
		return C.qs_value_number(C.double(v))
	case float64:
		return C.qs_value_number(C.double(v))
	case string:
		cs := C.CString(v)
		defer C.free(unsafe.Pointer(cs))
		return C.qs_value_string(cs)
	default:
		cs := C.CString(fmt.Sprintf("%v", v))
		defer C.free(unsafe.Pointer(cs))
		return C.qs_value_string(cs)
	}
}
