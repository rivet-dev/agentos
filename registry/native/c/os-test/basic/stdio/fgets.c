/* Test whether a basic fgets invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buf[] = "foo\nbar\n";
	FILE* fp = fmemopen(buf, sizeof(buf), "r");
	if ( !fp )
		err(1, "fmemopen");
	char out[1 + sizeof(buf)];
	if ( !fgets(out, sizeof(out), fp) )
		err(1, "fgets"); 
	const char* expected = "foo\n";
	if ( strcmp(out, expected) != 0 )
		errx(1, "got '%s' instead of '%s'", out, expected);
	return 0;
}
