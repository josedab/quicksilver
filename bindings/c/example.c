/* Example: Embedding Quicksilver in a C application */
#include <stdio.h>
#include "quicksilver.h"

int main(void) {
    /* Create a runtime */
    QsRuntime* rt = qs_runtime_new();
    if (!rt) {
        fprintf(stderr, "Failed to create runtime\n");
        return 1;
    }

    /* Evaluate a simple expression */
    QsError err = {0};
    QsValue* result = qs_eval(rt, "1 + 2 * 3", &err);
    if (result) {
        printf("1 + 2 * 3 = %f\n", qs_value_to_number(result));
        qs_value_free(result);
    } else {
        fprintf(stderr, "Error: %s\n", err.message);
        qs_error_free(&err);
    }

    /* Set a global variable and use it */
    QsValue* greeting = qs_value_string("Hello from C!");
    qs_global_set(rt, "message", greeting);
    qs_value_free(greeting);

    result = qs_eval(rt, "message.toUpperCase()", &err);
    if (result) {
        char* str = qs_value_to_string(result);
        printf("Result: %s\n", str);
        qs_string_free(str);
        qs_value_free(result);
    }

    /* Run a function */
    qs_eval(rt, "function fib(n) { return n <= 1 ? n : fib(n-1) + fib(n-2); }", &err);
    result = qs_eval(rt, "fib(10)", &err);
    if (result) {
        printf("fib(10) = %f\n", qs_value_to_number(result));
        qs_value_free(result);
    }

    /* Clean up */
    qs_runtime_free(rt);
    return 0;
}
