/* Test whether a basic setvbuf invocation works. */

#include <sys/stat.h>

#include <stdio.h>

#include "../basic.h"

static char buf[BUFSIZ];

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( setvbuf(fp, buf, _IOLBF, sizeof(buf)) )
		err(1, "setvbuf");
	struct stat st;
	if ( fstat(fileno(fp), &st) < 0 )
		err(1, "first fstat");
	if ( st.st_size != 0 )
		errx(1, "wrong size after first fstat");
	size_t amount = fwrite("foo", 1, 3, fp);
	if ( amount != 3 )
		err(1, "fwrite");
	if ( fstat(fileno(fp), &st) < 0 )
		err(1, "second fstat");
	if ( st.st_size != 0 )
		errx(1, "wrong size after second fstat");
	amount = fwrite("bar\n", 1, 4, fp);
	if ( amount != 4 )
		err(1, "fwrite");
	if ( fstat(fileno(fp), &st) < 0 )
		err(1, "third fstat");
	if ( st.st_size != 7 )
		errx(1, "wrong size after third fstat");
	return 0;
}
