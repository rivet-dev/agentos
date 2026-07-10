#!/bin/sh

# Test whether a specific UTF-8 locale is installed.
: "${LOCALE_EN_UTF8=en_US.UTF-8}"
: "${LOCALE_FR_UTF8=fr_FR.UTF-8}"
if test "$LOCALE_EN_UTF8" = none && test $LOCALE_FR_UTF8 = none; then
  if test -f /usr/bin/localedef; then
    echo "Skipping test: no english or french Unicode locale is installed"
  else
    echo "Skipping test: no english or french Unicode locale is supported"
  fi
  exit 77
fi

# It's sufficient to test in one of the two locales.
if test $LOCALE_FR_UTF8 != none; then
  testlocale=$LOCALE_FR_UTF8
else
  testlocale="$LOCALE_EN_UTF8"
fi

LC_ALL="$testlocale" \
${CHECKER} ./test-mbsrtowcs${EXEEXT} 3
