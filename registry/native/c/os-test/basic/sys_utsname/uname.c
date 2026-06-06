/* Test whether a basic uname invocation works. */

#include <sys/utsname.h>

#include "../basic.h"

int main(void)
{
	struct utsname uts;
	if ( uname(&uts) < 0 )
		err(1, "uname");
	return 0;
}
