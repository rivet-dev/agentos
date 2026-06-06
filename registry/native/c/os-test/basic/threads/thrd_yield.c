/* Test whether a basic thrd_yield invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	thrd_yield();
	return 0;
}
