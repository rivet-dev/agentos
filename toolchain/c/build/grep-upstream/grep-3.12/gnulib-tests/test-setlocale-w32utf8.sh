#!/bin/sh

# Test the UTF-8 environment on native Windows.
unset LC_ALL
unset LC_CTYPE
unset LC_MESSAGES
unset LC_NUMERIC
unset LC_COLLATE
unset LC_MONETARY
unset LC_TIME
unset LANG
${CHECKER} ./test-setlocale-w32utf8${EXEEXT}
