/*[FSC]*/
/* Test whether a basic fsync invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fsync(fileno(fp)) < 0 )
		err(1, "fsync");
	return 0;
}
