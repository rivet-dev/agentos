/*[CPT]*/
/* Test whether a basic clock_getcpuclockid invocation works. */

#include <time.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	clockid_t id;
	int errnum = clock_getcpuclockid(getpid(), &id);
	if ( errnum  )
	{
		errno = errnum;
		err(1, "clock_getcpuclockid");
	}
	return 0;
}
