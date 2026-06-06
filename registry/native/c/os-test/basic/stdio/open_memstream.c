/* Test whether a basic open_memstream invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char* buf;
	size_t size;
	FILE* fp = open_memstream(&buf, &size);
	if ( !fp )
		err(1, "open_memstream");
	if ( fflush(fp) == EOF )
		err(1, "first fflush");
	if ( !buf )
		errx(1, "second check: buf is NULL");
	if ( size != 0 )
		errx(1, "second check: size = %zu, expected %zu", size, 0);
	if ( fprintf(fp, "hello %s %d", "world", 42) < 0 )
		err(1, "first fprintf");
	if ( fflush(fp) == EOF )
		err(1, "first fflush");
	if ( !buf )
		errx(1, "second check: buf is NULL");
	const char* expected1 = "hello world 42";
	if ( size != strlen(expected1) )
		errx(1, "second check: size = %zu, expected %zu", size, expected1);
	if ( strcmp(buf, expected1) != 0 )
		err(1, "second check: buf is '%s' instead of '%s'", buf, expected1);
	if ( fprintf(fp, " cool") < 0 )
		err(1, "second fprintf");
	if ( fclose(fp) == EOF )
		err(1, "fclose");
	const char* expected2 = "hello world 42 cool";
	if ( size != strlen(expected2) )
		errx(1, "second check: size = %zu, expected %zu", size, expected2);
	if ( strcmp(buf, expected2) != 0 )
		err(1, "second check: buf is '%s' instead of '%s'", buf, expected2);
	free(buf);
	return 0;
}
