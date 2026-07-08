# Copyright (C) 1988 Henry Spencer.
# Copyright (C) 2009-2025 Free Software Foundation, Inc.
#
# Copying and distribution of this file, with or without modification,
# are permitted in any medium without royalty provided the copyright
# notice and this notice are preserved.

BEGIN {
        FS = "@";
        printf ("failures=0\n");
}

$0 !~ /^#/  && NF == 3 {
        test("en_US.UTF-8")
        test("ru_RU.KOI8-R")
        test("fr_FR.ISO-8859-1")
        test("zh_CN")
        if (extra_locale != "") {
                test(extra_locale)
        }
}

function test(locale)
{
        printf ("status=$(echo '%s'| { LC_ALL=%s grep -E -e '%s' >/dev/null 2>&1 ; echo $?; })\n",$3, locale, $2);
        printf ("if test $status -ne %s ; then\n", $1);
        printf ("\techo Spencer test \\#%d failed \\(%s\\)\n", ++n, locale);
        printf ("\tfailures=1\n");
        printf ("fi\n");
}

END { printf ("exit $failures\n"); }
