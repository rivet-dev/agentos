/* Test whether a basic ftello invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( ftello(fp) != 0 )
		errx(1, "first ftello != 0");
	size_t amount = fwrite("foo\0", 1, 4, fp);
	if ( amount != 4 )
		err(1, "fwrite");
	if ( ftello(fp) != 4 )
		errx(1, "second ftello != 4");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	if ( ftello(fp) != 0 )
		errx(1, "third ftello != 0");
	char buf[8];
	amount = fread(buf, 1, sizeof(buf), fp);
	if ( ferror(fp) )
		err(1, "fread");
	const char* expected = "foo";
	if ( amount != strlen(expected) + 1 )
		err(1, "fread read wrong amount of data");
	if ( ftello(fp) != 4 )
		errx(1, "first ftello!= 4");
	return 0;
}
