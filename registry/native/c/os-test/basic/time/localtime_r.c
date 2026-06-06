/* Test whether a basic localtime_r invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t time = 0;
	struct tm storage;
	struct tm* tm = localtime_r(&time, &storage);
	if ( !tm )
		err(1, "localtime_r");
	return 0;
}
