#!/bin/sh

tmpfile=
trap 'rm -fr $tmpfile' HUP INT QUIT TERM

tmpfile=test-fpending.t

${CHECKER} ./test-fpending${EXEEXT} > $tmpfile || exit 1

rm -fr $tmpfile

exit 0
