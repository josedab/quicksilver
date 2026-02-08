#!/usr/bin/env bash
set -euo pipefail

echo "Running all Quicksilver tests..."
echo ""

FAILED=0

run_suite() {
    local name=$1
    shift
    echo "━━━ $name ━━━"
    if "$@" 2>&1 | tail -1; then
        echo ""
    else
        FAILED=1
        echo "❌ $name FAILED"
        echo ""
    fi
}

run_suite "Unit Tests" cargo test --lib
run_suite "Integration Tests" cargo test --test integration_tests
run_suite "Doc Tests" cargo test --doc

echo "━━━━━━━━━━━━━━━━━━━━━━"
if [ $FAILED -eq 0 ]; then
    echo "✅ All test suites passed"
else
    echo "❌ Some tests failed"
    exit 1
fi
