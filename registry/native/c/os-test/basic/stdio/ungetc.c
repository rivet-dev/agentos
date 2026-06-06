/* Test whether a basic ungetc invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buf[] = "foo";
	FILE* fp = fmemopen(buf, sizeof(buf), "r");
	if ( !fp )
		err(1, "fmemopen");
	if ( ungetc('X', fp) == EOF )
		err(1, "ungetc");
	char out[1 + sizeof(buf)];
	if ( !fgets(out, sizeof(out), fp) )
		err(1, "fgets"); 
	const char* expected = "Xfoo";
	if ( strcmp(out, expected) != 0 )
		errx(1, "got '%s' instead of '%s'", out, expected);
	return 0;
}
