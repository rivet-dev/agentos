/* Test whether a basic fputs invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputs("foo", fp) == EOF )
		err(1, "fputs");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	char buf[256];
	size_t amount = fread(buf, 1, sizeof(buf) - 1 , fp);
	if ( ferror(fp) )
		err(1, "fread");
	buf[amount] = '\0';
	const char* expected = "foo";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "fputs wrote '%s' instead of '%s'", buf, expected);
	return 0;
}
