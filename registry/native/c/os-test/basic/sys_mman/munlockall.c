/*[ML]*/
/* Test whether a basic munlockall invocation works. */

#include <sys/mman.h>

#include "../basic.h"

int main(void)
{
	if ( mlockall(MCL_CURRENT | MCL_FUTURE) < 0 )
	{
		if ( errno == EPERM || errno == ENOMEM )
			return 0;
		err(1, "mlockall");
	}
	if ( munlockall() < 0 )
		err(1, "munlockall");
	return 0;
}
