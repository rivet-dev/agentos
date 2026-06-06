/* Test whether a basic times invocation works. */

#include <sys/times.h>

#include "../basic.h"

int main(void)
{
	struct tms tms;
	if ( times(&tms) == (clock_t) -1 )
		err(1, "times");
	return 0;
}
