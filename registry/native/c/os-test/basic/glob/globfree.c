/* Test whether a basic globfree invocation works. */

#include <glob.h>

#include "../basic.h"

int main(void)
{
	glob_t gl = { .gl_offs = 3 };
	int ret = glob("gl[o]b/gl?b*", GLOB_ERR | GLOB_DOOFFS, NULL, &gl);
	if ( ret == GLOB_ABORTED )
		err(1, "glob was aborted");
	else if ( ret == GLOB_NOMATCH )
		errx(1, "glob did not match");
	else if ( ret == GLOB_NOSPACE )
		errx(1, "glob was oom");
	else if ( ret != 0 )
		errx(1, "glob failed weirdly");
	globfree(&gl);
	return 0;
}
