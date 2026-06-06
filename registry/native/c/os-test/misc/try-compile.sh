#!/bin/sh
set -e

COMPILE=$1
FILE=$2
LDFLAGS=$3
EXTRA_LDFLAGS=$4
OS=$5
OUT_PATH=$6

COMPILE="$COMPILE -Wall -Wextra -Werror -Wno-error=deprecated -Wno-error=deprecated-declarations"

mkdir -p -- "$(dirname "$OUT_PATH/$FILE")"
rm -f "$FILE" "$OUT_PATH/$FILE.o" "$OUT_PATH/$FILE.err" "$OUT_PATH/$FILE.out"
echo "$COMPILE $FILE.c -o $FILE -D_POSIX_C_SOURCE=202405L $LDFLAGS $EXTRA_LDFLAGS" > "$OUT_PATH/$FILE.err"
if ! $COMPILE -c "$FILE.c" -o "$OUT_PATH/$FILE.o" -D_POSIX_C_SOURCE=202405L 2>> "$OUT_PATH/$FILE.err" 1>&2; then
  if ! $COMPILE -c "$FILE.c" -o "$OUT_PATH/$FILE.o" -D_POSIX_C_SOURCE=200809L 2> /dev/null 1>&2; then
    if ! $COMPILE -c "$FILE.c" -o "$OUT_PATH/$FILE.o" -D_GNU_SOURCE -D_BSD_SOURCE -D_ALL_SOURCE -D_DEFAULT_SOURCE 2> /dev/null 1>&2; then
      rm -f "$OUT_PATH/$FILE.o"
      if grep -Eq '^/\*optional\*/$' "$FILE.c"; then
        echo "missing_optional" > "$OUT_PATH/$FILE.out"
      elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'fatal error' > /dev/null; then
        echo "missing_header" > "$OUT_PATH/$FILE.out"
      elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'incompatible|pointer-sign' > /dev/null; then
        echo "incompatible" > "$OUT_PATH/$FILE.out"
      elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'undeclared|no member named|is not defined' > /dev/null; then
        echo "undeclared" > "$OUT_PATH/$FILE.out"
      elif grep -E 'error:' "$OUT_PATH/$FILE.err" | grep -Ev 'type specifier missing,' | head -1 | grep -E 'unknown type name|Wvisibility|expected declaration specifiers|function cannot return function type|storage size of|declared inside parameter list|tentative definition has type|expected identifier|a parameter list without types|parameter names \(without types\) in function declaration' > /dev/null; then
        echo "unknown_type" > "$OUT_PATH/$FILE.out"
      else
        echo "compile_error" > "$OUT_PATH/$FILE.out"
      fi
      exit 0
    else
      echo "extension" > "$OUT_PATH/$FILE.out"
    fi
  else
    echo "previous_posix" > "$OUT_PATH/$FILE.out"
  fi
else
  echo "good" > "$OUT_PATH/$FILE.out"
fi
if ! $COMPILE "$OUT_PATH/$FILE.o" -o "$FILE" $LDFLAGS 2> /dev/null; then
  if ! $COMPILE "$OUT_PATH/$FILE.o" -o "$FILE" $LDFLAGS $EXTRA_LDFLAGS 2>> "$OUT_PATH/$FILE.err" 1>&2; then
    rm -f "$OUT_PATH/$FILE.o" "$FILE"
    echo "undefined" > "$OUT_PATH/$FILE.out"
    exit 0
  else
    echo "outside_libc" > "$OUT_PATH/$FILE.out"
  fi
fi
rm -f "$FILE" "$OUT_PATH/$FILE.o" "$OUT_PATH/$FILE.err"
