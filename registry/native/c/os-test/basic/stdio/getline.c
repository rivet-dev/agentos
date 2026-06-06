/* Test whether a basic getline invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buf[] = "foo\nbar";
	FILE* fp = fmemopen(buf, sizeof(buf) - 1, "r");
	if ( !fp )
		err(1, "fmemopen");
	char* line = NULL;
	size_t size = 0;
	ssize_t length = getline(&line, &size, fp);
	if ( length < 0 )
		err(1, "first getline");
	if ( ferror(fp) )
		errx(1, "first getline did not fail but ferror is true");
	if ( !line )
		errx(1, "first getline did not set line");
	if ( (size_t) length >= size )
		errx(1, "first getline returned length larger than size");
	if ( (size_t) length != strlen(line) )
		errx(1, "first getline returned wrong length");
	const char* expected1 = "foo\n";
	if ( strcmp(line, expected1) != 0 )
		errx(1, "first getline gave '%s' instead of '%s'", line, expected1);
	length = getline(&line, &size, fp);
	if ( length < 0 )
		err(1, "second getline");
	if ( ferror(fp) )
		errx(1, "second getline did not fail but ferror is true");
	if ( !line )
		errx(1, "second getline did not set line");
	if ( (size_t) length >= size )
		errx(1, "second getline returned length larger than size");
	if ( (size_t) length != strlen(line) )
		errx(1, "second getline returned wrong length");
	const char* expected2 = "bar";
	if ( strcmp(line, expected2) != 0 )
		errx(1, "second getline gave '%s' instead of '%s'", line, expected2);
	return 0;
}
