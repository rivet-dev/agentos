/* Test whether a basic sched_yield invocation works. */

#include <sched.h>

#include "../basic.h"

int main(void)
{
	if ( sched_yield() < 0 )
		err(1, "sched_yield");
	return 0;
}
