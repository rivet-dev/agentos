/* Test whether a basic putc_unlocked invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	flockfile(fp);
	int c = 'x';
	if ( putc_unlocked(c, fp) == EOF )
		err(1, "putc_unlocked");
	if ( fflush(fp) == EOF )
		err(1, "fflush");
	funlockfile(fp);
	return 0;
}
