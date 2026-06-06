/* Test whether a basic fread invocation works. */

#include <stdio.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	size_t amount = fwrite("foo\0", 1, 4, fp);
	if ( amount != 4 )
		err(1, "fwrite");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	char buf[8];
	amount = fread(buf, 1, sizeof(buf), fp);
	if ( ferror(fp) )
		err(1, "fread");
	const char* expected = "foo";
	if ( amount != strlen(expected) + 1 )
		err(1, "fread read wrong amount of data");
	if ( strcmp(buf, expected) != 0 )
		err(1, "fread read wrong data");
	return 0;
}
