#!/bin/sh

# Test the UTF-8 environment on native Windows.
unset LC_ALL
unset LC_CTYPE
unset LANG
${CHECKER} ./test-localename-w32utf8${EXEEXT}
