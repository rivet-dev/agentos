#!/bin/sh
set -e

COMPILE=$1
FILE=$2
LDFLAGS=$3
EXTRA_LDFLAGS=$4
OS=$5
OUT_PATH=$6

COMPILE="$COMPILE -Wall -Wextra -Werror=implicit-function-declaration"

mkdir -p -- "$(dirname "$OUT_PATH/$FILE")"
rm -f "$FILE" "$OUT_PATH/$FILE.o" "$OUT_PATH/$FILE.err" "$OUT_PATH/$FILE.out"
echo "$COMPILE $FILE.c -o $FILE -D_GNU_SOURCE -D_BSD_SOURCE -D_ALL_SOURCE -D_DEFAULT_SOURCE $LDFLAGS $EXTRA_LDFLAGS" > "$OUT_PATH/$FILE.err"
if ! $COMPILE -c "$FILE.c" -o "$OUT_PATH/$FILE.o" -D_GNU_SOURCE -D_BSD_SOURCE -D_ALL_SOURCE -D_DEFAULT_SOURCE 2>> "$OUT_PATH/$FILE.err" 1>&2; then
  rm -f "$OUT_PATH/$FILE.o"
  if grep -Eq '^/\*optional\*/$' "$FILE.c"; then
    outcome=missing_optional
  elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'fatal error' > /dev/null; then
    outcome=missing_header
  elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'incompatible|pointer-sign' > /dev/null; then
    outcome=incompatible
  elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'undeclared|no member named|is not defined' > /dev/null; then
    outcome=undeclared
  elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'unknown type name|Wvisibility|expected declaration specifiers|function cannot return function type|storage size of|declared inside parameter list|tentative definition has type|expected identifier|a parameter list without types|parameter names \(without types\) in function declaration' > /dev/null; then
    outcome=unknown_type
   else
    outcome=compile_error
  fi
  echo "echo $outcome" > "$FILE"
  chmod +x "$FILE"
  echo "$outcome" > "$OUT_PATH/$FILE.out"
  exit 0
fi
if ! $COMPILE "$OUT_PATH/$FILE.o" -o "$FILE" $LDFLAGS $EXTRA_LDFLAGS 2>> "$OUT_PATH/$FILE.err" 1>&2; then
  rm -f "$OUT_PATH/$FILE.o" "$FILE"
  outcome=undefined
  echo "echo $outcome" > "$FILE"
  chmod +x "$FILE"
  echo "$outcome" > "$OUT_PATH/$FILE.out"
  exit 0
fi
rm -f "$OUT_PATH/$FILE.o" "$OUT_PATH/$FILE.err" "$OUT_PATH/$FILE.out"
