/* Test whether a basic setbuf invocation works. */

#include <sys/stat.h>

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	setbuf(fp, NULL);
	if ( fputc('x', fp) == EOF )
		err(1, "fputc");
	struct stat st;
	if ( fstat(fileno(fp), &st) < 0 )
		err(1, "fstat");
	if ( st.st_size != 1 )
		err(1, "setbuf(NULL) was not unbuffered");
	return 0;
}
