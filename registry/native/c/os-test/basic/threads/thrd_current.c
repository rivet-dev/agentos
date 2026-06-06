/* Test whether a basic thrd_current invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	thrd_current();
	return 0;
}
