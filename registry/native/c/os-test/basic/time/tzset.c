/* Test whether a basic tzset invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	tzset();
	return 0;
}
