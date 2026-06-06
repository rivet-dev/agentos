/* Test whether a basic glob invocation works. */

#include <glob.h>
#include <stdbool.h>

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
	// Test if gl_offs is respected.
	if ( gl.gl_offs != 3 )
		errx(1, "gl_offs != 3");
	for ( size_t i = 0; i < gl.gl_offs; i++ )
		if ( gl.gl_pathv[i] )
			errx(1, "gl_offs not respected");
	// glob is supposed to sort unless GLOB_NOSORT.
	bool found_glob = false;
	bool found_globfree = false;
	for ( size_t i = 0; i < gl.gl_pathc; i++ )
	{
		if ( !strcmp(gl.gl_pathv[gl.gl_offs + i], "glob/glob") )
		{
			if ( found_glob )
				errx(1, "found glob/glob twice");
			if ( found_globfree )
				errx(1, "found glob/globfree before glob/glob");
			found_glob = true;
		}
		else if ( !strcmp(gl.gl_pathv[gl.gl_offs + i], "glob/glob") )
		{
			if ( found_globfree )
				errx(1, "found glob/globfree twice");
			if ( !found_glob )
				errx(1, "found glob/globfree before glob/glob");
			found_globfree = true;
		}
	}
	if ( gl.gl_pathv[gl.gl_offs + gl.gl_pathc] )
		errx(1, "glob did not null terminate path list");
	return 0;
}
