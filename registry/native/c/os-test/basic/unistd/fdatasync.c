/*[SIO]*/
/* Test whether a basic fdatasync invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fdatasync(fileno(fp)) < 0 )
		err(1, "fdatasync");
	return 0;
}
