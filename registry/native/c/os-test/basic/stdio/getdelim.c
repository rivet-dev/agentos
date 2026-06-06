/* Test whether a basic getdelim invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buf[] = "foo\nbar\0qux";
	FILE* fp = fmemopen(buf, sizeof(buf) - 1, "r");
	if ( !fp )
		err(1, "fmemopen");
	char* line = NULL;
	size_t size = 0;
	ssize_t length = getdelim(&line, &size, '\0', fp);
	if ( length < 0 )
		err(1, "first getdelim");
	if ( ferror(fp) )
		errx(1, "first getdelim did not fail but ferror is true");
	if ( !line )
		errx(1, "first getdelim did not set line");
	if ( (size_t) length >= size )
		errx(1, "first getdelim returned length larger than size");
	if ( (size_t) length != strlen(line) + 1 )
		errx(1, "first getdelim returned wrong length");
	const char* expected1 = "foo\nbar";
	if ( strcmp(line, expected1) != 0 )
		errx(1, "first getdelim gave '%s' instead of '%s'", line, expected1);
	length = getdelim(&line, &size, '\0', fp);
	if ( length < 0 )
		err(1, "second getdelim");
	if ( ferror(fp) )
		errx(1, "second getdelim did not fail but ferror is true");
	if ( !line )
		errx(1, "second getdelim did not set line");
	if ( (size_t) length >= size )
		errx(1, "second getdelim returned length larger than size");
	if ( (size_t) length != strlen(line) )
		errx(1, "second getdelim returned wrong length");
	const char* expected2 = "qux";
	if ( strcmp(line, expected2) != 0 )
		errx(1, "second getdelim gave '%s' instead of '%s'", line, expected2);
	return 0;
}
