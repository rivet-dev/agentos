/* Test whether a basic sleep invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	sleep(0);
	return 0;
}
