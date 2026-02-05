/*
 * Quicksilver JavaScript Runtime - C API Header
 *
 * This header provides the public C interface for embedding Quicksilver
 * in non-Rust applications.
 *
 * Usage:
 *   #include "quicksilver.h"
 *   QsRuntime* rt = qs_runtime_new();
 *   QsError err = {0};
 *   QsValue* result = qs_eval(rt, "1 + 2 * 3", &err);
 *   if (result) {
 *       double num = qs_value_to_number(result);
 *       printf("Result: %f\n", num);
 *       qs_value_free(result);
 *   } else {
 *       printf("Error: %s\n", err.message);
 *       qs_error_free(&err);
 *   }
 *   qs_runtime_free(rt);
 *
 * Build:
 *   cargo build --release
 *   Link against target/release/libquicksilver.a (static) or .so/.dylib (dynamic)
 */

#ifndef QUICKSILVER_H
#define QUICKSILVER_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* === Opaque Types === */

/** Opaque runtime handle. Create with qs_runtime_new(), free with qs_runtime_free(). */
typedef struct QsRuntime QsRuntime;

/** Opaque value handle. Free with qs_value_free(). */
typedef struct QsValue QsValue;

/* === Error Handling === */

/** Error information returned from API calls */
typedef struct {
    char* message;  /**< Error message (free with qs_error_free) */
    int32_t code;   /**< Error code: 0=none, 1=invalid_input, 2=eval_error */
} QsError;

/* === Value Type Tags === */

/** Type tag for QsValue */
typedef enum {
    QS_TYPE_UNDEFINED = 0,
    QS_TYPE_NULL      = 1,
    QS_TYPE_BOOLEAN   = 2,
    QS_TYPE_NUMBER    = 3,
    QS_TYPE_STRING    = 4,
    QS_TYPE_OBJECT    = 5,
    QS_TYPE_ARRAY     = 6,
    QS_TYPE_FUNCTION  = 7,
    QS_TYPE_BIGINT    = 8,
    QS_TYPE_SYMBOL    = 9,
} QsValueType;

/* === Runtime Management === */

/** Create a new Quicksilver runtime. Returns NULL on allocation failure. */
QsRuntime* qs_runtime_new(void);

/** Free a runtime and all associated resources. Safe to call with NULL. */
void qs_runtime_free(QsRuntime* rt);

/* === Evaluation === */

/**
 * Evaluate JavaScript source code.
 *
 * @param rt     Valid runtime handle
 * @param source Null-terminated UTF-8 JavaScript source
 * @param error  Optional error output (can be NULL)
 * @return       Value handle on success, NULL on error. Free with qs_value_free().
 */
QsValue* qs_eval(QsRuntime* rt, const char* source, QsError* error);

/* === Value Creation === */

QsValue* qs_value_undefined(void);
QsValue* qs_value_null(void);
QsValue* qs_value_boolean(bool val);
QsValue* qs_value_number(double val);
QsValue* qs_value_string(const char* val);
QsValue* qs_value_object(void);
QsValue* qs_value_array(void);

/* === Value Inspection === */

/** Get the type tag of a value */
QsValueType qs_value_type(const QsValue* val);

/** Convert to boolean (JS truthiness rules) */
bool qs_value_to_boolean(const QsValue* val);

/** Convert to number (JS ToNumber semantics) */
double qs_value_to_number(const QsValue* val);

/** Convert to string. Caller must free with qs_string_free(). */
char* qs_value_to_string(const QsValue* val);

/** Strict equality check (===) */
bool qs_value_strict_equals(const QsValue* a, const QsValue* b);

/* === Object Operations === */

/** Set a property on an object. Returns false if obj is not an object. */
bool qs_object_set(QsValue* obj, const char* key, const QsValue* val);

/** Get a property from an object. Returns undefined if not found. Free result. */
QsValue* qs_object_get(const QsValue* obj, const char* key);

/* === Array Operations === */

/** Push a value onto an array. Returns false if not an array. */
bool qs_array_push(QsValue* arr, const QsValue* val);

/** Get array length. Returns -1 if not an array. */
int32_t qs_array_length(const QsValue* arr);

/** Get array element by index. Returns undefined if out of bounds. Free result. */
QsValue* qs_array_get(const QsValue* arr, int32_t index);

/* === Global Variables === */

/** Set a global variable in the runtime */
void qs_global_set(QsRuntime* rt, const char* name, const QsValue* val);

/** Get a global variable. Returns undefined if not found. Free result. */
QsValue* qs_global_get(const QsRuntime* rt, const char* name);

/* === Memory Management === */

/** Free a value handle. Safe to call with NULL. */
void qs_value_free(QsValue* val);

/** Free a string returned by qs_value_to_string(). Safe to call with NULL. */
void qs_string_free(char* s);

/** Free an error's message. Safe to call with NULL. */
void qs_error_free(QsError* err);

/* === Version Info === */

/** Get the Quicksilver version string. Do NOT free the returned pointer. */
const char* qs_version(void);

#ifdef __cplusplus
}
#endif

#endif /* QUICKSILVER_H */
