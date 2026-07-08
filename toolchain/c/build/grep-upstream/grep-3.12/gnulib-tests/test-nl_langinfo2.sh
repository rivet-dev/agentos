#!/bin/sh

# The name of a specific traditional locale.
: "${LOCALE_FR=fr_FR}"

# The name of a specific UTF-8 locale.
: "${LOCALE_FR_UTF8=fr_FR.UTF-8}"

# Make them accessible as environment variables.
export LOCALE_FR LOCALE_FR_UTF8

${CHECKER} ./test-nl_langinfo2${EXEEXT}
