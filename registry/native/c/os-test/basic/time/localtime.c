/* Test whether a basic localtime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t time = 0;
	struct tm* tm = localtime(&time);
	if ( !tm )
		err(1, "localtime");
	return 0;
}
