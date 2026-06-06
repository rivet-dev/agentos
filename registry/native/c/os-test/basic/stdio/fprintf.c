/* Test whether a basic fprintf invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fprintf(fp, "hello %s %d", "world", 42) < 0 )
		err(1, "fprintf");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	char buf[256];
	size_t amount = fread(buf, 1, sizeof(buf) - 1, fp);
	if ( ferror(fp) )
		err(1, "fread");
	buf[amount] = '\0';
	const char* expected = "hello world 42";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "fprintf wrote '%s' instead of '%s'", buf, expected);
	return 0;
}
