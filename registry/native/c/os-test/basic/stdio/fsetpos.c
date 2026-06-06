/* Test whether a basic fsetpos invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	size_t amount = fwrite("foo", 1, 3, fp);
	if ( amount != 3 )
		err(1, "fwrite");
	fpos_t pos;
	if ( fgetpos(fp, &pos) )
		err(1, "fgetpos");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	amount = fwrite("x", 1, 1, fp);
	if ( amount != 1 )
		err(1, "fwrite");
	if ( fsetpos(fp, &pos) )
		err(1, "fsetpos");
	amount = fwrite("bar", 1, 4, fp);
	if ( amount != 4 )
		err(1, "fwrite");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	char buf[16];
	amount = fread(buf, 1, sizeof(buf) - 1, fp);
	if ( ferror(fp) )
		err(1, "fread");
	buf[amount] = '\0';
	const char* expected = "xoobar";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "got '%s' instead of '%s'", buf, expected);
	return 0;
}
