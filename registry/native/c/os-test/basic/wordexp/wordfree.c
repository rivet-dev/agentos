/* Test whether a basic wordfree invocation works. */

#include <stdlib.h>
#include <string.h>
#include <wordexp.h>

#include "../basic.h"

int main(void)
{
	if ( setenv("FOO", "bar qux", 1) < 0 )
		err(1, "setenv");
	wordexp_t we = { .we_offs = 3 };
	int ret = wordexp("foo bar $FOO \"$FOO\" `echo $FOO`", &we, WRDE_DOOFFS);
	if ( ret == WRDE_BADCHAR )
		errx(1, "bad character");
	else if ( ret == WRDE_BADVAL )
		errx(1, "undefined variable");
	else if ( ret == WRDE_CMDSUB )
		errx(1, "denied command execution");
	else if ( ret == WRDE_CMDSUB )
		errx(1, "out of memoey");
	else if ( ret != 0 )
		errx(1, "wordexp failed weirdly");
	wordfree(&we);
	return 0;
}
