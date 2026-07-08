#!/bin/sh

tmpfiles=""
trap 'rm -fr $tmpfiles' HUP INT QUIT TERM

tmpfiles="t-c-stack2.tmp"

# Sanitize exit status within a subshell, since some shells fail to
# redirect stderr on their message about death due to signal.
(${CHECKER} ./test-c-stack${EXEEXT} 1; exit $?) 2> t-c-stack2.tmp

case $? in
  77) if grep 'stack overflow' t-c-stack2.tmp >/dev/null ; then
        if test -z "$LIBSIGSEGV"; then
          echo 'cannot tell stack overflow from crash; consider installing libsigsegv' >&2
          exit 77
        else
          echo 'cannot tell stack overflow from crash, in spite of libsigsegv' >&2
          exit 1
        fi
      else
        cat t-c-stack2.tmp >&2
        exit 77
      fi
      ;;
  1)
      # Dereferencing NULL exits the program with status 1,
      # so this test doesn't check the c-stack testing harness like it should.
      # https://lists.gnu.org/r/grep-devel/2020-09/msg00034.html
      cat t-c-stack2.tmp >&2
      echo 'skipping test (perhaps gcc -fsanitize=undefined is in use?)'
      exit 77;;
  0) (exit 1); exit 1 ;;
esac
if grep 'program error' t-c-stack2.tmp >/dev/null ; then
  :
else
  (exit 1); exit 1
fi

rm -fr $tmpfiles

exit 0
